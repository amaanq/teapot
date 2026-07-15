use axum::{
   http::header,
   response::{
      IntoResponse as _,
      Response,
   },
};

use crate::{
   AppState,
   cache::{
      keys as cache_keys,
      ttl,
   },
   error::Result,
   types::{
      Conversation,
      Timeline,
      Translation,
      Tweet,
      Tweets,
      User,
   },
};

#[derive(Clone)]
struct CachedRss {
   body:   String,
   min_id: Option<i64>,
}

/// Fetch a user, using cache when available.
pub async fn get_cached_user(state: &AppState, username: &str) -> Result<User> {
   let cache_key = cache_keys::user(username);
   if let Some(cached) = state.cache.get::<User>(&cache_key) {
      return Ok(cached);
   }
   let fetched = state.api.get_user(username).await?;
   state.cache.set(&cache_key, &fetched, ttl::DEFAULT);
   Ok(fetched)
}

/// Fetch one tweet, reusing either the conversation or tweet cache.
pub async fn get_cached_tweet(state: &AppState, id: &str) -> Result<Tweet> {
   if let Some(conversation) = state
      .cache
      .get::<Conversation>(&cache_keys::conversation(id))
   {
      return Ok(conversation.tweet);
   }
   let key = cache_keys::tweet(id);
   if let Some(tweet) = state.cache.get::<Tweet>(&key) {
      return Ok(tweet);
   }
   let tweet = state.api.get_tweet(id).await?;
   state.cache.set(&key, &tweet, ttl::DEFAULT);
   Ok(tweet)
}

/// Translate a tweet once per backend and cache the result.
pub async fn get_cached_translation(
   state: &AppState,
   tweet: &Tweet,
   kagi_token: Option<&str>,
) -> Result<Translation> {
   let backend = if kagi_token.is_some() {
      "kagi"
   } else {
      "strato"
   };
   let key = cache_keys::translation(tweet.id, backend);
   if let Some(translation) = state.cache.get::<Translation>(&key) {
      return Ok(translation);
   }
   let _permit = state.translation_limiter.acquire().await?;
   let translation = state.api.translate_auto(tweet, kagi_token).await?;
   if !translation.text.is_empty() {
      state.cache.set(&key, &translation, ttl::TRANSLATION);
   }
   Ok(translation)
}

/// Build an RSS response with `Content-Type` and `Min-Id` headers.
pub fn rss_response(rss: String, tweets: &[Tweet]) -> Response {
   let min_id = tweets.iter().map(|tweet| tweet.id).min();
   rss_response_with_min_id(rss, min_id)
}

/// Build an RSS response when the caller already has the minimum ID.
pub fn rss_response_with_min_id(rss: String, min_id: Option<i64>) -> Response {
   let mut response = (
      [(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")],
      rss,
   )
      .into_response();
   if let Some(id) = min_id {
      response.headers_mut().insert(
         header::HeaderName::from_static("min-id"),
         header::HeaderValue::from(id),
      );
   }
   response
}

/// Check RSS cache and return early if hit.
pub fn check_rss_cache(state: &AppState, key: &str) -> Option<Response> {
   let cached = state.cache.get::<CachedRss>(key)?;
   Some(rss_response_with_min_id(cached.body, cached.min_id))
}

/// Cache an RSS result.
pub fn cache_rss(state: &AppState, key: &str, rss: &str, min_id: Option<i64>) {
   let cached = CachedRss {
      body: rss.to_owned(),
      min_id,
   };
   state
      .cache
      .set(key, &cached, state.config.cache.rss_minutes * 60);
}

/// Extract tweet groups and cursor from a Timeline.
/// Preserves conversation grouping. Each inner [`Vec<Tweet>`] is a conversation
/// thread (parent → reply chain) from a single `profile-conversation-*` entry.
pub fn extract_timeline(timeline: Timeline) -> (Vec<Tweets>, Option<String>) {
   (timeline.content, timeline.bottom)
}
