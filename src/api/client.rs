use std::time::Duration;

use axum::http::header;
use serde::{
   Deserialize,
   de::DeserializeOwned,
};
use tokio::time::timeout;

use super::{
   SessionLease,
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
         let legacy = user.legacy.as_ref()?;
         legacy.name.as_deref()
      })
      .unwrap_or_default();
   let username = user
      .core
      .as_ref()
      .and_then(|core| core.screen_name.as_deref())
      .or_else(|| {
         let legacy = user.legacy.as_ref()?;
         legacy.screen_name.as_deref()
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
   client:      HttpClient,
   sessions:    SessionPool,
   tid:         TidClient,
   tid_enabled: bool,
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

      let api_proxy = if config.config.api_proxy.is_empty() {
         &config.config.proxy
      } else {
         &config.config.api_proxy
      };
      let client =
         HttpClient::new(api_proxy, &config.config.proxy_auth).with_default_headers(headers);

      let tid = TidClient::new(client.clone());

      Self {
         client,
         sessions,
         tid,
         tid_enabled: !config.config.disable_tid,
      }
   }

   async fn bearer_and_tid(&self, api_path: &str) -> (&'static str, Option<String>) {
      if !self.tid_enabled {
         return (endpoints::BEARER_TOKEN_NO_TID, None);
      }
      self
         .tid
         .generate(api_path)
         .await
         .map_or((endpoints::BEARER_TOKEN_NO_TID, None), |tid| {
            (endpoints::BEARER_TOKEN, Some(tid))
         })
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
   async fn graphql_request<T>(
      &self,
      endpoint: &str,
      variables: &str,
      features: &str,
      field_toggles: Option<&str>,
   ) -> Result<T>
   where
      T: DeserializeOwned,
   {
      let session = self.sessions.acquire(endpoint, None).await?;
      self
         .graphql_request_with_session(session, endpoint, variables, features, field_toggles)
         .await
   }

   async fn graphql_request_with_session<T>(
      &self,
      session: SessionLease,
      endpoint: &str,
      variables: &str,
      features: &str,
      field_toggles: Option<&str>,
   ) -> Result<T>
   where
      T: DeserializeOwned,
   {
      let session_id = session.id;
      let session_kind = session.kind;
      match self
         .graphql_request_inner(&session, endpoint, variables, features, field_toggles)
         .await
      {
         Err(Error::TwitterApi(ref msg)) if msg.starts_with("Invalid token") => {
            tracing::warn!("Token rejected, retrying with another session: {msg}");
            drop(session);
            let retry = self
               .sessions
               .acquire_excluding(endpoint, Some(session_kind), Some(session_id))
               .await?;
            self
               .graphql_request_inner(&retry, endpoint, variables, features, field_toggles)
               .await
         },
         other => other,
      }
   }

   /// Inner implementation of [`graphql_request`].
   async fn graphql_request_inner<T>(
      &self,
      session: &SessionLease,
      endpoint: &str,
      variables: &str,
      features: &str,
      field_toggles: Option<&str>,
   ) -> Result<T>
   where
      T: DeserializeOwned,
   {
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
            let mut oauth_params = vec![("variables", variables), ("features", features)];
            if let Some(toggles) = field_toggles {
               oauth_params.push(("fieldToggles", toggles));
            }
            let auth = super::oauth1_sign(
               "GET",
               &auth_url,
               &oauth_params,
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
            let (bearer, tid) = self.bearer_and_tid(&api_path).await;

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

   async fn cookie_json_request<T>(&self, session_key: &str, api_path: &str, url: &str) -> Result<T>
   where
      T: DeserializeOwned,
   {
      let session = self
         .sessions
         .acquire(session_key, Some(SessionKind::Cookie))
         .await?;

      let (bearer, tid) = self.bearer_and_tid(api_path).await;
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
      drop(session);
      serde_json::from_slice(&bytes)
         .map_err(|err| Error::Internal(format!("Response parse error: {err}")))
   }
}

#[path = "client_endpoints.rs"] mod endpoint_methods;
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
