use axum::http::header;
use serde::{
   Deserialize,
   de::DeserializeOwned,
};

use super::{
   SessionPool,
   TidClient,
   endpoints,
   http::HttpClient,
   parser,
};
use crate::{
   api::schema::{
      AudioSpaceData,
      AudioSpaceMetadata,
      BroadcastMetadata,
      BroadcastsData,
      ConversationData,
      EditHistoryData,
      GqlResponse,
      ListByIdData,
      ListBySlugData,
      ListMembersData,
      ListTimelineData,
      RetweetersData,
      SearchTimelineData,
      TweetData,
      UserResultData,
      UserTimelineData,
   },
   config::Config,
   error::{
      Error,
      Result,
      TwitterError,
   },
   types::{
      Article,
      CardKind,
      Conversation,
      EditHistory,
      GalleryPhoto,
      List,
      PaginatedResult,
      Profile,
      SessionKind,
      Timeline,
      Translation,
      Tweet,
      User,
   },
   utils::formatters,
};

fn space_id_from_url(url: &str) -> Option<&str> {
   url.split("/spaces/")
      .nth(1)?
      .split(['/', '?', '#'])
      .next()
      .filter(|id| !id.is_empty())
}

fn millis_to_time(milliseconds: i64) -> Option<time::OffsetDateTime> {
   time::OffsetDateTime::from_unix_timestamp(milliseconds.checked_div(1_000)?).ok()
}

fn audio_space_status(metadata: &AudioSpaceMetadata) -> String {
   match metadata.state.as_deref() {
      Some("NotStarted") => {
         metadata
            .scheduled_start
            .and_then(millis_to_time)
            .map_or_else(
               || "Scheduled".to_owned(),
               |time| format!("Scheduled · {}", formatters::format_tweet_time(time)),
            )
      },
      Some("Running") => {
         metadata
            .total_live_listeners
            .filter(|count| *count > 0)
            .map_or_else(
               || "Live now".to_owned(),
               |count| {
                  format!(
                     "Live now · {} listening",
                     formatters::abbreviate_number(count)
                  )
               },
            )
      },
      Some("Ended" | "TimedOut") if metadata.is_space_available_for_replay.unwrap_or(false) => {
         metadata
            .total_replay_watched
            .filter(|count| *count > 0)
            .map_or_else(
               || "Replay available".to_owned(),
               |count| {
                  format!(
                     "Replay available · {} plays",
                     formatters::abbreviate_number(count)
                  )
               },
            )
      },
      Some("Ended" | "TimedOut") => "Space ended".to_owned(),
      _ => String::new(),
   }
}

fn audio_space_host(metadata: &AudioSpaceMetadata) -> String {
   let Some(user) = metadata
      .creator_results
      .as_ref()
      .and_then(|results| results.result.as_deref())
   else {
      return "X Space".to_owned();
   };
   let name = user
      .core
      .as_ref()
      .and_then(|core| core.name.as_deref())
      .or_else(|| {
         user
            .legacy
            .as_ref()
            .and_then(|legacy| legacy.name.as_deref())
      })
      .unwrap_or_default();
   let username = user
      .core
      .as_ref()
      .and_then(|core| core.screen_name.as_deref())
      .or_else(|| {
         user
            .legacy
            .as_ref()
            .and_then(|legacy| legacy.screen_name.as_deref())
      })
      .unwrap_or_default();

   hosted_by(name, username).unwrap_or_else(|| "X Space".to_owned())
}

fn hosted_by(name: &str, username: &str) -> Option<String> {
   match (name.is_empty(), username.is_empty()) {
      (false, false) => Some(format!("Hosted by {name} (@{username})")),
      (false, true) => Some(format!("Hosted by {name}")),
      (true, false) => Some(format!("Hosted by @{username}")),
      (true, true) => None,
   }
}

fn broadcast_id_from_url(url: &str) -> Option<&str> {
   url.split("/broadcasts/")
      .nth(1)?
      .split(['/', '?', '#'])
      .next()
      .filter(|id| !id.is_empty())
}

fn broadcast_status(metadata: &BroadcastMetadata) -> String {
   match metadata.state.as_str() {
      "RUNNING" => {
         metadata
            .total_watching
            .filter(|count| *count > 0)
            .map_or_else(
               || "Live now".to_owned(),
               |count| {
                  format!(
                     "Live now · {} watching",
                     formatters::abbreviate_number(count)
                  )
               },
            )
      },
      "ENDED" if metadata.available_for_replay => {
         metadata
            .total_watched
            .filter(|count| *count > 0)
            .map_or_else(
               || "Replay available".to_owned(),
               |count| {
                  format!(
                     "Replay available · {} views",
                     formatters::abbreviate_number(count)
                  )
               },
            )
      },
      "ENDED" => {
         metadata
            .total_watched
            .filter(|count| *count > 0)
            .map_or_else(
               || "Broadcast ended".to_owned(),
               |count| {
                  format!(
                     "Broadcast ended · {} views",
                     formatters::abbreviate_number(count)
                  )
               },
            )
      },
      _ => String::new(),
   }
}

fn article_tweet_data<'a>(data: &'a ConversationData, tweet_id: &str) -> Option<&'a TweetData> {
   let raw = data
      .tweet_result
      .as_ref()
      .and_then(|nested| nested.result.as_deref())
      .or_else(|| {
         data
            .threaded_conversation_with_injections_v2
            .as_ref()?
            .instructions
            .iter()
            .filter_map(|instruction| instruction.entries.as_deref())
            .flatten()
            .find(|entry| {
               entry
                  .entry_id_str()
                  .starts_with(&format!("tweet-{tweet_id}"))
            })
            .and_then(|entry| entry.tweet_result())
      })?;

   raw.tweet.as_deref().or(Some(raw))
}

