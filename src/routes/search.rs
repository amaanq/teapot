use std::slice;

use axum::{
   Router,
   extract::State,
   http::header::CONTENT_TYPE,
   response::{
      IntoResponse as _,
      Response,
   },
   routing::get,
};
use percent_encoding::{
   NON_ALPHANUMERIC,
   utf8_percent_encode,
};
use topcoat::{
   context::Cx,
   router::{
      error::see_other,
      page,
      path_param,
      query_params,
      request::uri,
   },
   view::{
      BoxView,
      View,
      ViewExt as _,
      view,
   },
};

use crate::{
   AppState,
   cache::{
      keys as cache_keys,
      ttl,
   },
   error::Result,
   routes::helpers::{
      enrich_tweet_groups,
      request_prefs,
      state,
      swr_take,
   },
   types::{
      query::{
         Query,
         QueryKind,
         SearchProduct,
      },
      timeline::{
         PaginatedResult,
         Timeline,
      },
      user::User,
   },
   views::{
      document::document,
      search::{
         SearchFilters,
         search_results,
         user_search_results,
      },
   },
};

#[query_params(error = bad_request)]
#[derive(Default)]
pub struct SearchQuery {
   #[serde(rename = "q")]
   pub query:      Option<String>,
   #[serde(rename = "f")]
   pub filter:     Option<String>,
   pub cursor:     Option<String>,
   pub from:       Option<String>,
   pub since:      Option<String>,
   pub until:      Option<String>,
   pub min_faves:  Option<String>,
   #[serde(rename = "f-media")]
   pub f_media:    Option<String>,
   #[serde(rename = "f-images")]
   pub f_images:   Option<String>,
   #[serde(rename = "f-videos")]
   pub f_videos:   Option<String>,
   #[serde(rename = "f-links")]
   pub f_links:    Option<String>,
   #[serde(rename = "f-news")]
   pub f_news:     Option<String>,
   #[serde(rename = "f-quote")]
   pub f_quote:    Option<String>,
   #[serde(rename = "f-verified")]
   pub f_verified: Option<String>,
   #[serde(rename = "e-replies")]
   pub e_replies:  Option<String>,
   #[serde(rename = "e-retweets")]
   pub e_retweets: Option<String>,
}

impl SearchQuery {
   /// Convert URL parameters to `SearchFilters` for form rendering.
   pub fn to_filters(&self) -> SearchFilters {
      SearchFilters {
         media:            self.f_media.as_deref() == Some("on"),
         images:           self.f_images.as_deref() == Some("on"),
         videos:           self.f_videos.as_deref() == Some("on"),
         links:            self.f_links.as_deref() == Some("on"),
         news:             self.f_news.as_deref() == Some("on"),
         quote:            self.f_quote.as_deref() == Some("on"),
         verified:         self.f_verified.as_deref() == Some("on"),
         exclude_replies:  self.e_replies.as_deref() == Some("on"),
         exclude_retweets: self.e_retweets.as_deref() == Some("on"),
         since:            self.since.clone().unwrap_or_default(),
         until:            self.until.clone().unwrap_or_default(),
         min_faves:        self.min_faves.clone().unwrap_or_default(),
      }
   }

