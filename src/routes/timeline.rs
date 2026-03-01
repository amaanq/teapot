use std::cmp;

use axum::{
   Router,
   extract::{
      Path,
      Query,
      State,
   },
   http::StatusCode,
   response::{
      Html,
      IntoResponse as _,
      Response,
   },
   routing::get,
};
use axum_extra::extract::CookieJar;
use maud::html;
use serde::Deserialize;

use super::helpers::{
   extract_timeline,
   get_cached_user,
};
use crate::{
   AppState,
   cache::{
      keys as cache_keys,
      ttl,
   },
   error::{
      Error,
      Result,
   },
   types::{
      GalleryPhoto,
      Prefs,
      Profile,
      Timeline,
      TimelineKind,
   },
   utils::formatters,
   views::{
      layout,
      profile,
      search::render_search_panel_with_action,
      timeline,
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

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
   pub tab:    Option<String>,
   pub cursor: Option<String>,
   /// For AJAX infinite scroll requests - returns only tweet HTML.
   pub scroll: Option<String>,
}

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/{username}", get(user_timeline))
      .route("/{username}/", get(user_timeline))
      .route("/{username}/with_replies", get(user_replies))
      .route("/{username}/media", get(user_media))
      .route("/{username}/search", get(user_search))
}

async fn user_timeline(
   State(state): State<AppState>,
   jar: CookieJar,
   Path(username): Path<String>,
   Query(query): Query<TimelineQuery>,
) -> Result<Response> {
   // Validate username
   if is_reserved_path(&username) {
      return Err(Error::InvalidUrl(format!(
         "'{username}' is a reserved path",
      )));
   }

   // Check for multi-user search (comma-separated)
   if username.contains(',') {
      return multi_user_timeline(state, jar, username, query).await;
   }

   if !is_valid_username(&username) {
      return Err(Error::InvalidUrl("Invalid username format".into()));
   }

   // Get the tab type and validate
   let tab = query.tab.as_deref().unwrap_or("tweets");
   if !["tweets", "with_replies", "media", "search", ""].contains(&tab) {
      return Err(Error::InvalidUrl("Invalid tab parameter".into()));
   }

   // Check if this is an AJAX scroll request
   let is_scroll = query.scroll.as_ref().is_some_and(|val| val == "true");

   // Extract prefs from cookies
   let prefs = Prefs::from_cookies(&jar, &state.config);

   // Check cache first (only for initial page load without cursor)
   let cache_key = cache_keys::profile(&username);
   let profile_result = if query.cursor.is_none() {
      if let Some(cached) = state.cache.get::<Profile>(&cache_key) {
         tracing::debug!("Cache hit for profile: {username}");
         Ok(cached)
      } else {
         let result = state.api.get_profile(&username, None).await;
         if let Ok(ref profile_data) = result {
            state.cache.set(&cache_key, profile_data, ttl::DEFAULT);
         }
         result
      }
   } else {
      state
         .api
         .get_profile(&username, query.cursor.as_deref())
         .await
   };

   match profile_result {
      Ok(profile_data) => {
         // For AJAX scroll requests, return only the tweets HTML
         if is_scroll {
            let (tweets, cursor) = extract_timeline(profile_data.tweets);
            let base_url = format!("/{}", profile_data.user.username);
            let content = timeline::render_timeline_with_prefs(
               &tweets,
               &state.config,
               cursor.as_deref(),
               Some(&base_url),
               &prefs,
               None,
            );
            return Ok(Html(content.into_string()).into_response());
         }

         let title = format!(
            "{} (@{})",
            profile_data.user.fullname, profile_data.user.username
         );
         let desc = format!("The latest tweets from {}", profile_data.user.fullname);
         let canonical = format!("https://x.com/{}", profile_data.user.username);
         let rss_url = format!("/{}/rss", profile_data.user.username);
         let referer = format!("/{}", profile_data.user.username);
         let og_image = format!(
            "{}{}",
            state.config.url_prefix(),
            formatters::get_pic_url(
               &profile_data.user.user_pic.replace("_normal", "_400x400"),
               state.config.config.base64_media,
            )
         );
         let newer = query
            .cursor
            .is_some()
            .then(|| format!("/{}", profile_data.user.username));
         let content = profile::render_profile_with_prefs(
            &profile_data,
            &state.config,
            tab,
            Some(&prefs),
            newer.as_deref(),
         );
         let markup = layout::PageLayout::new(&state.config, &title, content)
            .description(&desc)
            .prefs(&prefs)
            .rss(&rss_url)
            .canonical(&canonical)
            .referer(&referer)
            .og_image(&og_image)
            .og_type("object")
            .render();

         Ok(Html(markup.into_string()).into_response())
      },
      Err(Error::UserNotFound(msg)) => {
         let markup = layout::render_error(&state.config, "User not found", &msg);
         Ok((StatusCode::NOT_FOUND, Html(markup.into_string())).into_response())
      },
      Err(err) => {
         let markup = layout::render_error(&state.config, "Error", &err.to_string());
         Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(markup.into_string()),
         )
            .into_response())
      },
   }
}