/// Twitter/X API client.
#[derive(Clone)]
pub struct ApiClient {
   client:   HttpClient,
   sessions: SessionPool,
   tid:      TidClient,
}

impl ApiClient {
   pub fn new(config: &Config, sessions: SessionPool) -> Self {
      let mut headers = header::HeaderMap::new();
      headers.insert(
         header::USER_AGENT,
         header::HeaderValue::from_static(endpoints::USER_AGENT),
      );
      headers.insert(
         header::ACCEPT_LANGUAGE,
         header::HeaderValue::from_static("en-US,en;q=0.9"),
      );
      headers.insert(
         header::ACCEPT_ENCODING,
         header::HeaderValue::from_static("gzip"),
      );
      headers.insert(
         header::CONNECTION,
         header::HeaderValue::from_static("keep-alive"),
      );

      let client = HttpClient::new(&config.config.proxy, &config.config.proxy_auth)
         .with_default_headers(headers);

      let tid = TidClient::new(client.clone());

      Self {
         client,
         sessions,
         tid,
      }
   }

   /// Check for API-level errors in the raw response bytes.
   fn check_api_errors(bytes: &[u8]) -> Result<()> {
      #[derive(Deserialize)]
      struct ErrorCheck {
         errors: Option<Vec<ApiError>>,
      }
      #[derive(Deserialize)]
      struct ApiError {
         #[serde(default)]
         code:    i64,
         #[serde(default)]
         message: String,
      }

      let Ok(check) = serde_json::from_slice::<ErrorCheck>(bytes) else {
         return Ok(());
      };
      let Some(error) = check.errors.as_ref().and_then(|errs| errs.first()) else {
         return Ok(());
      };

      if let Some(twitter_err) = TwitterError::from_code(error.code) {
         return match twitter_err {
            TwitterError::UserNotFound | TwitterError::NoUserMatches => {
               Err(Error::UserNotFound(error.message.clone()))
            },
            TwitterError::ProtectedUser => Err(Error::ProtectedUser(error.message.clone())),
            TwitterError::UserSuspended | TwitterError::Locked => {
               Err(Error::UserSuspended(error.message.clone()))
            },
            TwitterError::RateLimited => Err(Error::RateLimited),
            TwitterError::TweetNotFound
            | TwitterError::TweetUnavailable
            | TwitterError::NoStatusFound
            | TwitterError::TweetUnavailable421
            | TwitterError::TweetCensored => Err(Error::TweetNotFound(error.message.clone())),
            TwitterError::InvalidToken | TwitterError::BadToken => {
               Err(Error::TwitterApi(format!(
                  "Invalid token: {}",
                  error.message
               )))
            },
         };
      }

      Err(Error::TwitterApi(format!(
         "Error {}: {}",
         error.code, error.message
      )))
   }

   /// Make a GraphQL request to the Twitter API.
   ///
   /// On token-related failures the session is marked as limited and the
   /// request is retried once with a different session.
   async fn graphql_request<T: DeserializeOwned>(
      &self,
      endpoint: &str,
      variables: &str,
      features: &str,
      field_toggles: Option<&str>,
   ) -> Result<T> {
      match self
         .graphql_request_inner(endpoint, variables, features, field_toggles)
         .await
      {
         Err(Error::TwitterApi(ref msg)) if msg.starts_with("Invalid token") => {
            tracing::warn!("Token rejected, retrying with another session: {msg}");
            self
               .graphql_request_inner(endpoint, variables, features, field_toggles)
               .await
         },
         other => other,
      }
   }

