use std::{
   collections::HashMap,
   path::Path,
   sync::Arc,
   time::{
      SystemTime,
      UNIX_EPOCH,
   },
};

use data_encoding::BASE64;
use ring::hmac;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use tokio::{
   fs,
   sync::RwLock,
};

use crate::{
   error::{
      Error,
      Result,
   },
   types::{
      RateLimit,
      Session,
      SessionCredentials,
      SessionKind,
      SessionLimits,
   },
};

#[derive(Serialize)]
pub struct HealthResponse {
   pub sessions:  SessionStats,
   pub requests:  RequestStats,
   pub timestamp: String,
}

#[derive(Serialize)]
pub struct SessionStats {
   pub total:     usize,
   pub limited:   usize,
   pub available: usize,
}

#[derive(Serialize)]
pub struct RequestStats {
   pub total:  i32,
   pub by_api: HashMap<String, i32>,
}

#[derive(Serialize)]
pub struct DebugResponse {
   pub sessions:  Vec<SessionDetail>,
   pub count:     usize,
   pub timestamp: String,
}

#[derive(Serialize)]
pub struct SessionDetail {
   pub id:         i64,
   pub username:   String,
   pub kind:       SessionKind,
   pub limited:    bool,
   pub limited_at: i64,
   pub pending:    i32,
   pub apis:       HashMap<String, RateLimit>,
}

/// Pool of authentication sessions for Twitter API.
#[derive(Clone)]
pub struct SessionPool {
   sessions: Vec<Arc<SessionCredentials>>,
   limits:   Arc<RwLock<HashMap<i64, SessionLimits>>>,
}

impl SessionPool {
   /// Load sessions from a JSONL file.
   #[expect(
      clippy::cognitive_complexity,
      reason = "session loading has inherent branching"
   )]
   pub async fn load(path: &str) -> Result<Self> {
      let parsed = if Path::new(path).exists() {
         let content = fs::read_to_string(path).await?;
         let mut parsed = Vec::new();

         for line in content.lines() {
            if line.trim().is_empty() {
               continue;
            }
            match serde_json::from_str::<Session>(line) {
               Ok(session) => parsed.push(session),
               Err(err) => {
                  tracing::warn!("Failed to parse session: {err}");
               },
            }
         }

         tracing::info!("Loaded {} sessions", parsed.len());
         parsed
      } else {
         tracing::warn!("Sessions file not found: {path}");
         Vec::new()
      };

      let mut sessions = Vec::with_capacity(parsed.len());
      let mut limits = HashMap::with_capacity(parsed.len());

      for session in parsed {
         let (creds, lims) = session.into_credentials_and_limits();
         let id = creds.id;
         sessions.push(Arc::new(creds));
         limits.insert(id, lims);
      }

      Ok(Self {
         sessions,
         limits: Arc::new(RwLock::new(limits)),
      })
   }

   /// Get an available session for making API requests.
   pub async fn get_session(&self, api: &str) -> Result<Arc<SessionCredentials>> {
      if self.sessions.is_empty() {
         return Err(Error::NoSessions);
      }

      let limits = self.limits.read().await;

      // Find a session that isn't rate limited for this API
      for session in &self.sessions {
         if let Some(lim) = limits.get(&session.id) {
            if !lim.is_limited(api) {
               return Ok(Arc::clone(session));
            }
         } else {
            // No limits recorded yet — session is available
            return Ok(Arc::clone(session));
         }
      }

      // If all sessions are limited, return the one with the earliest reset time
      let best = self
         .sessions
         .iter()
         .min_by_key(|sess| {
            limits
               .get(&sess.id)
               .and_then(|lim| lim.apis.get(api))
               .map_or(i64::MAX, |rate| rate.reset)
         })
         .ok_or(Error::NoSessions)?;

      Ok(Arc::clone(best))
   }

   /// Update rate limit info for a session.
   pub async fn update_session_limit(
      &self,
      session_id: i64,
      api: &str,
      limit: i32,
      remaining: i32,
      reset: i64,
   ) {
      let mut limits = self.limits.write().await;

      if let Some(lim) = limits.get_mut(&session_id) {
         lim.update_limit(api, limit, remaining, reset);
      }
   }

   /// Mark a session as globally rate limited.
   pub async fn mark_limited(&self, session_id: i64) {
      let mut limits = self.limits.write().await;

      if let Some(lim) = limits.get_mut(&session_id) {
         lim.limited = true;
         lim.limited_at = time::OffsetDateTime::now_utc().unix_timestamp();
      }
   }

   /// Get the session kind that would be used for a given API.
   pub async fn get_session_kind(&self, api: &str) -> SessionKind {
      self
         .get_session(api)
         .await
         .map_or_else(|_| SessionKind::default(), |session| session.kind)
   }

   /// Get session count.
   pub const fn len(&self) -> usize {
      self.sessions.len()
   }

   /// Check if pool is empty.
   pub const fn is_empty(&self) -> bool {
      self.sessions.is_empty()
   }

   /// Get health statistics about the session pool.
   #[expect(
      clippy::iter_over_hash_type,
      reason = "iteration order irrelevant for aggregation"
   )]
   pub async fn get_health(&self) -> HealthResponse {
      let limits = self.limits.read().await;

      let mut limited_count = 0;
      let mut total_requests = 0;
      let mut by_api = HashMap::<String, i32>::new();

      for lim in limits.values() {
         if lim.limited {
            limited_count += 1;
         }

         for (api, limit_info) in &lim.apis {
            let used = limit_info.limit - limit_info.remaining;
            total_requests += used;
            *by_api.entry(api.clone()).or_default() += used;
         }
      }
      drop(limits);

      HealthResponse {
         sessions:  SessionStats {
            total:     self.sessions.len(),
            limited:   limited_count,
            available: self.sessions.len() - limited_count,
         },
         requests:  RequestStats {
            total: total_requests,
            by_api,
         },
         timestamp: time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
      }
   }

   /// Get detailed debug info about sessions.
   pub async fn get_debug(&self) -> DebugResponse {
      let limits = self.limits.read().await;

      let sessions = self
         .sessions
         .iter()
         .map(|sess| {
            let lim = limits.get(&sess.id);
            SessionDetail {
               id:         sess.id,
               username:   sess.username.clone(),
               kind:       sess.kind,
               limited:    lim.is_some_and(|sl| sl.limited),
               limited_at: lim.map_or(0, |sl| sl.limited_at),
               pending:    lim.map_or(0, |sl| sl.pending),
               apis:       lim.map(|sl| sl.apis.clone()).unwrap_or_default(),
            }
         })
         .collect();

      DebugResponse {
         sessions,
         count: self.sessions.len(),
         timestamp: time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
      }
   }
}

