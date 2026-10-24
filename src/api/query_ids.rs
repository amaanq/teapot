//! Live GraphQL query IDs scraped from X's logged-in web client.
//!
//! X retires a query ID by answering it with a bodyless 404, so the IDs
//! compiled into [`endpoints`] rot with every web deploy. The logged-in
//! `x.com/home` names `main.<hash>.js` and embeds webpack's lazy chunk map, and
//! those bundles carry every `queryId`/`operationName` pair the web app uses.
//!
//! - Endpoint strings stay the stable key for budgets and rate limits; only the
//!   URL of a cookie request swaps in the live ID.
//! - OAuth sessions call `api.x.com` with app IDs the web bundle never ships,
//!   so they keep the compiled ones.

use std::{
   borrow::Cow,
   collections::HashMap,
   sync::{
      Arc,
      RwLock,
   },
   time::{
      Duration,
      Instant,
   },
};

use axum::http::HeaderMap;
use tokio::{
   sync::Mutex,
   task::JoinSet,
};

use super::{
   auth::SessionPool,
   endpoints,
   http::HttpClient,
   tid,
};
use crate::utils::formatters::{
   find_matches,
   is_word_char,
};

const REFRESH_INTERVAL: Duration = Duration::from_hours(1);

/// Floor between attempts, so a failing bootstrap or a burst of WAF 404s does
/// not refetch the homepage on every request.
const RETRY_INTERVAL: Duration = Duration::from_mins(5);

const CHUNK_CONCURRENCY: usize = 8;

const CLIENT_WEB: &str = "https://abs.twimg.com/responsive-web/client-web/";

struct RefreshState {
   next:     Instant,
   last_try: Option<Instant>,
   main_url: Option<String>,
}

#[derive(Clone)]
pub struct QueryIds {
   live:     Arc<RwLock<HashMap<String, String>>>,
   state:    Arc<Mutex<RefreshState>>,
   http:     HttpClient,
   sessions: SessionPool,
}

impl QueryIds {
   pub fn new(http: HttpClient, sessions: SessionPool) -> Self {
      Self {
         live: Arc::default(),
         state: Arc::new(Mutex::new(RefreshState {
            next:     Instant::now(),
            last_try: None,
            main_url: None,
         })),
         http,
         sessions,
      }
   }

   /// The endpoint path to request, with the live query ID when one is known.
   pub fn resolve<'a>(&self, endpoint: &'a str) -> Cow<'a, str> {
      self.refresh_if_due();

      let Some((compiled, operation)) = endpoint.split_once('/') else {
         return Cow::Borrowed(endpoint);
      };
      let live = self.live.read().unwrap();

      match live.get(operation) {
         Some(id) if id != compiled => Cow::Owned(format!("{id}/{operation}")),
         _ => Cow::Borrowed(endpoint),
      }
   }

   /// Pull the next refresh forward after an upstream 404 that may be a retired
   /// query ID.
   pub fn mark_stale(&self) {
      let Ok(mut state) = self.state.try_lock() else {
         return;
      };
      let earliest = state
         .last_try
         .map_or_else(Instant::now, |last| last + RETRY_INTERVAL);
      state.next = state.next.min(earliest);
   }

   fn refresh_if_due(&self) {
      let Ok(mut state) = Arc::clone(&self.state).try_lock_owned() else {
         return;
      };

      if Instant::now() < state.next {
         return;
      }

      let this = self.clone();
      tokio::spawn(async move {
         let now = Instant::now();
         state.last_try = Some(now);

         match this.scan(state.main_url.as_deref()).await {
            Ok(Some((main_url, found))) => {
               this.publish(found);
               state.main_url = Some(main_url);
               state.next = now + REFRESH_INTERVAL;
            },
            Ok(None) => state.next = now + REFRESH_INTERVAL,
            Err(err) => {
               tracing::warn!("Failed to refresh GraphQL query IDs: {err}");
               state.next = now + RETRY_INTERVAL;
            },
         }
      });
   }

   fn publish(&self, found: HashMap<String, String>) {
      for endpoint in endpoints::WEB_OPERATIONS {
         let Some((compiled, operation)) = endpoint.split_once('/') else {
            continue;
         };

         match found.get(operation) {
            Some(id) if id != compiled => {
               tracing::info!(operation, compiled, live = %id, "GraphQL query ID moved");
            },
            Some(_) => {},
            None => tracing::warn!(operation, "GraphQL operation missing from the web bundle"),
         }
      }

      *self.live.write().unwrap() = found;
   }

   /// Returns [`None`] when `main.js` is the one already scanned, since its
   /// content-hashed name only changes with a deploy.
   async fn scan(
      &self,
      scanned_main: Option<&str>,
   ) -> Result<Option<(String, HashMap<String, String>)>, String> {
      let (home, headers) = tid::fetch_home(&self.http, &self.sessions).await?;
      let main_url = main_js_url(&home)
         .ok_or("no main.js in the x.com homepage")?
         .to_owned();

      if scanned_main == Some(main_url.as_str()) {
         return Ok(None);
      }

      let mut found = HashMap::new();
      let main_js = fetch_text(&self.http, &main_url, &headers).await?;
      collect_pairs(&main_js, &mut found);

      // Every lazily loaded operation teapot uses sat in a `shared~` chunk on
      // 2026-10-20.
      let mut chunks = chunk_urls(&home)?;
      chunks.sort_by_key(|url| !url[CLIENT_WEB.len()..].starts_with("shared~"));

      let mut pending = chunks.into_iter();

      while !all_found(&found) {
         let mut batch = JoinSet::new();

         for url in pending.by_ref().take(CHUNK_CONCURRENCY) {
            let http = self.http.clone();
            let headers = headers.clone();
            batch.spawn(async move { fetch_text(&http, &url, &headers).await });
         }

         if batch.is_empty() {
            break;
         }

         while let Some(joined) = batch.join_next().await {
            match joined {
               Ok(Ok(js)) => collect_pairs(&js, &mut found),
               Ok(Err(err)) => tracing::debug!("skipping chunk: {err}"),
               Err(err) => return Err(format!("chunk task failed: {err}")),
            }
         }
      }

      Ok(Some((main_url, found)))
   }
}