   /// Inner implementation of [`graphql_request`].
   async fn graphql_request_inner<T: DeserializeOwned>(
      &self,
      endpoint: &str,
      variables: &str,
      features: &str,
      field_toggles: Option<&str>,
   ) -> Result<T> {
      let session = self.sessions.get_session(endpoint).await?;

      let base_url = match session.kind {
         SessionKind::OAuth => endpoints::API_URL,
         SessionKind::Cookie => endpoints::GRAPHQL_URL,
      };

      // Build URL with query string (scoped to drop Serializer before await)
      let url = {
         let mut qs = form_urlencoded::Serializer::new(String::new());
         qs.append_pair("variables", variables);
         qs.append_pair("features", features);
         if let Some(toggles) = field_toggles {
            qs.append_pair("fieldToggles", toggles);
         }
         format!("{base_url}/{endpoint}?{}", qs.finish())
      };

      // Build auth + extra headers
      let mut headers = header::HeaderMap::new();

      match session.kind {
         SessionKind::OAuth => {
            let auth_url = format!("{base_url}/{endpoint}");
            let auth = super::oauth1_sign(
               "GET",
               &auth_url,
               &[],
               &session.oauth_token,
               &session.oauth_secret,
            );
            headers.insert(
               header::AUTHORIZATION,
               auth
                  .parse()
                  .map_err(|_| Error::Internal("invalid OAuth header value".into()))?,
            );
         },
         SessionKind::Cookie => {
            let api_path = format!("/i/api/graphql/{endpoint}");
            let (bearer, tid) = self
               .tid
               .generate(&api_path)
               .await
               .map_or((endpoints::BEARER_TOKEN_NO_TID, None), |tid_val| {
                  (endpoints::BEARER_TOKEN, Some(tid_val))
               });

            headers.insert(
               header::AUTHORIZATION,
               header::HeaderValue::from_str(bearer)
                  .map_err(|_| Error::Internal("invalid bearer token value".into()))?,
            );
            headers.insert(
               "x-twitter-auth-type",
               header::HeaderValue::from_static("OAuth2Session"),
            );
            headers.insert(
               "x-csrf-token",
               session
                  .ct0
                  .parse()
                  .map_err(|_| Error::Internal("invalid ct0 header value".into()))?,
            );
            headers.insert(
               header::COOKIE,
               format!("auth_token={}; ct0={}", session.auth_token, session.ct0)
                  .parse()
                  .map_err(|_| Error::Internal("invalid cookie header value".into()))?,
            );
            headers.insert(
               header::ORIGIN,
               header::HeaderValue::from_static("https://x.com"),
            );
            headers.insert(
               header::CONTENT_TYPE,
               header::HeaderValue::from_static("application/json"),
            );
            headers.insert(
               "sec-ch-ua",
               header::HeaderValue::from_static(
                  r#""Google Chrome";v="142", "Chromium";v="142", "Not A(Brand";v="24""#,
               ),
            );
            headers.insert("sec-ch-ua-mobile", header::HeaderValue::from_static("?0"));
            headers.insert(
               "sec-ch-ua-platform",
               header::HeaderValue::from_static("\"Windows\""),
            );
            headers.insert("sec-fetch-dest", header::HeaderValue::from_static("empty"));
            headers.insert("sec-fetch-mode", header::HeaderValue::from_static("cors"));
            headers.insert(
               "sec-fetch-site",
               header::HeaderValue::from_static("same-site"),
            );

            if let Some(tid) = tid
               && let Ok(val) = tid.parse()
            {
               headers.insert("x-client-transaction-id", val);
            }
         },
      }

      // Common headers
      headers.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));
      headers.insert(
         "x-twitter-active-user",
         header::HeaderValue::from_static("yes"),
      );
      headers.insert(
         "x-twitter-client-language",
         header::HeaderValue::from_static("en"),
      );

      let response = self.client.get_with_headers(&url, &headers).await?;

      // Check rate limit headers
      if let Some(remaining) = response.headers().get("x-rate-limit-remaining")
         && let Ok(remaining_str) = remaining.to_str()
         && let Ok(remaining_val) = remaining_str.parse::<i32>()
      {
         let limit = response
            .headers()
            .get("x-rate-limit-limit")
            .and_then(|hv| hv.to_str().ok())
            .and_then(|sv| sv.parse().ok())
            .unwrap_or(0);
         let reset = response
            .headers()
            .get("x-rate-limit-reset")
            .and_then(|hv| hv.to_str().ok())
            .and_then(|sv| sv.parse().ok())
            .unwrap_or(0);

         self
            .sessions
            .update_session_limit(session.id, endpoint, limit, remaining_val, reset)
            .await;
      }

      if !response.status().is_success() {
         let status = response.status();
         let body = response.text().await.unwrap_or_default();
         tracing::error!(
            session_id = session.id,
            session_user = %session.username,
            "API request failed: {status} - {body}"
         );

         if status.as_u16() == 429 {
            self.sessions.mark_limited(session.id).await;
            return Err(Error::RateLimited);
         }

         return Err(Error::TwitterApi(format!("Status {status}: {body}")));
      }

      let bytes = response.bytes().await?;

      // Check for API errors before full deserialization.
      // Mark the session as limited on token errors so the retry picks
      // a different one.
      let api_check = Self::check_api_errors(&bytes);
      if let Err(Error::TwitterApi(ref msg)) = api_check
         && msg.starts_with("Invalid token")
      {
         self.sessions.mark_limited(session.id).await;
      }
      api_check?;

      let resp = serde_json::from_slice::<GqlResponse<T>>(&bytes)
         .map_err(|err| Error::Internal(format!("Response parse error: {err}")))?;
      Ok(resp.data)
   }

   async fn cookie_json_request<T: DeserializeOwned>(
      &self,
      session_key: &str,
      api_path: &str,
      url: &str,
   ) -> Result<T> {
      let session = self.sessions.get_session(session_key).await?;
      if !matches!(session.kind, SessionKind::Cookie) {
         return Err(Error::Internal(
            "This X endpoint requires a cookie session".into(),
         ));
      }

      let (bearer, tid) = self
         .tid
         .generate(api_path)
         .await
         .map_or((endpoints::BEARER_TOKEN_NO_TID, None), |tid_val| {
            (endpoints::BEARER_TOKEN, Some(tid_val))
         });
      let mut headers = header::HeaderMap::new();
      headers.insert(
         header::AUTHORIZATION,
         header::HeaderValue::from_str(bearer)
            .map_err(|_| Error::Internal("invalid bearer token value".into()))?,
      );
      headers.insert(
         "x-twitter-auth-type",
         header::HeaderValue::from_static("OAuth2Session"),
      );
      headers.insert(
         "x-csrf-token",
         session
            .ct0
            .parse()
            .map_err(|_| Error::Internal("invalid ct0 header value".into()))?,
      );
      headers.insert(
         header::COOKIE,
         format!("auth_token={}; ct0={}", session.auth_token, session.ct0)
            .parse()
            .map_err(|_| Error::Internal("invalid cookie header value".into()))?,
      );
      headers.insert(
         header::ORIGIN,
         header::HeaderValue::from_static("https://x.com"),
      );
      headers.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));
      headers.insert(
         "x-twitter-active-user",
         header::HeaderValue::from_static("yes"),
      );
      headers.insert(
         "x-twitter-client-language",
         header::HeaderValue::from_static("en"),
      );
      if let Some(tid) = tid
         && let Ok(value) = tid.parse()
      {
         headers.insert("x-client-transaction-id", value);
      }

      let response = self.client.get_with_headers(url, &headers).await?;
      if !response.status().is_success() {
         let status = response.status();
         let body = response.text().await.unwrap_or_default();
         return Err(Error::TwitterApi(format!(
            "X API request failed ({status}): {body}"
         )));
      }

      let bytes = response.bytes().await?;
      serde_json::from_slice(&bytes)
         .map_err(|err| Error::Internal(format!("Response parse error: {err}")))
   }

   /// Get user by screen name.
   pub async fn get_user(&self, screen_name: &str) -> Result<User> {
      let data = self
         .graphql_request::<UserResultData>(
            endpoints::GRAPH_USER,
            &endpoints::user_by_screen_name_vars(screen_name),
            endpoints::GQL_FEATURES,
            Some(endpoints::USER_FIELD_TOGGLES),
         )
         .await?;
      super::parse_user(&data)
   }

   /// Get user by REST ID (numeric user ID).
   pub async fn get_user_by_id(&self, user_id: &str) -> Result<User> {
      if user_id.is_empty() || !user_id.chars().all(|ch| ch.is_ascii_digit()) {
         return Err(Error::UserNotFound("Invalid user ID format".to_owned()));
      }
      let data = self
         .graphql_request::<UserResultData>(
            endpoints::GRAPH_USER_BY_ID,
            &endpoints::user_by_id_vars(user_id),
            endpoints::GQL_FEATURES,
            Some(endpoints::USER_FIELD_TOGGLES),
         )
         .await?;
      super::parse_user(&data)
   }

   /// Get edit history for a tweet.
   pub async fn get_edit_history(&self, tweet_id: &str) -> Result<EditHistory> {
      let data = self
         .graphql_request::<EditHistoryData>(
            endpoints::GRAPH_TWEET_EDIT_HISTORY,
            &endpoints::tweet_edit_history_vars(tweet_id),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      parser::edit_history::parse_edit_history(&data)
   }

   /// Get tweet by ID.
   ///
   /// Uses the `TweetDetail` endpoint (same as conversation) because
   /// `TweetResultByIdQuery` returns 404 for many tweets.
   pub async fn get_tweet(&self, tweet_id: &str) -> Result<Tweet> {
      let convo = self.get_conversation(tweet_id, None, "Relevance").await?;
      Ok(convo.tweet)
   }

   /// Re-fetch unavailable quote tweets (e.g. blocked-quoter tombstones).
   /// The tweet still exists — it's just hidden from the quoter's context.
   pub async fn resolve_unavailable_quote(&self, tweet: &mut Tweet) {
      let should_resolve = tweet
         .quote
         .as_ref()
         .is_some_and(|qt| !qt.available && qt.id != 0);
      if !should_resolve {
         return;
      }
      let quote_id = tweet.quote.as_ref().unwrap().id.to_string();
      if let Ok(resolved) = self.get_tweet(&quote_id).await {
         tweet.quote = Some(Box::new(resolved));
      }
   }

   async fn enrich_audio_space_card(&self, tweet: &mut Tweet) {
      let Some(space_id) = tweet
         .card
         .as_ref()
         .filter(|card| card.kind == CardKind::Audiospace)
         .and_then(|card| space_id_from_url(&card.url))
         .map(str::to_owned)
      else {
         return;
      };

      let Ok(data) = self
         .graphql_request::<AudioSpaceData>(
            endpoints::GRAPH_AUDIO_SPACE,
            &endpoints::audio_space_vars(&space_id),
            endpoints::GQL_FEATURES,
            None,
         )
         .await
      else {
         return;
      };
      let Some(metadata) = data.audio_space.and_then(|space| space.metadata) else {
         return;
      };
      let Some(card) = tweet
         .card
         .as_mut()
         .filter(|card| card.kind == CardKind::Audiospace)
      else {
         return;
      };

      if let Some(title) = metadata.title.as_deref().filter(|title| !title.is_empty()) {
         title.clone_into(&mut card.title);
      }
      card.text = audio_space_status(&metadata);
      card.dest = audio_space_host(&metadata);
   }

   async fn enrich_broadcast_card(&self, tweet: &mut Tweet) {
      let Some(broadcast_id) = tweet
         .card
         .as_ref()
         .filter(|card| matches!(card.kind, CardKind::Broadcast | CardKind::Periscope))
         .and_then(|card| broadcast_id_from_url(&card.url))
         .map(str::to_owned)
      else {
         return;
      };
      let url = endpoints::broadcast_show_url(&broadcast_id);
      let Ok(data) = self
         .cookie_json_request::<BroadcastsData>(
            endpoints::BROADCAST_SHOW_PATH,
            endpoints::BROADCAST_SHOW_PATH,
            &url,
         )
         .await
      else {
         return;
      };
      let Some(metadata) = data.broadcasts.get(&broadcast_id) else {
         return;
      };
      let Some(card) = tweet
         .card
         .as_mut()
         .filter(|card| matches!(card.kind, CardKind::Broadcast | CardKind::Periscope))
      else {
         return;
      };

      if !metadata.status.is_empty() {
         metadata.status.clone_into(&mut card.title);
      } else if !card.text.is_empty() {
         card.text.clone_into(&mut card.title);
      }
      card.text = broadcast_status(metadata);
      card.dest = hosted_by(&metadata.user_display_name, &metadata.twitter_username)
         .unwrap_or_else(|| "Live broadcast".to_owned());
      if !metadata.image_url.is_empty() {
         metadata.image_url.clone_into(&mut card.image);
         if let Some(video) = card.video.as_mut() {
            metadata.image_url.clone_into(&mut video.thumb);
         }
      }
   }

   /// Get conversation/thread for a tweet.
   pub async fn get_conversation(
      &self,
      tweet_id: &str,
      cursor: Option<&str>,
      ranking_mode: &str,
   ) -> Result<Conversation> {
      let data = self
         .graphql_request::<ConversationData>(
            endpoints::GRAPH_TWEET_DETAIL,
            &endpoints::tweet_detail_vars(tweet_id, cursor, ranking_mode),
            endpoints::GQL_FEATURES,
            Some(endpoints::TWEET_DETAIL_FIELD_TOGGLES),
         )
         .await?;
      let mut conversation = super::parse_conversation(&data, tweet_id, cursor.is_some())?;
      if cursor.is_none()
         && let Some(tweet_data) = article_tweet_data(&data, tweet_id)
         && let Ok(article) = parser::parse_article(tweet_data)
      {
         parser::attach_article_preview(&mut conversation.tweet, &article);
      }
      if cursor.is_none()
         && conversation
            .tweet
            .entities
            .iter()
            .any(|entity| entity.url.contains("/article/"))
         && let Ok((_tweet, article)) = self.get_article_tweet(tweet_id).await
      {
         parser::attach_article_preview(&mut conversation.tweet, &article);
      }
      if cursor.is_none() {
         self.enrich_audio_space_card(&mut conversation.tweet).await;
         self.enrich_broadcast_card(&mut conversation.tweet).await;
      }
      Ok(conversation)
   }

   /// Get user's tweets timeline.
   pub async fn get_user_tweets(&self, user_id: &str, cursor: Option<&str>) -> Result<Timeline> {
      let data = self
         .graphql_request::<UserTimelineData>(
            endpoints::GRAPH_USER_TWEETS,
            &endpoints::user_tweets_vars(user_id, cursor),
            endpoints::GQL_FEATURES,
            Some(endpoints::USER_TWEETS_FIELD_TOGGLES),
         )
         .await?;
      super::parse_timeline(&data)
   }

   /// Get user's media timeline.
   pub async fn get_user_media(&self, user_id: &str, cursor: Option<&str>) -> Result<Timeline> {
      // Use different endpoint/variables based on session type
      let session_kind = self
         .sessions
         .get_session_kind(endpoints::GRAPH_USER_MEDIA)
         .await;
      let (endpoint, variables) = match session_kind {
         SessionKind::OAuth => {
            (
               endpoints::GRAPH_USER_MEDIA_V2,
               endpoints::user_media_v2_vars(user_id, cursor),
            )
         },
         SessionKind::Cookie => {
            (
               endpoints::GRAPH_USER_MEDIA,
               endpoints::user_media_vars(user_id, cursor),
            )
         },
      };

      let data = self
         .graphql_request::<UserTimelineData>(endpoint, &variables, endpoints::GQL_FEATURES, None)
         .await?;

      super::parse_timeline(&data)
   }

   /// Get user's profile with tweets.
   pub async fn get_profile(&self, screen_name: &str, cursor: Option<&str>) -> Result<Profile> {
      // First get user info
      let user = self.get_user(screen_name).await?;

      // Protected/suspended accounts don't expose tweets
      if user.protected || user.suspended {
         return Ok(Profile {
            user,
            ..Profile::default()
         });
      }

      // Fetch tweets and photo rail in parallel (only for first page)
      let (tweets, photo_rail) = if cursor.is_none() {
         let tweets_future = self.get_user_tweets(&user.id, None);
         let photo_rail_future = self.get_photo_rail(&user.id);
         let (tweets_result, photo_rail_result) = tokio::join!(tweets_future, photo_rail_future);
         (tweets_result?, photo_rail_result.unwrap_or_default())
      } else {
         (self.get_user_tweets(&user.id, cursor).await?, Vec::new())
      };

      // Get pinned tweet if present
      let pinned = if user.pinned_tweet > 0 {
         tweets
            .content
            .iter()
            .flatten()
            .find(|tweet| tweet.id == user.pinned_tweet)
            .cloned()
      } else {
         None
      };

      Ok(Profile {
         user,
         photo_rail,
         pinned,
         tweets,
      })
   }

   /// Search tweets.
   pub async fn search(
      &self,
      query: &str,
      cursor: Option<&str>,
      product: &str,
   ) -> Result<Timeline> {
      let data = self
         .graphql_request::<SearchTimelineData>(
            endpoints::GRAPH_SEARCH_TIMELINE,
            &endpoints::search_vars(query, cursor, product),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      let mut timeline = super::parse_search_timeline(&data);

      // When no more items are available the API returns the last page again.
      // Detect this by comparing the first 64 chars of the input and output cursors.
      if let Some(after) = cursor
         && let Some(ref bottom) = timeline.bottom
         && let Some(after_prefix) = after.get(..64)
         && let Some(bottom_prefix) = bottom.get(..64)
         && after_prefix == bottom_prefix
      {
         timeline.content.clear();
         timeline.bottom = None;
      }

      Ok(timeline)
   }

   /// Search users.
   pub async fn search_users(
      &self,
      query: &str,
      cursor: Option<&str>,
   ) -> Result<PaginatedResult<User>> {
      let data = self
         .graphql_request::<SearchTimelineData>(
            endpoints::GRAPH_SEARCH_TIMELINE,
            &endpoints::search_vars(query, cursor, "People"),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      Ok(super::parse_user_search(&data))
   }

   /// Get list by ID.
   pub async fn get_list(&self, list_id: &str) -> Result<List> {
      let data = self
         .graphql_request::<ListByIdData>(
            endpoints::GRAPH_LIST_BY_ID,
            &endpoints::list_by_id_vars(list_id),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      let wrapper = data
         .list
         .as_ref()
         .ok_or_else(|| Error::NotFound("List not found".into()))?;
      Ok(super::parse_list(wrapper.list_data()))
   }

   /// Get list by owner username and slug.
   pub async fn get_list_by_slug(&self, screen_name: &str, slug: &str) -> Result<List> {
      let data = self
         .graphql_request::<ListBySlugData>(
            endpoints::GRAPH_LIST_BY_SLUG,
            &endpoints::list_by_slug_vars(screen_name, slug),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      data
         .user_by_screen_name
         .as_ref()
         .and_then(|nested| nested.list.as_ref())
         .map(|ld| Ok(super::parse_list(ld)))
         .ok_or_else(|| Error::NotFound("List not found".into()))?
   }

   /// Get list tweets.
   pub async fn get_list_tweets(&self, list_id: &str, cursor: Option<&str>) -> Result<Timeline> {
      let data = self
         .graphql_request::<ListTimelineData>(
            endpoints::GRAPH_LIST_TWEETS,
            &endpoints::list_timeline_vars(list_id, cursor),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      super::parse_list_timeline(&data)
   }

   /// Get list members.
   pub async fn get_list_members(
      &self,
      list_id: &str,
      cursor: Option<&str>,
   ) -> Result<PaginatedResult<User>> {
      let data = self
         .graphql_request::<ListMembersData>(
            endpoints::GRAPH_LIST_MEMBERS,
            &endpoints::list_members_vars(list_id, cursor),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      Ok(super::parse_list_members(&data))
   }

   /// Get users who retweeted a tweet.
   pub async fn get_retweeters(
      &self,
      tweet_id: &str,
      cursor: Option<&str>,
   ) -> Result<PaginatedResult<User>> {
      let data = self
         .graphql_request::<RetweetersData>(
            endpoints::GRAPH_RETWEETERS,
            &endpoints::retweeters_vars(tweet_id, cursor),
            endpoints::GQL_FEATURES,
            None,
         )
         .await?;
      Ok(super::parse_retweeters(&data))
   }

   /// Get user's tweets and replies timeline.
   pub async fn get_user_tweets_and_replies(
      &self,
      user_id: &str,
      cursor: Option<&str>,
   ) -> Result<Timeline> {
      let session_kind = self
         .sessions
         .get_session_kind(endpoints::GRAPH_USER_TWEETS_AND_REPLIES)
         .await;
      let (endpoint, variables, field_toggles) = match session_kind {
         SessionKind::OAuth => {
            (
               endpoints::GRAPH_USER_TWEETS_AND_REPLIES_V2,
               endpoints::user_tweets_and_replies_v2_vars(user_id, cursor),
               None,
            )
         },
         SessionKind::Cookie => {
            (
               endpoints::GRAPH_USER_TWEETS_AND_REPLIES,
               endpoints::user_tweets_and_replies_vars(user_id, cursor),
               Some(endpoints::USER_TWEETS_FIELD_TOGGLES),
            )
         },
      };
      let data = self
         .graphql_request::<UserTimelineData>(
            endpoint,
            &variables,
            endpoints::GQL_FEATURES,
            field_toggles,
         )
         .await?;
      super::parse_timeline(&data)
   }

   /// Get a tweet with its inline article data.
   ///
   /// Uses `TweetDetail` (not `TweetResultByIdQuery`) because only the detail
   /// endpoint supports `withArticleRichContentState`.
   pub async fn get_article_tweet(&self, tweet_id: &str) -> Result<(Tweet, Article)> {
      let data = self
         .graphql_request::<ConversationData>(
            endpoints::GRAPH_TWEET_DETAIL,
            &endpoints::tweet_detail_vars(tweet_id, None, "Relevance"),
            endpoints::GQL_FEATURES,
            Some(endpoints::TWEET_DETAIL_FIELD_TOGGLES),
         )
         .await?;

      // Parse the conversation to get the Tweet (reuses proven logic).
      let conversation = super::parse_conversation(&data, tweet_id, false)?;
      let tweet = conversation.tweet;

      let tweet_data = article_tweet_data(&data, tweet_id)
         .ok_or_else(|| Error::TweetNotFound("Tweet data not found in response".into()))?;

      let article = parser::parse_article(tweet_data)?;
      let mut tweet = tweet;
      parser::attach_article_preview(&mut tweet, &article);
      Ok((tweet, article))
   }

   /// Translate a tweet using the Strato translation API.
   pub async fn translate_tweet(&self, tweet_id: &str) -> Result<Translation> {
      let url = endpoints::translate_url(tweet_id);
      let session = self
         .sessions
         .get_session(endpoints::GRAPH_TWEET_DETAIL)
         .await?;

      // Strato endpoint only works with cookie sessions
      if !matches!(session.kind, SessionKind::Cookie) {
         return Err(Error::Internal(
            "Translation requires cookie session".into(),
         ));
      }

      let api_path = format!(
         "/i/api/1.1/strato/column/None/tweetId={tweet_id},destinationLanguage=None,\
          translationSource=Some(Google),feature=None,timeout=None,onlyCached=None/translation/\
          service/translateTweet"
      );
      let (bearer, tid) = self
         .tid
         .generate(&api_path)
         .await
         .map_or((endpoints::BEARER_TOKEN_NO_TID, None), |tid_val| {
            (endpoints::BEARER_TOKEN, Some(tid_val))
         });

      let mut headers = header::HeaderMap::new();
      headers.insert(
         header::AUTHORIZATION,
         header::HeaderValue::from_str(bearer)
            .map_err(|_| Error::Internal("invalid bearer token value".into()))?,
      );
      headers.insert(
         "x-twitter-auth-type",
         header::HeaderValue::from_static("OAuth2Session"),
      );
      headers.insert(
         "x-csrf-token",
         session
            .ct0
            .parse()
            .map_err(|_| Error::Internal("invalid ct0 header value".into()))?,
      );
      headers.insert(
         header::COOKIE,
         format!("auth_token={}; ct0={}", session.auth_token, session.ct0)
            .parse()
            .map_err(|_| Error::Internal("invalid cookie header value".into()))?,
      );
      headers.insert(
         header::ORIGIN,
         header::HeaderValue::from_static("https://x.com"),
      );
      headers.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));
      headers.insert(
         "x-twitter-active-user",
         header::HeaderValue::from_static("yes"),
      );
      headers.insert(
         "x-twitter-client-language",
         header::HeaderValue::from_static("en"),
      );

      if let Some(tid) = tid
         && let Ok(val) = tid.parse()
      {
         headers.insert("x-client-transaction-id", val);
      }

      let response = self.client.get_with_headers(&url, &headers).await?;

      if !response.status().is_success() {
         let status = response.status();
         let body = response.text().await.unwrap_or_default();
         return Err(Error::Internal(format!(
            "Translation API error {status}: {body}"
         )));
      }

      #[expect(
         clippy::items_after_statements,
         reason = "local response type near its use"
      )]
      #[derive(Deserialize)]
      struct TranslationResponse {
         translation:               Option<String>,
         #[serde(rename = "sourceLanguage")]
         source_language:           Option<String>,
         #[serde(rename = "destinationLanguage")]
         dest_language:             Option<String>,
         #[serde(rename = "localizedSourceLanguage")]
         localized_source_language: Option<String>,
      }

      let bytes = response.bytes().await?;
      let resp: TranslationResponse = serde_json::from_slice(&bytes)
         .map_err(|err| Error::Internal(format!("Translation parse error: {err}")))?;

      Ok(Translation {
         text:                resp.translation.unwrap_or_default(),
         source_lang:         resp.source_language.unwrap_or_default(),
         dest_lang:           resp.dest_language.unwrap_or_default(),
         source_lang_display: resp.localized_source_language.unwrap_or_default(),
      })
   }

   /// Translate a tweet using Kagi Translate API.
   pub async fn kagi_translate(&self, tweet_id: &str, kagi_token: &str) -> Result<Translation> {
      use http_body_util::Full;
      use hyper_rustls::HttpsConnectorBuilder;
      use hyper_util::{
         client::legacy::Client as LegacyClient,
         rt::TokioExecutor,
      };
      use percent_encoding::{
         NON_ALPHANUMERIC,
         utf8_percent_encode,
      };

      #[derive(Deserialize)]
      struct KagiResponse {
         translation:       Option<String>,
         detected_language: Option<KagiDetectedLang>,
      }

      #[derive(Deserialize)]
      struct KagiDetectedLang {
         label: Option<String>,
      }

      // First fetch the tweet text
      let tweet = self.get_tweet(tweet_id).await?;
      if tweet.text.is_empty() {
         return Err(Error::Internal("Tweet has no text to translate".into()));
      }

      let payload = serde_json::json!({
         "text": tweet.text,
         "source_lang": tweet.lang,
         "target_lang": "en",
         "skip_definition": true,
         "model": "standard"
      });

      let url = format!(
         "https://translate.kagi.com/api/translate?token={}",
         utf8_percent_encode(kagi_token, NON_ALPHANUMERIC)
      );

      let body = payload.to_string();
      let uri: hyper::Uri = url
         .parse()
         .map_err(|err| Error::Internal(format!("invalid Kagi URL: {err}")))?;

      let connector = HttpsConnectorBuilder::new()
         .with_native_roots()
         .map_err(|err| Error::Internal(format!("TLS setup error: {err}")))?
         .https_only()
         .enable_http1()
         .build();

      let client = LegacyClient::builder(TokioExecutor::new()).build(connector);

      let request = hyper::Request::builder()
         .method(hyper::Method::POST)
         .uri(&uri)
         .header(header::HOST, "translate.kagi.com")
         .header(header::CONTENT_TYPE, "application/json")
         .body(Full::new(bytes::Bytes::from(body)))
         .map_err(|err| Error::Internal(format!("build Kagi request: {err}")))?;

      let resp = client
         .request(request)
         .await
         .map_err(|err| Error::Internal(format!("Kagi request failed: {err}")))?;

      let status = resp.status();
      let body_bytes = http_body_util::BodyExt::collect(resp.into_body())
         .await
         .map_err(|err| Error::Internal(format!("Kagi body read error: {err}")))?
         .to_bytes();

      if !status.is_success() {
         let body_text = String::from_utf8_lossy(&body_bytes);
         return Err(Error::Internal(format!(
            "Kagi API error {status}: {body_text}"
         )));
      }

      let kagi: KagiResponse = serde_json::from_slice(&body_bytes)
         .map_err(|err| Error::Internal(format!("Kagi parse error: {err}")))?;

      let source_display = kagi
         .detected_language
         .and_then(|dl| dl.label)
         .unwrap_or_else(|| tweet.lang.clone());

      Ok(Translation {
         text:                kagi.translation.unwrap_or_default(),
         source_lang:         tweet.lang,
         dest_lang:           "en".to_owned(),
         source_lang_display: source_display,
      })
   }

   /// Translate a tweet using the best available backend.
   /// Uses Kagi when a token is provided, otherwise falls back to Strato.
   pub async fn translate_auto(
      &self,
      tweet_id: &str,
      kagi_token: Option<&str>,
   ) -> Result<Translation> {
      if let Some(token) = kagi_token {
         self.kagi_translate(tweet_id, token).await
      } else {
         self.translate_tweet(tweet_id).await
      }
   }

   /// Get session pool health statistics.
   pub async fn get_session_health(&self) -> super::HealthResponse {
      self.sessions.get_health().await
   }

   /// Get detailed session debug info.
   pub async fn get_session_debug(&self) -> super::DebugResponse {
      self.sessions.get_debug().await
   }

   /// Get photo rail (up to 16 recent photos) for a user.
   pub async fn get_photo_rail(&self, user_id: &str) -> Result<Vec<GalleryPhoto>> {
      let timeline = self.get_user_media(user_id, None).await?;

      let mut photos = Vec::new();
      for mut tweet in timeline.content.into_iter().flatten() {
         // Extract ONE photo per tweet:
         // first photo > video thumb > gif thumb > card image
         let url = if !tweet.photos.is_empty() {
            Some(tweet.photos.swap_remove(0).url)
         } else if let Some(video) = tweet.video.take() {
            (!video.thumb.is_empty()).then_some(video.thumb)
         } else if let Some(gif) = tweet.gif.take() {
            (!gif.thumb.is_empty()).then_some(gif.thumb)
         } else if let Some(card) = tweet.card.take() {
            (!card.image.is_empty()).then_some(card.image)
         } else {
            None
         };

         if let Some(url) = url {
            photos.push(GalleryPhoto {
               url,
               tweet_id: tweet.id.to_string(),
               color: String::new(),
            });
            if photos.len() >= 10 {
               return Ok(photos);
            }
         }
      }

      Ok(photos)
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn formats_scheduled_audio_space_status() {
      let metadata = AudioSpaceMetadata {
         state: Some("NotStarted".to_owned()),
         scheduled_start: Some(1_784_203_200_000),
         ..AudioSpaceMetadata::default()
      };

      assert_eq!(
         audio_space_status(&metadata),
         "Scheduled · Jul 16, 2026 · 12:00 PM UTC"
      );
   }

   #[test]
   fn parses_recorded_audio_space_metadata() {
      let data: AudioSpaceData = serde_json::from_str(
         r#"{
            "audioSpace": {
               "metadata": {
                  "ended_at": "1784107519245",
                  "is_space_available_for_replay": true,
                  "state": "TimedOut",
                  "title": "Interlink enters a new phase",
                  "total_replay_watched": 4
               }
            }
         }"#,
      )
      .unwrap();
      let metadata = data.audio_space.unwrap().metadata.unwrap();

      assert_eq!(metadata.ended_at, Some(1_784_107_519_245));
      assert_eq!(audio_space_status(&metadata), "Replay available · 4 plays");
   }

   #[test]
   fn parses_live_broadcast_metadata() {
      let data: BroadcastsData = serde_json::from_str(
         r#"{
            "broadcasts": {
               "1XxyggAaLzvGM": {
                  "available_for_replay": true,
                  "image_url": "https://video.pscp.tv/latest.jpg",
                  "state": "RUNNING",
                  "status": "Stripe x PayPal",
                  "total_watched": "26",
                  "total_watching": "26",
                  "twitter_username": "tbpn",
                  "user_display_name": "TBPN"
               }
            }
         }"#,
      )
      .unwrap();
      let metadata = &data.broadcasts["1XxyggAaLzvGM"];

      assert_eq!(broadcast_status(metadata), "Live now · 26 watching");
      assert_eq!(
         hosted_by(&metadata.user_display_name, &metadata.twitter_username),
         Some("Hosted by TBPN (@tbpn)".to_owned())
      );
   }

   #[test]
   fn extracts_audio_space_id_from_canonical_url() {
      assert_eq!(
         space_id_from_url("https://x.com/i/spaces/1AxRnnrNvyDxl/peek?foo=bar"),
         Some("1AxRnnrNvyDxl")
      );
   }

   #[test]
   fn extracts_broadcast_id_from_canonical_url() {
      assert_eq!(
         broadcast_id_from_url("https://x.com/i/broadcasts/1XxyggAaLzvGM?foo=bar"),
         Some("1XxyggAaLzvGM")
      );
   }
}
