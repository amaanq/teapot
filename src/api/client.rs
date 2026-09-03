use std::time::Duration;

use axum::http::header;
use serde::{
   Deserialize,
   de::{
      DeserializeOwned,
      IgnoredAny,
   },
};
use tokio::time::timeout;

use super::{
   SessionLease,
   SessionPool,
   TidClient,
   budget::{
      self,
      ClientBudget,
   },
   endpoints,
   http::HttpClient,
   parser,
};
use crate::{
   api::schema::{
      AboutAccountData,
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
      AccountContext,
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

/// A search spends a `SearchTimeline` call and is never served from cache.
const SEARCH_COST: f64 = 2.0;

/// Community-cache strings are written by third parties, so cap them before
/// they reach a page.
fn clamp_community_value(value: Option<String>) -> String {
   value
      .map(|value| value.trim().chars().take(100).collect())
      .unwrap_or_default()
}

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
   budget:      ClientBudget,
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

      let tid = TidClient::new(client.clone(), sessions.clone());

      Self {
         client,
         sessions,
         tid,
         budget: ClientBudget::new(config.config.client_budget),
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
            TwitterError::UserSuspended => Err(Error::UserSuspended(error.message.clone())),
            TwitterError::RateLimited => Err(Error::RateLimited),
            TwitterError::TweetNotFound
            | TwitterError::TweetUnavailable
            | TwitterError::NoStatusFound
            | TwitterError::TweetUnavailable421
            | TwitterError::TweetCensored => Err(Error::TweetNotFound(error.message.clone())),
            // 326 locks *our* cookie session, not the profile the reader
            // opened, so it rotates the session like a dead token does.
            TwitterError::Locked | TwitterError::InvalidToken | TwitterError::BadToken => {
               Err(Error::SessionRejected(error.message.clone()))
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
      let session = self.charged_session(endpoint, None).await?;
      self
         .graphql_request_with_session(session, endpoint, variables, features, field_toggles, None)
         .await
   }

   /// `retry_kind` pins the replacement when the caller picked the endpoint and
   /// variables from the first session's kind, since an OAuth request retried
   /// through a cookie session would still send the OAuth endpoint.
   async fn graphql_request_with_session<T>(
      &self,
      session: SessionLease,
      endpoint: &str,
      variables: &str,
      features: &str,
      field_toggles: Option<&str>,
      retry_kind: Option<SessionKind>,
   ) -> Result<T>
   where
      T: DeserializeOwned,
   {
      let session_id = session.id;
      let first = self
         .graphql_request_inner(&session, endpoint, variables, features, field_toggles)
         .await;

      match first {
         Err(Error::SessionRejected(_) | Error::TransientUpstream) => {},
         Err(Error::RateLimited)
            if self
               .sessions
               .has_unlimited(endpoint, retry_kind, session_id)
               .await => {},
         other => return other,
      }

      tracing::warn!(
         session_id,
         endpoint,
         "session unusable, retrying on another"
      );
      drop(session);
      self.charge(endpoint).await?;
      let retry = match self
         .sessions
         .acquire_excluding(endpoint, retry_kind, Some(session_id))
         .await
      {
         Ok(retry) => retry,
         Err(err) => {
            self.refund(endpoint).await;
            return first.and(Err(err));
         },
      };
      self
         .graphql_request_inner(&retry, endpoint, variables, features, field_toggles)
         .await
   }

   /// Take a session for one upstream call, billing the caller first.
   ///
   /// The only way to reach X, so a caller cannot spend quota by taking a
   /// route that forgot to charge. Billing precedes acquisition so a spent
   /// caller neither holds a lease nor waits on a permit, and a lease that
   /// never materialises is refunded rather than charged for nothing.
   pub(crate) async fn charged_session(
      &self,
      endpoint: &str,
      kind: Option<SessionKind>,
   ) -> Result<SessionLease> {
      self.charge(endpoint).await?;
      match self.sessions.acquire(endpoint, kind).await {
         Ok(session) => Ok(session),
         Err(err) => {
            self.refund(endpoint).await;
            Err(err)
         },
      }
   }

   /// [`charged_session`](Self::charged_session) where the endpoint depends on
   /// which kind of session is chosen.
   pub(crate) async fn charged_session_by_kind(
      &self,
      cookie_api: &str,
      oauth_api: &str,
   ) -> Result<SessionLease> {
      self.charge(cookie_api).await?;
      match self.sessions.acquire_by_kind(cookie_api, oauth_api).await {
         Ok(session) => Ok(session),
         Err(err) => {
            self.refund(cookie_api).await;
            Err(err)
         },
      }
   }

   /// Give back what [`charge`](Self::charge) took for a call that never ran.
   async fn refund(&self, endpoint: &str) {
      if let Some(client) = budget::current_client() {
         self.budget.refund(&client, Self::cost_of(endpoint)).await;
      }
   }

   /// A search spends a `SearchTimeline` call and is never served from cache.
   fn cost_of(endpoint: &str) -> f64 {
      if endpoint.contains("SearchTimeline") {
         SEARCH_COST
      } else {
         1.0
      }
   }

   /// Charge one upstream call to the caller of the current request.
   ///
   /// Called before a session is acquired, so a spent caller neither takes a
   /// lease nor waits on a permit, and once per attempt so a retry costs what
   /// it spends.
   async fn charge(&self, endpoint: &str) -> Result<()> {
      let cost = Self::cost_of(endpoint);
      if let Some(client) = budget::current_client()
         && !self.budget.try_spend(&client, cost).await
      {
         tracing::debug!(?client, endpoint, "client budget exhausted");
         return Err(Error::ClientBudgetExhausted);
      }

      Ok(())
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
      let headers = self
         .graphql_headers(
            session,
            base_url,
            endpoint,
            variables,
            features,
            field_toggles,
         )
         .await?;

      let response = self.client.get_with_headers(&url, &headers).await?;
      let (bytes, limit_recorded) = self.account_response(session, endpoint, response).await?;

      // Check for API errors before full deserialization.
      // Mark the session as limited on token errors so the retry picks
      // a different one.
      let api_check = Self::check_api_errors(&bytes);
      if let Err(Error::SessionRejected(ref msg)) = api_check {
         self.sessions.mark_rejected(session.id).await;
         return Err(Error::SessionRejected(msg.clone()));
      }
      if !limit_recorded && matches!(api_check, Err(Error::RateLimited)) {
         self
            .sessions
            .mark_endpoint_limited(session.id, endpoint)
            .await;
      }
      api_check?;

      let resp = serde_json::from_slice::<GqlResponse<T>>(&bytes)
         .map_err(|err| Error::Internal(format!("Response parse error: {err}")))?;
      Ok(resp.data)
   }

   /// Every authenticated call to X goes through here, or its 429s and refused
   /// credentials never reach the pool. The flag reports whether the headers
   /// recorded a real window, so a code 88 does not overwrite one.
   async fn account_response(
      &self,
      session: &SessionLease,
      endpoint: &str,
      response: super::http::Response,
   ) -> Result<(bytes::Bytes, bool)> {
      let account_scoped = response.headers().contains_key("x-rate-limit-remaining");
      let limit_recorded = if let Some(remaining) = response.headers().get("x-rate-limit-remaining")
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
         remaining_val <= 0 && reset > time::OffsetDateTime::now_utc().unix_timestamp()
      } else {
         false
      };

      if !response.status().is_success() {
         let status = response.status();
         let body = response.text().await.unwrap_or_default();
         tracing::error!(
            session_id = session.id,
            session_user = %session.username,
            account_scoped,
            "API request failed: {status} - {body}"
         );

         if status.as_u16() == 429 {
            // An edge 429 carries no `x-rate-limit-*` headers, so it is aimed
            // at the address every session shares rather than at this account.
            if account_scoped {
               if !limit_recorded {
                  self
                     .sessions
                     .mark_endpoint_limited(session.id, endpoint)
                     .await;
               }
            } else {
               self.sessions.back_off_edge();
            }
            return Err(Error::RateLimited);
         }

         // X answers a stale search or a WAF hiccup with a bodyless 404 across
         // every session, so only a JSON 404 names a resource that is gone.
         if status.as_u16() == 404 {
            if body.trim().is_empty() || serde_json::from_str::<IgnoredAny>(&body).is_err() {
               return Err(Error::TransientUpstream);
            }
            return Err(Error::NotFound("Not found".into()));
         }

         // Only 401 means the credentials themselves were refused. A 403 is an
         // authenticated request denied a particular resource, unless it
         // carries code 326, which is the session itself being locked.
         if status.as_u16() == 401
            || (status.as_u16() == 403
               && Self::check_api_errors(body.as_bytes())
                  .is_err_and(|err| matches!(err, Error::SessionRejected(_))))
         {
            self.sessions.mark_rejected(session.id).await;
            return Err(Error::SessionRejected(format!("Status {status}: {body}")));
         }

         return Err(Error::TwitterApi(format!("Status {status}: {body}")));
      }

      Ok((response.bytes().await?, limit_recorded))
   }

   async fn graphql_headers(
      &self,
      session: &SessionLease,
      base_url: &str,
      endpoint: &str,
      variables: &str,
      features: &str,
      field_toggles: Option<&str>,
   ) -> Result<header::HeaderMap> {
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

      Ok(headers)
   }

   async fn cookie_json_request<T>(&self, session_key: &str, api_path: &str, url: &str) -> Result<T>
   where
      T: DeserializeOwned,
   {
      let session = self
         .charged_session(session_key, Some(SessionKind::Cookie))
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
      let (bytes, _) = self
         .account_response(&session, session_key, response)
         .await?;
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