async fn fetch_text(http: &HttpClient, url: &str, headers: &HeaderMap) -> Result<String, String> {
   http
      .get_with_headers(url, headers)
      .await
      .map_err(|err| format!("fetch {url}: {err}"))?
      .text()
      .await
      .map_err(|err| format!("read {url}: {err}"))
}

fn main_js_url(home: &str) -> Option<&str> {
   let prefix = format!("{CLIENT_WEB}main.");
   home.match_indices(&prefix).find_map(|(start, _)| {
      let hash_start = start + prefix.len();
      let hash_len = home[hash_start..]
         .find(|ch: char| !matches!(ch, '0'..='9' | 'a'..='f'))
         .unwrap_or(home.len() - hash_start);
      let end = hash_start + hash_len + ".js".len();
      (hash_len > 0 && home[hash_start + hash_len..].starts_with(".js")).then(|| &home[start..end])
   })
}

fn word_len(text: &str) -> Option<usize> {
   let len = text.find(|ch| !is_word_char(ch)).unwrap_or(text.len());
   (len > 0).then_some(len)
}

fn query_pairs(js: &str) -> impl Iterator<Item = (&str, &str)> {
   js.match_indices(r#"queryId:""#)
      .filter_map(|(start, needle)| {
         let id_and_rest = &js[start + needle.len()..];
         let id_len = id_and_rest
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')))
            .unwrap_or(id_and_rest.len());
         let operation_and_rest = id_and_rest[id_len..].strip_prefix(r#"",operationName:""#)?;
         let operation_len = word_len(operation_and_rest)?;
         operation_and_rest[operation_len..]
            .starts_with('"')
            .then_some(())?;
         (id_len > 0).then(|| (&id_and_rest[..id_len], &operation_and_rest[..operation_len]))
      })
}

fn collect_pairs(js: &str, found: &mut HashMap<String, String>) {
   for (id, operation) in query_pairs(js) {
      if endpoints::WEB_OPERATIONS
         .iter()
         .any(|endpoint| endpoint.ends_with(&format!("/{operation}")))
      {
         found.insert(operation.to_owned(), id.to_owned());
      }
   }
}

fn all_found(found: &HashMap<String, String>) -> bool {
   endpoints::WEB_OPERATIONS.iter().all(|endpoint| {
      endpoint
         .split_once('/')
         .is_some_and(|(_, operation)| found.contains_key(operation))
   })
}

fn chunk_map(home: &str) -> Option<(&str, &str)> {
   let object = |text: &'_ str| -> Option<(usize, usize)> {
      let len = text.find(['{', '}'])?;
      text[len..].starts_with('}').then_some((len, len + 1))
   };
   home.match_indices("({").find_map(|(start, _)| {
      let names_and_rest = &home[start + 2..];
      let (names_len, names_end) = object(names_and_rest)?;
      let after_names = names_and_rest[names_end..].strip_prefix(")[")?;
      let after_or = after_names[word_len(after_names)?..].strip_prefix("]||")?;
      let hashes_and_rest = after_or[word_len(after_or)?..].strip_prefix(r#")+"."+({"#)?;
      let (hashes_len, hashes_end) = object(hashes_and_rest)?;
      let after_hashes = hashes_and_rest[hashes_end..].strip_prefix(")[")?;
      after_hashes[word_len(after_hashes)?..]
         .starts_with(r#"]+"a.js""#)
         .then(|| (&names_and_rest[..names_len], &hashes_and_rest[..hashes_len]))
   })
}

fn chunk_entries(object: &str) -> impl Iterator<Item = (&str, &str)> {
   let is_digit = |ch: char| ch.is_numeric() && is_word_char(ch) && !ch.is_alphabetic();
   find_matches(object, move |pos| {
      let digits_and_rest = &object[pos..];
      let digits_len = digits_and_rest
         .find(|ch| !is_digit(ch))
         .unwrap_or(digits_and_rest.len());
      let value_and_rest = digits_and_rest[digits_len..].strip_prefix(r#":""#)?;
      let value_len = value_and_rest.find('"').filter(|&len| len > 0)?;
      let end = object.len() - value_and_rest.len() + value_len + 1;
      (digits_len > 0).then(|| {
         (
            end,
            (&digits_and_rest[..digits_len], &value_and_rest[..value_len]),
         )
      })
   })
   .map(|(_, _, entry)| entry)
}

fn chunk_urls(home: &str) -> Result<Vec<String>, String> {
   let (names_object, hashes_object) =
      chunk_map(home).ok_or("no webpack chunk map in the x.com homepage")?;
   let names = chunk_entries(names_object).collect::<HashMap<&str, &str>>();

   Ok(chunk_entries(hashes_object)
      .filter_map(|(id, hash)| {
         let name = names.get(id).copied().unwrap_or(id);
         let skip = name.starts_with("i18n/") || name.starts_with("ondemand.");
         (!skip).then(|| format!("{CLIENT_WEB}{name}.{hash}a.js"))
      })
      .collect())
}