/// Shared handler for user sub-tab timelines (replies, media).
async fn user_tab_handler(
   state: &AppState,
   prefs: &Prefs,
   username: &str,
   cursor: Option<&str>,
   tab: TimelineKind,
   include_photo_rail: bool,
) -> Result<Response> {
   let user = get_cached_user(state, username).await?;

   let (cache_kind, tab_str, title_prefix) = match tab {
      TimelineKind::Replies => ("replies", "with_replies", "Tweets & replies from"),
      TimelineKind::Media => ("media", "media", "Media from"),
      _ => unreachable!(),
   };

   // Fetch timeline (with cache for first page)
   let fetch_timeline = async {
      let cache_key = cache_keys::timeline(username, cache_kind);
      if cursor.is_none() {
         if let Some(cached) = state.cache.get::<Timeline>(&cache_key) {
            return Ok(cached);
         }
         let result = match tab {
            TimelineKind::Replies => state.api.get_user_tweets_and_replies(&user.id, None).await,
            TimelineKind::Media => state.api.get_user_media(&user.id, None).await,
            _ => unreachable!(),
         };
         if let Ok(ref data) = result {
            state.cache.set(&cache_key, data, ttl::DEFAULT);
         }
         result
      } else {
         match tab {
            TimelineKind::Replies => {
               state
                  .api
                  .get_user_tweets_and_replies(&user.id, cursor)
                  .await
            },
            TimelineKind::Media => state.api.get_user_media(&user.id, cursor).await,
            _ => unreachable!(),
         }
      }
   };

   let (timeline_result, photo_rail) = if include_photo_rail {
      let (tl, rail) = tokio::join!(fetch_timeline, fetch_photo_rail(state, &user.id));
      (tl, rail)
   } else {
      (fetch_timeline.await, vec![])
   };

   let has_request_cursor = cursor.is_some();

   match timeline_result {
      Ok(data) => {
         let (tweets, next_cursor) = extract_timeline(data);
         let base_url = format!("/{username}/{tab_str}");
         let newer = has_request_cursor.then_some(base_url.as_str());

         let timeline_content = timeline::render_timeline_with_prefs(
            &tweets,
            &state.config,
            next_cursor.as_deref(),
            Some(&base_url),
            prefs,
            newer,
         );
         let content = profile::render_profile_page(
            &user,
            &photo_rail,
            &state.config,
            prefs,
            tab,
            &timeline_content,
         );

         let title = format!("{title_prefix} @{}", user.username);
         let canonical = format!("https://x.com/{username}/{tab_str}");
         let rss_url = format!("/{username}/{tab_str}/rss");
         let referer = format!("/{username}/{tab_str}");
         let markup = layout::PageLayout::new(&state.config, &title, content)
            .prefs(prefs)
            .rss(&rss_url)
            .canonical(&canonical)
            .referer(&referer)
            .render();
         Ok(Html(markup.into_string()).into_response())
      },
      Err(err) => {
         let markup = layout::render_error(&state.config, "Error", &err.to_string());
         Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(markup.into_string()),
         )
            .into_response())
      },
   }
}

async fn user_replies(
   State(state): State<AppState>,
   jar: CookieJar,
   Path(username): Path<String>,
   Query(query): Query<TimelineQuery>,
) -> Result<Response> {
   let prefs = Prefs::from_cookies(&jar, &state.config);
   user_tab_handler(
      &state,
      &prefs,
      &username,
      query.cursor.as_deref(),
      TimelineKind::Replies,
      true,
   )
   .await
}

async fn user_media(
   State(state): State<AppState>,
   jar: CookieJar,
   Path(username): Path<String>,
   Query(query): Query<TimelineQuery>,
) -> Result<Response> {
   let prefs = Prefs::from_cookies(&jar, &state.config);
   user_tab_handler(
      &state,
      &prefs,
      &username,
      query.cursor.as_deref(),
      TimelineKind::Media,
      false,
   )
   .await
}

#[derive(Debug, serde::Deserialize)]
pub struct UserSearchQuery {
   #[serde(rename = "q")]
   pub query:  Option<String>,
   pub cursor: Option<String>,
}

