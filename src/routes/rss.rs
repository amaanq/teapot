use axum::{
   Router,
   extract::{
      Path,
      Query,
      State,
   },
   response::Response,
   routing::get,
};
use serde::Deserialize;

use super::helpers::{
   cache_rss,
   check_rss_cache,
   get_cached_user,
   rss_response,
};
use crate::{
   AppState,
   cache::keys as cache_keys,
   error::{
      Error,
      Result,
   },
   views::rss as rss_view,
};

#[derive(Debug, Deserialize)]
pub struct RssQuery {
   pub cursor: Option<String>,
}

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/{username}/rss", get(user_rss))
      .route("/{username}/with_replies/rss", get(user_replies_rss))
      .route("/{username}/media/rss", get(user_media_rss))
      .route("/{username}/search/rss", get(user_search_rss))
      .route("/{username}/lists/{slug}/rss", get(list_by_slug_rss))
      .route("/search/rss", get(search_rss))
      .route("/i/lists/{id}/rss", get(list_rss))
}

/// Check if RSS is enabled; return 404 error if not.
fn check_rss_enabled(state: &AppState) -> Result<()> {
   if !state.config.config.enable_rss {
      return Err(Error::NotFound("RSS feeds are disabled".to_owned()));
   }
   Ok(())
}

/// Which user timeline variant to fetch for RSS.
enum UserRssKind {
   Tweets,
   Replies,
   Media,
}

/// Unified user RSS handler -- handles tweets, replies, and media variants.
async fn user_rss_handler(
   state: &AppState,
   username: &str,
   cursor: Option<&str>,
   kind: UserRssKind,
) -> Result<Response> {
   let (feed_kind, cache_key_fn): (&str, fn(&str) -> String) = match kind {
      UserRssKind::Tweets => ("tweets", cache_keys::rss_user),
      UserRssKind::Replies => ("replies", cache_keys::rss_replies),
      UserRssKind::Media => ("media", cache_keys::rss_media),
   };

   // Check RSS cache (first page only)
   let rss_cache_key = if cursor.is_none() {
      let key = cache_key_fn(username);
      if let Some(cached) = check_rss_cache(state, &key) {
         return Ok(cached);
      }
      Some(key)
   } else {
      None
   };

   let user = get_cached_user(state, username).await?;

   let timeline = match kind {
      UserRssKind::Tweets | UserRssKind::Replies => {
         state.api.get_user_tweets(&user.id, cursor).await?
      },
      UserRssKind::Media => state.api.get_user_media(&user.id, cursor).await?,
   };
   let tweets = timeline.content.into_iter().flatten().collect::<Vec<_>>();
   let rss = rss_view::render_user_rss(&user, &tweets, &state.config, feed_kind);

   if let Some(ref key) = rss_cache_key {
      cache_rss(state, key, &rss);
   }

   Ok(rss_response(rss, &tweets))
}

async fn user_rss(
   State(state): State<AppState>,
   Path(username): Path<String>,
   Query(query): Query<RssQuery>,
) -> Result<Response> {
   check_rss_enabled(&state)?;
   user_rss_handler(
      &state,
      &username,
      query.cursor.as_deref(),
      UserRssKind::Tweets,
   )
   .await
}

async fn user_replies_rss(
   State(state): State<AppState>,
   Path(username): Path<String>,
   Query(query): Query<RssQuery>,
) -> Result<Response> {
   check_rss_enabled(&state)?;
   user_rss_handler(
      &state,
      &username,
      query.cursor.as_deref(),
      UserRssKind::Replies,
   )
   .await
}

async fn user_media_rss(
   State(state): State<AppState>,
   Path(username): Path<String>,
   Query(query): Query<RssQuery>,
) -> Result<Response> {
   check_rss_enabled(&state)?;
   user_rss_handler(
      &state,
      &username,
      query.cursor.as_deref(),
      UserRssKind::Media,
   )
   .await
}

