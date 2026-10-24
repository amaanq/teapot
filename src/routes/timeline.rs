#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[page] and view! expansions"
)]

use std::cmp;

use percent_encoding::{
   NON_ALPHANUMERIC,
   utf8_percent_encode,
};
use topcoat::{
   context::Cx,
   router::{
      page,
      path_param,
      query_params,
   },
   view::{
      View,
      ViewExt as _,
      view,
   },
};

use crate::{
   AppState,
   api::budget,
   cache::{
      keys as cache_keys,
      ttl,
   },
   error::{
      Error,
      Result,
   },
   routes::helpers::{
      apply_account_context,
      enrich_tweet_groups,
      extract_timeline,
      get_cached_user,
      request_prefs,
      state,
      swr_take,
   },
   types::{
      query::SearchProduct,
      timeline::{
         Profile,
         Timeline,
         TimelineKind,
      },
      tweet::GalleryPhoto,
   },
   utils::formatters,
   views::{
      document::document,
      profile::profile_page,
      search::search_panel,
      timeline::{
         tab_to_kind,
         timeline,
      },
   },
};

/// Reserved path names that cannot be usernames.
const RESERVED_PATHS: &[&str] = &[
   "pic",
   "gif",
   "video",
   "search",
   "settings",
   "login",
   "intent",
   "i",
   "about",
   "explore",
   "help",
   "oauth",
   "oauth2",
   "saveprefs",
   "resetprefs",
   "enablemp4",
   "public",
   "css",
   "js",
   "fonts",
   "embed",
];

/// Validate username contains only valid characters.
fn is_valid_username(name: &str) -> bool {
   !name.is_empty()
      && name
         .chars()
         .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == ',')
      && !name.contains('.')
}

/// Check if a username is actually a reserved path.
fn is_reserved_path(name: &str) -> bool {
   let lower = name.to_lowercase();
   RESERVED_PATHS.iter().any(|&reserved| reserved == lower)
}

/// Split comma-separated usernames and validate each.
fn parse_usernames(name: &str) -> Option<Vec<String>> {
   let names = name
      .split(',')
      .map(|part| part.trim().to_owned())
      .filter(|part| !part.is_empty())
      .collect::<Vec<_>>();

   if names.is_empty()
      || names
         .iter()
         .any(|n| !is_valid_username(n) || is_reserved_path(n))
   {
      return None;
   }

   Some(names)
}

#[query_params(error = bad_request)]
struct TimelineQuery {
   tab:    Option<String>,
   cursor: Option<String>,
   /// For AJAX infinite scroll requests - returns only tweet HTML.
   scroll: Option<String>,
}

#[query_params(error = bad_request)]
struct UserSearchQuery {
   #[serde(rename = "q")]
   query:  Option<String>,
   cursor: Option<String>,
}

path_param!(username);

#[page("/{username}")]
pub async fn user_timeline(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let username = path_param::<Username>(cx);
   let query = query_params::<TimelineQuery>(cx)?;

   if is_reserved_path(username) {
      return Err(Error::InvalidUrl(format!("'{username}' is a reserved path")).into());
   }
   if username.contains(',') {
      return Ok(multi_user_timeline(cx, username).await?.boxed());
   }
   if !is_valid_username(username) {
      return Err(Error::InvalidUrl("Invalid username format".into()).into());
   }

   let tab = query.tab.as_deref().unwrap_or("tweets");
   if !["tweets", "with_replies", "media", "search", ""].contains(&tab) {
      return Err(Error::InvalidUrl("Invalid tab parameter".into()).into());
   }

   let prefs = request_prefs(cx);
   let mut profile = fetch_profile(state, username, query.cursor.as_deref()).await?;
   apply_account_context(state, &mut profile.user).await;
   let base_url = format!("/{}", profile.user.username);

   if query.scroll.as_deref() == Some("true") {
      let (mut tweets, cursor) = extract_timeline(profile.tweets);
      enrich_tweet_groups(state, &mut tweets).await;
      return Ok(view! {
         timeline(groups: &tweets, prefs: &prefs, cursor: cursor.as_deref(), base_url: &base_url)
      }
      .boxed());
   }

   let kind = tab_to_kind(tab);
   let user = &profile.user;
   let title = format!("{} (@{})", user.fullname, user.username);
   let description = format!("The latest tweets from {}", user.fullname);
   let canonical = format!("https://x.com/{}", user.username);
   let rss_url = format!("/{}/rss", user.username);
   let og_image = format!(
      "{}{}",
      state.config.url_prefix(),
      formatters::get_pic_url(
         &user.user_pic.replace("_normal", "_400x400"),
         state.config.config.base64_media,
      )
   );
   let has_cursor = query.cursor.is_some();

   Ok(view! {
      let pinned = profile.pinned.as_ref().filter(|_| kind == TimelineKind::Tweets);
      document(
         title: &title,
         prefs: &prefs,
         description: &description,
         rss: &rss_url,
         canonical: &canonical,
         referer: &base_url,
         og_image: &og_image,
         og_type: "object",
         profile_page(
            user: &profile.user,
            photo_rail: &profile.photo_rail,
            prefs: &prefs,
            tab: kind,
            timeline(
               groups: &profile.tweets.content,
               prefs: &prefs,
               cursor: profile.tweets.bottom.as_deref(),
               base_url: &base_url,
               pinned: pinned,
               newer_url: has_cursor.then_some(base_url.as_str()),
            )
         )
      )
   }
   .boxed())
}