/// Sign a request with `OAuth1`.
pub fn oauth1_sign(
   method: &str,
   url: &str,
   params: &[(&str, &str)],
   oauth_token: &str,
   oauth_secret: &str,
) -> String {
   // OAuth parameters
   let timestamp = time::OffsetDateTime::now_utc().unix_timestamp().to_string();
   let nonce = format!(
      "{:032x}",
      SystemTime::now()
         .duration_since(UNIX_EPOCH)
         .unwrap()
         .as_nanos()
   );

   let mut oauth_params = vec![
      ("oauth_consumer_key", super::endpoints::CONSUMER_KEY),
      ("oauth_nonce", &nonce),
      ("oauth_signature_method", "HMAC-SHA1"),
      ("oauth_timestamp", &timestamp),
      ("oauth_token", oauth_token),
      ("oauth_version", "1.0"),
   ];

   // Combine all parameters and sort
   let mut all_params: Vec<(&str, &str)> = params.to_vec();
   all_params.extend(oauth_params.iter().copied());
   all_params.sort_by(|lhs, rhs| lhs.0.cmp(rhs.0));

   // Create parameter string
   let param_string = all_params
      .iter()
      .map(|&(key, val)| format!("{}={}", percent_encode(key), percent_encode(val)))
      .collect::<Vec<_>>()
      .join("&");

   // Create signature base string
   let base_string = format!(
      "{}&{}&{}",
      method.to_uppercase(),
      percent_encode(url),
      percent_encode(&param_string)
   );

   // Create signing key
   let signing_key = format!(
      "{}&{}",
      percent_encode(super::endpoints::CONSUMER_SECRET),
      percent_encode(oauth_secret)
   );

   // Generate signature
   let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, signing_key.as_bytes());
   let tag = hmac::sign(&key, base_string.as_bytes());
   let signature = BASE64.encode(tag.as_ref());

   oauth_params.push(("oauth_signature", &signature));

   // Build Authorization header
   let auth_header = oauth_params
      .iter()
      .map(|&(param, val)| format!("{}=\"{}\"", param, percent_encode(val)))
      .collect::<Vec<_>>()
      .join(", ");

   format!("OAuth {auth_header}")
}

fn percent_encode(input: &str) -> String {
   percent_encoding::utf8_percent_encode(input, percent_encoding::NON_ALPHANUMERIC).to_string()
}