async fn search_rss(
   State(state): State<AppState>,
   Query(query): Query<super::search::SearchQuery>,
) -> Result<Response> {
   check_rss_enabled(&state)?;
   let search_query = query.query.as_deref().unwrap_or_default();
   if search_query.is_empty() {
      return Err(Error::InvalidUrl("Search query is required".into()));
   }

   let rss_cache_key = cache_keys::rss_search(search_query);
   if let Some(cached) = check_rss_cache(&state, &rss_cache_key) {
      return Ok(cached);
   }

   let timeline = state.api.search(search_query, None, "Latest").await?;
   let tweets = timeline.content.into_iter().flatten().collect::<Vec<_>>();
   let rss = rss_view::render_search_rss(search_query, &tweets, &state.config);

   cache_rss(&state, &rss_cache_key, &rss);
   Ok(rss_response(rss, &tweets))
}

/// User search RSS - search tweets from a specific user.
#[derive(Debug, Deserialize)]
pub struct UserSearchRssQuery {
   #[serde(rename = "q")]
   pub query: Option<String>,
}

async fn user_search_rss(
   State(state): State<AppState>,
   Path(username): Path<String>,
   Query(query): Query<UserSearchRssQuery>,
) -> Result<Response> {
   check_rss_enabled(&state)?;
   let search_query = query.query.as_deref().unwrap_or_default();
   if search_query.is_empty() {
      return Err(Error::InvalidUrl("Search query is required".into()));
   }

   let api_query = format!("from:{username} {search_query}");
   let rss_cache_key = cache_keys::rss_user_search(&username, search_query);
   if let Some(cached) = check_rss_cache(&state, &rss_cache_key) {
      return Ok(cached);
   }

   let timeline = state.api.search(&api_query, None, "Latest").await?;
   let tweets = timeline.content.into_iter().flatten().collect::<Vec<_>>();
   let rss = rss_view::render_search_rss(
      &format!("from:{username} {search_query}"),
      &tweets,
      &state.config,
   );

   cache_rss(&state, &rss_cache_key, &rss);
   Ok(rss_response(rss, &tweets))
}

async fn list_rss(
   State(state): State<AppState>,
   Path(id): Path<String>,
   Query(query): Query<RssQuery>,
) -> Result<Response> {
   check_rss_enabled(&state)?;
   let rss_cache_key = if query.cursor.is_none() {
      let key = cache_keys::rss_list(&id);
      if let Some(cached) = check_rss_cache(&state, &key) {
         return Ok(cached);
      }
      Some(key)
   } else {
      None
   };

   let (list, timeline) = tokio::join!(
      state.api.get_list(&id),
      state.api.get_list_tweets(&id, query.cursor.as_deref()),
   );
   let list = list?;
   let timeline = timeline?;
   let tweets = timeline.content.into_iter().flatten().collect::<Vec<_>>();
   let rss = rss_view::render_list_rss(&list, &tweets, &state.config);

   if let Some(ref key) = rss_cache_key {
      cache_rss(&state, key, &rss);
   }
   Ok(rss_response(rss, &tweets))
}

/// List RSS by owner username and slug.
async fn list_by_slug_rss(
   State(state): State<AppState>,
   Path((username, slug)): Path<(String, String)>,
   Query(query): Query<RssQuery>,
) -> Result<Response> {
   check_rss_enabled(&state)?;
   let rss_cache_key = if query.cursor.is_none() {
      let key = cache_keys::rss_list_slug(&username, &slug);
      if let Some(cached) = check_rss_cache(&state, &key) {
         return Ok(cached);
      }
      Some(key)
   } else {
      None
   };

   let list = state.api.get_list_by_slug(&username, &slug).await?;
   let timeline = state
      .api
      .get_list_tweets(&list.id, query.cursor.as_deref())
      .await?;
   let tweets = timeline.content.into_iter().flatten().collect::<Vec<_>>();
   let rss = rss_view::render_list_rss(&list, &tweets, &state.config);

   if let Some(ref key) = rss_cache_key {
      cache_rss(&state, key, &rss);
   }
   Ok(rss_response(rss, &tweets))
}