   /// Convert URL parameters to `Query` struct.
   fn to_query(&self) -> Query {
      let raw_query = self.query.as_deref().unwrap_or("");

      let kind = match self.filter.as_deref() {
         Some("replies") => QueryKind::Replies,
         Some("media") => QueryKind::Media,
         Some("users") => QueryKind::Users,
         Some("top") => QueryKind::Top,
         _ => QueryKind::Posts,
      };

      let mut query = Query::parse(raw_query, kind);

      // Add from user if specified as parameter
      if let Some(ref from) = self.from {
         for user in from.split(',') {
            let user = user.trim();
            if !user.is_empty() && !query.from_user.contains(&user.to_owned()) {
               query.from_user.push(user.to_owned());
            }
         }
      }

      if let Some(ref since) = self.since
         && query.since.is_empty()
      {
         query.since.clone_from(since);
      }
      if let Some(ref until) = self.until
         && query.until.is_empty()
      {
         query.until.clone_from(until);
      }
      if let Some(ref min) = self.min_faves
         && query.min_likes.is_empty()
      {
         query.min_likes.clone_from(min);
      }

      // Add filter toggles (data-driven to avoid repetition)
      let filter_toggles = &[
         (&self.f_media, "media"),
         (&self.f_images, "images"),
         (&self.f_videos, "videos"),
         (&self.f_links, "links"),
         (&self.f_news, "news"),
         (&self.f_quote, "quote"),
         (&self.f_verified, "verified"),
      ];
      for &(param, name) in filter_toggles {
         if param.as_deref() == Some("on") && !query.filters.iter().any(|filter| filter == name) {
            query.filters.push((*name).to_owned());
         }
      }

      let exclude_toggles = &[(&self.e_replies, "replies"), (&self.e_retweets, "retweets")];
      for &(param, name) in exclude_toggles {
         if param.as_deref() == Some("on") && !query.excludes.iter().any(|excl| excl == name) {
            query.excludes.push((*name).to_owned());
         }
      }

      query
   }
}

pub fn router() -> Router<AppState> {
   Router::new().route("/opensearch", get(opensearch))
}

path_param!(tag);

#[page("/search")]
pub async fn search(cx: &Cx) -> topcoat::Result<impl View> {
   let params = query_params::<SearchQuery>(cx)?;
   if let Some(qs) = uri(cx).query() {
      let clean = qs
         .split('&')
         .filter(|pair| pair.split_once('=').is_none_or(|(_, val)| !val.is_empty()))
         .collect::<Vec<&str>>();
      if clean.len() < qs.split('&').count() {
         let url = if clean.is_empty() {
            "/search".to_owned()
         } else {
            format!("/search?{}", clean.join("&"))
         };
         return Err(see_other(url).into());
      }
   }
   Box::pin(search_page(cx, params)).await
}

#[page("/hashtag/{tag}")]
pub async fn hashtag(cx: &Cx) -> topcoat::Result<impl View> {
   let query = query_params::<SearchQuery>(cx)?;
   let params = SearchQuery {
      query: Some(format!("#{}", path_param::<Tag>(cx))),
      filter: query.filter.clone(),
      cursor: query.cursor.clone(),
      ..Default::default()
   };
   Box::pin(search_page(cx, &params)).await
}

async fn search_page<'a>(cx: &'a Cx, params: &SearchQuery) -> topcoat::Result<BoxView<'a>> {
   let state = state(cx);
   let prefs = request_prefs(cx);
   let raw_q = params.query.clone().unwrap_or_default();
   let encoded_q = utf8_percent_encode(&raw_q, NON_ALPHANUMERIC).to_string();

   if raw_q.contains(',')
      && raw_q.split(',').all(|segment| {
         let trimmed = segment.trim();
         !trimmed.is_empty()
            && trimmed
               .chars()
               .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '@')
      })
   {
      let cleaned = raw_q
         .split(',')
         .map(|segment| segment.trim().trim_start_matches('@'))
         .collect::<Vec<_>>()
         .join(",");
      return Err(see_other(format!("/{cleaned}")).into());
   }

   let is_user_search = params.filter.as_deref() == Some("users");
   let filters = params.to_filters();

   if raw_q.is_empty() && params.from.is_none() {
      return Ok(view! { cx =>
         document(
            title: "Search",
            prefs: &prefs,
            if is_user_search {
               user_search_results(query: "", users: &[], prefs: &prefs)
            } else {
               search_results(
                  query: "",
                  tweets: &[],
                  prefs: &prefs,
                  active_tab: QueryKind::Posts,
                  filters: Some(&filters),
               )
            }
         )
      }
      .boxed());
   }

   if is_user_search {
      let result = search_users(state, &raw_q, params.cursor.as_deref()).await?;
      let newer_url = params
         .cursor
         .is_some()
         .then(|| format!("/search?q={encoded_q}&f=users"));
      let title = format!("Search ({raw_q}) | Users");
      let canonical = format!("https://x.com/search?q={encoded_q}&src=typed_query");
      let referer = format!("/search?q={encoded_q}&f=users");
      return Ok(view! { cx =>
         document(
            title: &title,
            prefs: &prefs,
            canonical: &canonical,
            referer: &referer,
            user_search_results(
               query: &raw_q,
               users: &result.content,
               prefs: &prefs,
               cursor: result.bottom.as_deref(),
               newer_url: newer_url.as_deref(),
            )
         )
      }
      .boxed());
   }

   let query = params.to_query();
   let api_query = query.build();
   let timeline = search_tweets(
      state,
      &api_query,
      query.kind.product(),
      params.cursor.as_deref(),
   )
   .await?;
   let mut tweets = timeline.content.into_iter().flatten().collect::<Vec<_>>();
   enrich_tweet_groups(state, slice::from_mut(&mut tweets)).await;

   // Display the original user query, not the API query
   let display_query = if query.has_filters() {
      api_query
   } else {
      raw_q.clone()
   };
   let referer = format!("/search?q={encoded_q}");
   let has_cursor = params.cursor.is_some();
   let title = format!("Search ({raw_q})");
   let rss = format!("/search/rss?f=tweets&q={encoded_q}");
   let canonical = format!("https://x.com/search?f=live&q={encoded_q}&src=typed_query");

   Ok(view! { cx =>
      document(
         title: &title,
         prefs: &prefs,
         rss: &rss,
         canonical: &canonical,
         referer: &referer,
         search_results(
            query: &display_query,
            tweets: &tweets,
            prefs: &prefs,
            active_tab: query.kind,
            cursor: timeline.bottom.as_deref(),
            filters: Some(&filters),
            newer_url: has_cursor.then_some(referer.as_str()),
         )
      )
   }
   .boxed())
}