async fn user_search(
   State(state): State<AppState>,
   jar: CookieJar,
   Path(username): Path<String>,
   Query(query): Query<UserSearchQuery>,
) -> Result<Response> {
   let prefs = Prefs::from_cookies(&jar, &state.config);
   let user = get_cached_user(&state, &username).await?;

   let search_query = query.query.as_deref().unwrap_or("");
   let search_action = format!("/{username}/search");

   // When search query is empty, default to showing the user's recent tweets
   let api_query = if search_query.is_empty() {
      format!("from:{username} include:nativeretweets")
   } else {
      format!("from:{username} {search_query}")
   };
   let (photo_rail, search_result) = tokio::join!(
      fetch_photo_rail(&state, &user.id),
      state.api.search(&api_query, query.cursor.as_deref()),
   );

   match search_result {
      Ok(results) => {
         let (tweets, cursor) = extract_timeline(results);
         let base_url = if search_query.is_empty() {
            format!("/{username}/search")
         } else {
            format!(
               "/{}/search?q={}",
               username,
               percent_encoding::utf8_percent_encode(
                  search_query,
                  percent_encoding::NON_ALPHANUMERIC,
               )
            )
         };

         let newer = query.cursor.is_some().then_some(base_url.as_str());
         let timeline_content = html! {
             div class="timeline-header" {
                 (render_search_panel_with_action(search_query, None, &search_action))
             }
             (timeline::render_timeline_with_prefs(&tweets, &state.config, cursor.as_deref(), Some(&base_url), &prefs, newer))
         };
         let content = profile::render_profile_page(
            &user,
            &photo_rail,
            &state.config,
            &prefs,
            TimelineKind::Search,
            &timeline_content,
         );

         let title = if search_query.is_empty() {
            format!("Search @{username}")
         } else {
            format!("Search @{username}: {search_query}")
         };
         let canonical = format!("https://x.com/{username}");
         let rss_url = format!("/{username}/search/rss?f=tweets");
         let referer = format!("/{username}/search");
         let markup = layout::PageLayout::new(&state.config, &title, content)
            .prefs(&prefs)
            .rss(&rss_url)
            .canonical(&canonical)
            .referer(&referer)
            .render();
         Ok(Html(markup.into_string()).into_response())
      },
      Err(err) => {
         let markup = layout::render_error(&state.config, "Search Error", &err.to_string());
         Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(markup.into_string()),
         )
            .into_response())
      },
   }
}

/// Fetch photo rail for a user, returning empty vec on error.
async fn fetch_photo_rail(state: &AppState, user_id: &str) -> Vec<GalleryPhoto> {
   state.api.get_photo_rail(user_id).await.unwrap_or_default()
}

/// Handle multi-user timeline (comma-separated usernames).
async fn multi_user_timeline(
   state: AppState,
   jar: CookieJar,
   usernames_str: String,
   query: TimelineQuery,
) -> Result<Response> {
   const PAGE_SIZE: usize = 20;

   let prefs = Prefs::from_cookies(&jar, &state.config);

   let Some(usernames) = parse_usernames(&usernames_str) else {
      return Err(Error::InvalidUrl(
         "Invalid username format in multi-user search".into(),
      ));
   };

   if usernames.len() > 10 {
      return Err(Error::InvalidUrl(
         "Maximum 10 users allowed in multi-user search".into(),
      ));
   }

   let cursor_id = query
      .cursor
      .as_ref()
      .and_then(|val| val.parse::<i64>().ok());

   let mut all_tweets = Vec::new();
   let handles: Vec<_> = usernames
      .iter()
      .map(|username| {
         let state = state.clone();
         let username = username.clone();
         tokio::spawn(async move {
            let user = get_cached_user(&state, &username).await.ok()?;
            let timeline = state.api.get_user_tweets(&user.id, None).await.ok()?;
            Some(timeline.content.into_iter().flatten().collect::<Vec<_>>())
         })
      })
      .collect();

   for handle in handles {
      if let Ok(Some(tweets)) = handle.await {
         all_tweets.extend(tweets);
      }
   }

   all_tweets.sort_by_key(|tweet| cmp::Reverse(tweet.time));

   if let Some(cursor) = cursor_id {
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
   let base_url = format!("/{usernames_str}");

   let newer = query.cursor.is_some().then_some(base_url.as_str());
   let groups = all_tweets
      .into_iter()
      .map(|tweet| vec![tweet])
      .collect::<Vec<Vec<_>>>();
   let content = html! {
       div class="multi-user-timeline" {
           h2 { "Combined timeline: " (usernames.iter().map(|name| format!("@{name}")).collect::<Vec<_>>().join(", ")) }
           (timeline::render_timeline_with_prefs(&groups, &state.config, next_cursor.as_deref(), Some(&base_url), &prefs, newer))
       }
   };

   let markup = layout::PageLayout::new(&state.config, &title, content)
      .prefs(&prefs)
      .render();
   Ok(Html(markup.into_string()).into_response())
}
