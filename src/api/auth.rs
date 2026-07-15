use std::{
   collections::HashMap,
   fmt::Write as _,
   ops::Deref,
   path::Path,
   sync::{
      Arc,
      atomic::{
         AtomicUsize,
         Ordering,
      },
   },
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
   sync::{
      OwnedSemaphorePermit,
      RwLock,
      Semaphore,
   },
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
   sessions: Vec<Arc<SessionSlot>>,
   limits:   Arc<RwLock<HashMap<i64, SessionLimits>>>,
   cursor:   Arc<AtomicUsize>,
}

struct SessionSlot {
   credentials:     Arc<SessionCredentials>,
   permits:         Arc<Semaphore>,
   max_concurrency: usize,
}

/// A session and the concurrency permit held for its request lifetime.
pub struct SessionLease {
   credentials: Arc<SessionCredentials>,
   _permit:     OwnedSemaphorePermit,
}

impl Deref for SessionLease {
   type Target = SessionCredentials;

   fn deref(&self) -> &Self::Target {
      &self.credentials
   }
}

impl SessionPool {
   /// Load sessions from a JSONL file.
   #[expect(
      clippy::cognitive_complexity,
      reason = "session loading has inherent branching"
   )]
   pub async fn load(path: &str, max_concurrent_requests: u32) -> Result<Self> {
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

      let max_concurrency = usize::try_from(max_concurrent_requests.max(1)).unwrap_or(usize::MAX);
      let mut sessions = Vec::with_capacity(parsed.len());
      let mut limits = HashMap::with_capacity(parsed.len());

      for session in parsed {
         let (creds, lims) = session.into_credentials_and_limits();
         let id = creds.id;
         sessions.push(Arc::new(SessionSlot {
            credentials: Arc::new(creds),
            permits: Arc::new(Semaphore::new(max_concurrency)),
            max_concurrency,
         }));
         limits.insert(id, lims);
      }

      Ok(Self {
         sessions,
         limits: Arc::new(RwLock::new(limits)),
         cursor: Arc::new(AtomicUsize::new(0)),
      })
   }

   /// Acquire an available session for an API request.
   pub(crate) async fn acquire(
      &self,
      api: &str,
      required_kind: Option<SessionKind>,
   ) -> Result<SessionLease> {
      self.acquire_excluding(api, required_kind, None).await
   }

   /// Acquire a session while avoiding a token that was just rejected.
   pub(crate) async fn acquire_excluding(
      &self,
      api: &str,
      required_kind: Option<SessionKind>,
      excluded_id: Option<i64>,
   ) -> Result<SessionLease> {
      if self.sessions.is_empty() {
         return Err(Error::NoSessions);
      }

      let limits = self.limits.read().await;
      let start = self.cursor.fetch_add(1, Ordering::Relaxed);
      let eligible = (0..self.sessions.len())
         .map(|offset| &self.sessions[(start.wrapping_add(offset)) % self.sessions.len()])
         .filter(|slot| {
            excluded_id != Some(slot.credentials.id)
               && required_kind.is_none_or(|kind| slot.credentials.kind == kind)
         })
         .collect::<Vec<_>>();

      if eligible.is_empty() {
         return Err(Error::NoSessions);
      }

      // Prefer a non-limited session with a permit immediately available.
      for slot in &eligible {
         let limited = limits
            .get(&slot.credentials.id)
            .is_some_and(|session_limits| session_limits.is_limited(api));
         if !limited && let Ok(permit) = Arc::clone(&slot.permits).try_acquire_owned() {
            return Ok(SessionLease {
               credentials: Arc::clone(&slot.credentials),
               _permit:     permit,
            });
         }
      }

      // If every usable session is busy, queue on the next non-limited one. If
      // all are rate-limited, use the session whose reset is earliest.
      let chosen = eligible
         .iter()
         .find(|slot| {
            !limits
               .get(&slot.credentials.id)
               .is_some_and(|session_limits| session_limits.is_limited(api))
         })
         .copied()
         .or_else(|| {
            eligible.iter().copied().min_by_key(|slot| {
               limits
                  .get(&slot.credentials.id)
                  .and_then(|session_limits| session_limits.apis.get(api))
                  .map_or(i64::MAX, |rate| rate.reset)
            })
         })
         .ok_or(Error::NoSessions)?;
      let credentials = Arc::clone(&chosen.credentials);
      let permits = Arc::clone(&chosen.permits);
      drop(limits);

      let permit = permits
         .acquire_owned()
         .await
         .map_err(|_| Error::Internal("session request limiter closed".into()))?;
      Ok(SessionLease {
         credentials,
         _permit: permit,
      })
   }

   /// Update rate limit info for a session.
   ///
   /// A successful response with valid rate-limit headers proves the session
   /// is working, so clear the global `limited` flag if it has expired.
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
         // The session responded, so clear its expired global limit
         if lim.limited && !lim.is_limited(api) {
            lim.limited = false;
         }
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
         .map(|slot| {
            let sess = &slot.credentials;
            let lim = limits.get(&sess.id);
            SessionDetail {
               id:         sess.id,
               username:   sess.username.clone(),
               kind:       sess.kind,
               limited:    lim.is_some_and(|sl| sl.limited),
               limited_at: lim.map_or(0, |sl| sl.limited_at),
               pending:    i32::try_from(
                  slot
                     .max_concurrency
                     .saturating_sub(slot.permits.available_permits()),
               )
               .unwrap_or(i32::MAX),
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

   // RFC 5849 requires sorting by percent-encoded key and then encoded value.
   let mut all_params: Vec<(&str, &str)> = params.to_vec();
   all_params.extend(oauth_params.iter().copied());
   let param_string = normalized_parameter_string(&all_params);

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
   let mut encoded = String::with_capacity(input.len());
   for byte in input.as_bytes() {
      if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
         encoded.push(char::from(*byte));
      } else {
         let _ = write!(encoded, "%{byte:02X}");
      }
   }
   encoded
}

fn normalized_parameter_string(params: &[(&str, &str)]) -> String {
   let mut encoded = params
      .iter()
      .map(|&(key, value)| (percent_encode(key), percent_encode(value)))
      .collect::<Vec<_>>();
   encoded.sort();
   encoded
      .into_iter()
      .map(|(key, value)| format!("{key}={value}"))
      .collect::<Vec<_>>()
      .join("&")
}

#[cfg(test)]
mod tests {
   use std::{
      env,
      process,
      time::Duration,
   };

   use tokio::{
      fs,
      time::timeout,
   };

   use super::{
      SessionKind,
      SessionPool,
      normalized_parameter_string,
      percent_encode,
   };

   #[test]
   fn oauth_percent_encoding_follows_rfc_5849() {
      assert_eq!(
         percent_encode("Ladies + Gentlemen"),
         "Ladies%20%2B%20Gentlemen"
      );
      assert_eq!(percent_encode("-._~"), "-._~");
      assert_eq!(percent_encode("☃"), "%E2%98%83");
   }

   #[test]
   fn oauth_parameters_sort_by_encoded_key_and_value() {
      let params = [
         ("b5", "="),
         ("a3", "a"),
         ("c@", ""),
         ("a2", "r b"),
         ("c2", ""),
         ("a3", "2 q"),
      ];
      assert_eq!(
         normalized_parameter_string(&params),
         "a2=r%20b&a3=2%20q&a3=a&b5=%3D&c%40=&c2="
      );
   }

   #[tokio::test]
   async fn pool_filters_kinds_rotates_and_enforces_concurrency() {
      let path = env::temp_dir().join(format!("teapot-session-pool-{}.jsonl", process::id()));
      let sessions = concat!(
         r#"{"id":1,"username":"cookie","kind":"cookie","auth_token":"a","ct0":"c"}"#,
         "\n",
         r#"{"id":2,"username":"oauth-one","kind":"oauth","oauth_token":"a","oauth_secret":"s"}"#,
         "\n",
         r#"{"id":3,"username":"oauth-two","kind":"oauth","oauth_token":"b","oauth_secret":"t"}"#,
         "\n"
      );
      fs::write(&path, sessions).await.unwrap();
      let pool = SessionPool::load(path.to_str().unwrap(), 1).await.unwrap();

      let cookie = pool
         .acquire("cookie-api", Some(SessionKind::Cookie))
         .await
         .unwrap();
      assert_eq!(cookie.id, 1);
      assert!(
         timeout(
            Duration::from_millis(10),
            pool.acquire("cookie-api", Some(SessionKind::Cookie)),
         )
         .await
         .is_err()
      );
      drop(cookie);

      let first = pool
         .acquire("oauth-api", Some(SessionKind::OAuth))
         .await
         .unwrap();
      let first_id = first.id;
      drop(first);
      let second = pool
         .acquire("oauth-api", Some(SessionKind::OAuth))
         .await
         .unwrap();
      assert_ne!(first_id, second.id);

      fs::remove_file(path).await.unwrap();
   }
}