async fn fetch_profile(state: &AppState, username: &str, cursor: Option<&str>) -> Result<Profile> {
   if cursor.is_some() {
      return state.api.get_profile(username, cursor).await;
   }

   let cache_key = cache_keys::profile(username);
   let refresh = {
      let username = username.to_owned();
      let cache_key = cache_key.clone();
      move |app: AppState| {
         async move {
            if let Ok(profile) = app.api.get_profile(&username, None).await {
               app.cache
                  .set_swr(&cache_key, &profile, ttl::DEFAULT, ttl::DEFAULT_STALE);
            }
         }
      }
   };
   if let Some(cached) = swr_take::<Profile, _, _>(state, &cache_key, refresh) {
      tracing::debug!("Cache hit for profile: {username}");
      return Ok(cached);
   }

   let profile = state.api.get_profile(username, None).await?;
   state
      .cache
      .set_swr(&cache_key, &profile, ttl::DEFAULT, ttl::DEFAULT_STALE);
   Ok(profile)
}

async fn fetch_tab(
   state: &AppState,
   user_id: &str,
   username: &str,
   cursor: Option<&str>,
   tab: TimelineKind,
) -> Result<Timeline> {
   let fetch = |cursor| {
      async move {
         if tab == TimelineKind::Media {
            state.api.get_user_media(user_id, cursor).await
         } else {
            state.api.get_user_tweets_and_replies(user_id, cursor).await
         }
      }
   };
   if cursor.is_some() {
      return fetch(cursor).await;
   }

   let cache_key = cache_keys::timeline(username, tab);
   if let Some(cached) = state.cache.get::<Timeline>(&cache_key) {
      return Ok(cached);
   }
   let fetched = fetch(None).await?;
   state.cache.set(&cache_key, &fetched, ttl::DEFAULT);
   Ok(fetched)
}

/// Shared page for the replies and media tabs.
async fn user_tab(cx: &Cx, tab: TimelineKind) -> topcoat::Result<impl View> {
   let state = state(cx);
   let username = path_param::<Username>(cx);
   let cursor = query_params::<TimelineQuery>(cx)?.cursor.as_deref();
   let prefs = request_prefs(cx);
   let user = get_cached_user(state, username).await?;

   let (tab_path, title_prefix) = if tab == TimelineKind::Media {
      ("media", "Media from")
   } else {
      ("with_replies", "Tweets & replies from")
   };
   let fetch_timeline = fetch_tab(state, &user.id, username, cursor, tab);
   let (timeline_result, photo_rail) = if tab == TimelineKind::Media {
      (fetch_timeline.await, Vec::new())
   } else {
      tokio::join!(fetch_timeline, fetch_photo_rail(state, &user.id))
   };
   let (mut tweets, next_cursor) = extract_timeline(timeline_result?);
   enrich_tweet_groups(state, &mut tweets).await;

   let base_url = format!("/{username}/{tab_path}");
   let title = format!("{title_prefix} @{}", user.username);
   let canonical = format!("https://x.com/{username}/{tab_path}");
   let rss_url = format!("/{username}/{tab_path}/rss");
   let has_cursor = cursor.is_some();

   Ok(view! { cx =>
      document(
         title: &title,
         prefs: &prefs,
         rss: &rss_url,
         canonical: &canonical,
         referer: &base_url,
         profile_page(
            user: &user,
            photo_rail: &photo_rail,
            prefs: &prefs,
            tab: tab,
            timeline(
               groups: &tweets,
               prefs: &prefs,
               cursor: next_cursor.as_deref(),
               base_url: &base_url,
               newer_url: has_cursor.then_some(base_url.as_str()),
            )
         )
      )
   })
}

#[page("/{username}/with_replies")]
pub async fn user_replies(cx: &Cx) -> topcoat::Result<impl View> {
   user_tab(cx, TimelineKind::Replies).await
}

#[page("/{username}/media")]
pub async fn user_media(cx: &Cx) -> topcoat::Result<impl View> {
   user_tab(cx, TimelineKind::Media).await
}

