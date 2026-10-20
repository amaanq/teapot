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
      LazyLock,
      RwLock,
   },
   time::{
      Duration,
      Instant,
   },
};

use axum::http::HeaderMap;
use regex::Regex;
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

const REFRESH_INTERVAL: Duration = Duration::from_hours(1);

/// Floor between attempts, so a failing bootstrap or a burst of WAF 404s does
/// not refetch the homepage on every request.
const RETRY_INTERVAL: Duration = Duration::from_mins(5);

const CHUNK_CONCURRENCY: usize = 8;

const CLIENT_WEB: &str = "https://abs.twimg.com/responsive-web/client-web/";

static MAIN_RE: LazyLock<Regex> = LazyLock::new(|| {
   Regex::new(r"https://abs\.twimg\.com/responsive-web/client-web/main\.[0-9a-f]+\.js").unwrap()
});
static PAIR_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r#"queryId:"([A-Za-z0-9_-]+)",operationName:"(\w+)""#).unwrap());
static CHUNK_MAP_RE: LazyLock<Regex> = LazyLock::new(|| {
   Regex::new(r#"\(\{([^{}]*)\}\)\[\w+\]\|\|\w+\)\+"\."\+\(\{([^{}]*)\}\)\[\w+\]\+"a\.js""#)
      .unwrap()
});
static CHUNK_ENTRY_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r#"(\d+):"([^"]+)""#).unwrap());

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
      let main_url = MAIN_RE
         .find(&home)
         .ok_or("no main.js in the x.com homepage")?
         .as_str()
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

fn collect_pairs(js: &str, found: &mut HashMap<String, String>) {
   for caps in PAIR_RE.captures_iter(js) {
      let operation = &caps[2];

      if endpoints::WEB_OPERATIONS
         .iter()
         .any(|endpoint| endpoint.ends_with(&format!("/{operation}")))
      {
         found.insert(operation.to_owned(), caps[1].to_owned());
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

fn chunk_urls(home: &str) -> Result<Vec<String>, String> {
   let caps = CHUNK_MAP_RE
      .captures(home)
      .ok_or("no webpack chunk map in the x.com homepage")?;
   let names: HashMap<&str, &str> = CHUNK_ENTRY_RE
      .captures_iter(caps.get(1).map_or("", |names| names.as_str()))
      .filter_map(|entry| Some((entry.get(1)?.as_str(), entry.get(2)?.as_str())))
      .collect();

   Ok(CHUNK_ENTRY_RE
      .captures_iter(caps.get(2).map_or("", |hashes| hashes.as_str()))
      .filter_map(|entry| {
         let id = entry.get(1)?.as_str();
         let name = names.get(id).copied().unwrap_or(id);
         let hash = entry.get(2)?.as_str();
         let skip = name.starts_with("i18n/") || name.starts_with("ondemand.");
         (!skip).then(|| format!("{CLIENT_WEB}{name}.{hash}a.js"))
      })
      .collect())
}