async fn search_users(
   state: &AppState,
   raw_q: &str,
   cursor: Option<&str>,
) -> Result<PaginatedResult<User>> {
   if cursor.is_some() {
      return state.api.search_users(raw_q, cursor).await;
   }

   let cache_key = cache_keys::search_users(raw_q);
   let refresh = {
      let raw_q = raw_q.to_owned();
      let cache_key = cache_key.clone();
      move |app: AppState| {
         async move {
            if let Ok(data) = app.api.search_users(&raw_q, None).await {
               app.cache
                  .set_swr(&cache_key, &data, ttl::SEARCH, ttl::SEARCH_STALE);
            }
         }
      }
   };
   if let Some(cached) = swr_take::<PaginatedResult<User>, _, _>(state, &cache_key, refresh) {
      return Ok(cached);
   }
   let data = state.api.search_users(raw_q, None).await?;
   state
      .cache
      .set_swr(&cache_key, &data, ttl::SEARCH, ttl::SEARCH_STALE);
   Ok(data)
}

async fn search_tweets(
   state: &AppState,
   api_query: &str,
   product: SearchProduct,
   cursor: Option<&str>,
) -> Result<Timeline> {
   if cursor.is_some() {
      return state.api.search(api_query, cursor, product).await;
   }

   let cache_key = cache_keys::search_timeline(api_query, product);
   let refresh = {
      let api_query = api_query.to_owned();
      let cache_key = cache_key.clone();
      move |app: AppState| {
         async move {
            if let Ok(data) = app.api.search(&api_query, None, product).await {
               app.cache
                  .set_swr(&cache_key, &data, ttl::SEARCH, ttl::SEARCH_STALE);
            }
         }
      }
   };
   if let Some(cached) = swr_take::<Timeline, _, _>(state, &cache_key, refresh) {
      return Ok(cached);
   }
   let data = state.api.search(api_query, None, product).await?;
   state
      .cache
      .set_swr(&cache_key, &data, ttl::SEARCH, ttl::SEARCH_STALE);
   Ok(data)
}

async fn opensearch(State(state): State<AppState>) -> Response {
   let xml = format!(
      r#"<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <ShortName>{}</ShortName>
  <Description>Twitter search via {}</Description>
  <InputEncoding>UTF-8</InputEncoding>
  <Url type="text/html" template="{}/search?q={{searchTerms}}"/>
</OpenSearchDescription>"#,
      state.config.server.title,
      state.config.server.hostname,
      state.config.url_prefix()
   );

   (
      [(CONTENT_TYPE, "application/opensearchdescription+xml")],
      xml,
   )
      .into_response()
}