#[page("/{username}/search")]
pub async fn user_search(cx: &Cx) -> topcoat::Result<impl View> {
   let state = state(cx);
   let username = path_param::<Username>(cx);
   let query = query_params::<UserSearchQuery>(cx)?;
   let search_query = query.query.as_deref().unwrap_or("");
   let cursor = query.cursor.as_deref();
   let prefs = request_prefs(cx);
   let user = get_cached_user(state, username).await?;

   let api_query = if search_query.is_empty() {
      format!("from:{username} include:nativeretweets")
   } else {
      format!("from:{username} {search_query}")
   };
   let (photo_rail, results) = tokio::join!(
      fetch_photo_rail(state, &user.id),
      state.api.search(&api_query, cursor, SearchProduct::Latest),
   );
   let (mut tweets, next_cursor) = extract_timeline(results?);
   enrich_tweet_groups(state, &mut tweets).await;

   let action = format!("/{username}/search");
   let base_url = if search_query.is_empty() {
      action.clone()
   } else {
      format!(
         "{action}?q={}",
         utf8_percent_encode(search_query, NON_ALPHANUMERIC)
      )
   };
   let title = if search_query.is_empty() {
      format!("Search @{username}")
   } else {
      format!("Search @{username}: {search_query}")
   };
   let canonical = format!("https://x.com/{username}");
   let rss_url = format!("/{username}/search/rss?f=tweets");
   let has_cursor = cursor.is_some();

   Ok(view! {
      document(
         title: &title,
         prefs: &prefs,
         rss: &rss_url,
         canonical: &canonical,
         referer: &action,
         profile_page(
            user: &user,
            photo_rail: &photo_rail,
            prefs: &prefs,
            tab: TimelineKind::Search,
            <div class="timeline-header">
               search_panel(query: search_query, action: &action)
            </div>
            timeline(
               groups: &tweets,
               prefs: &prefs,
               cursor: next_cursor.as_deref(),
               base_url: &base_url,
               newer_url: has_cursor.then_some(base_url.as_str()),
            )
         )
      )
   })
}

/// Fetch photo rail for a user, returning empty vec on error.
async fn fetch_photo_rail(state: &AppState, user_id: &str) -> Vec<GalleryPhoto> {
   state.api.get_photo_rail(user_id).await.unwrap_or_default()
}

/// Handle multi-user timeline (comma-separated usernames).
async fn multi_user_timeline(cx: &Cx, usernames_str: &str) -> topcoat::Result<impl View> {
   const PAGE_SIZE: usize = 20;

   let state = state(cx);
   let cursor = query_params::<TimelineQuery>(cx)?.cursor.as_deref();
   let prefs = request_prefs(cx);

   let Some(usernames) = parse_usernames(usernames_str) else {
      return Err(Error::InvalidUrl("Invalid username format in multi-user search".into()).into());
   };

   if usernames.len() > 10 {
      return Err(Error::InvalidUrl("Maximum 10 users allowed in multi-user search".into()).into());
   }

   let mut all_tweets = Vec::new();
   let handles = usernames
      .iter()
      .map(|username| {
         let state = state.clone();
         let username = username.clone();
         // A spawned task starts with an empty task-local scope, so the caller
         // has to be carried over or these fan-out calls go unbilled.
         tokio::spawn(budget::scoped(async move {
            let user = get_cached_user(&state, &username).await.ok()?;
            let tweets = state.api.get_user_tweets(&user.id, None).await.ok()?;
            Some(tweets.content.into_iter().flatten().collect::<Vec<_>>())
         }))
      })
      .collect::<Vec<_>>();

   for handle in handles {
      if let Ok(Some(tweets)) = handle.await {
         all_tweets.extend(tweets);
      }
   }

   all_tweets.sort_by_key(|tweet| cmp::Reverse(tweet.time));

   if let Some(cursor) = cursor.and_then(|val| val.parse::<i64>().ok()) {
      all_tweets.retain(|tweet| tweet.id < cursor);
   }

   let has_more = all_tweets.len() > PAGE_SIZE;
   all_tweets.truncate(PAGE_SIZE);

   let next_cursor = if has_more {
      all_tweets.last().map(|tweet| tweet.id.to_string())
   } else {
      None
   };

   let title = format!("Tweets from {}", usernames.join(", "));
   let handles = usernames
      .iter()
      .map(|name| format!("@{name}"))
      .collect::<Vec<_>>()
      .join(", ");
   let base_url = format!("/{usernames_str}");
   let has_cursor = cursor.is_some();
   let mut groups = all_tweets
      .into_iter()
      .map(|tweet| vec![tweet])
      .collect::<Vec<Vec<_>>>();
   enrich_tweet_groups(state, &mut groups).await;

   Ok(view! { cx =>
      document(
         title: &title,
         prefs: &prefs,
         <div class="multi-user-timeline">
            <h2>"Combined timeline: " (handles)</h2>
            timeline(
               groups: &groups,
               prefs: &prefs,
               cursor: next_cursor.as_deref(),
               base_url: &base_url,
               newer_url: has_cursor.then_some(base_url.as_str()),
            )
         </div>
      )
   })
}
