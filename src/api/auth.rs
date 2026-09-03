use std::{
   collections::{
      BTreeMap,
      HashMap,
   },
   env,
   fmt::Write as _,
   ops::Deref,
   path::Path,
   sync::{
      Arc,
      atomic::{
         AtomicI64,
         AtomicUsize,
         Ordering,
      },
   },
   time::{
      Duration,
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
      Mutex,
      Notify,
      OwnedSemaphorePermit,
      RwLock,
      Semaphore,
   },
   time::sleep,
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
   pub rejected:  usize,
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
   pub rejected:   bool,
   pub limited_at: i64,
   pub pending:    i32,
   pub apis:       HashMap<String, RateLimit>,
}

/// Pool of authentication sessions for Twitter API.
#[derive(Clone)]
pub struct SessionPool {
   sessions:     Vec<Arc<SessionSlot>>,
   limits:       Arc<RwLock<HashMap<i64, SessionLimits>>>,
   cursor:       Arc<AtomicUsize>,
   state:        Option<Arc<LimitState>>,
   edge_backoff: Arc<AtomicI64>,
}

/// Writes the rate-limit map to disk so a restart does not forget which
/// sessions X has already cut off.
struct LimitState {
   path:    String,
   dirty:   Notify,
   writing: Mutex<()>,
}

const STATE_FLUSH_INTERVAL: Duration = Duration::from_secs(15);

const EDGE_BACKOFF_SECS: i64 = 30;

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

      // Defaults on, like the sessions path, so a deployment that is not the
      // Nix service does not silently forget its limits across a restart.
      // Empty opts out.
      let state_path =
         env::var("TEAPOT_SESSION_STATE_FILE").unwrap_or_else(|_| "session-limits.json".to_owned());
      let state = if state_path.is_empty() {
         None
      } else {
         Self::restore(&state_path, &mut limits).await;
         Some(Arc::new(LimitState {
            path:    state_path,
            dirty:   Notify::new(),
            writing: Mutex::new(()),
         }))
      };

      let pool = Self {
         sessions,
         limits: Arc::new(RwLock::new(limits)),
         cursor: Arc::new(AtomicUsize::new(0)),
         state,
         edge_backoff: Arc::new(AtomicI64::new(0)),
      };
      pool.spawn_flusher();

      Ok(pool)
   }

   async fn restore(path: &str, limits: &mut HashMap<i64, SessionLimits>) {
      let Ok(content) = fs::read_to_string(path).await else {
         return;
      };
      match serde_json::from_str::<BTreeMap<i64, SessionLimits>>(&content) {
         Ok(saved) => {
            let mut restored = 0_usize;
            for (id, saved_limits) in saved {
               if let Some(slot) = limits.get_mut(&id) {
                  *slot = saved_limits;
                  restored += 1;
               }
            }
            tracing::info!("Restored rate-limit state for {restored} sessions");
         },
         Err(err) => tracing::warn!("Ignoring unreadable session state: {err}"),
      }
   }

   fn spawn_flusher(&self) {
      let Some(state) = self.state.clone() else {
         return;
      };
      let limits = Arc::clone(&self.limits);
      tokio::spawn(async move {
         loop {
            state.dirty.notified().await;
            // Coalesce, since update_session_limit fires on every response.
            sleep(STATE_FLUSH_INTERVAL).await;

            if let Err(err) = Self::store(&state, &limits).await {
               tracing::warn!("Failed to store session state: {err}");
               state.dirty.notify_one();
            }
         }
      });
   }

   async fn store(state: &LimitState, limits: &RwLock<HashMap<i64, SessionLimits>>) -> Result<()> {
      let _writing = state.writing.lock().await;
      let snapshot = serde_json::to_string(&*limits.read().await)?;
      let temporary = format!("{}.tmp", state.path);
      fs::write(&temporary, snapshot).await?;
      fs::rename(&temporary, &state.path).await?;
      Ok(())
   }

   fn mark_dirty(&self) {
      if let Some(state) = self.state.as_ref() {
         state.dirty.notify_one();
      }
   }

   /// Write the state before returning, for the transitions worth a restart.
   async fn persist_now(&self) {
      if let Some(state) = self.state.as_ref()
         && let Err(err) = Self::store(state, &self.limits).await
      {
         tracing::warn!("Failed to store session state: {err}");
         self.mark_dirty();
      }
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
      self.acquire_with(|_| api, required_kind, excluded_id).await
   }

   /// Acquire when the endpoint depends on which kind of session is chosen.
   ///
   /// The OAuth and cookie flows call different endpoints for the same feature,
   /// and each keeps its own budget, so picking a session against one key and
   /// then spending the other hides an exhausted bucket from selection.
   pub(crate) async fn acquire_by_kind(
      &self,
      cookie_api: &str,
      oauth_api: &str,
   ) -> Result<SessionLease> {
      self
         .acquire_with(
            |kind| {
               match kind {
                  SessionKind::OAuth => oauth_api,
                  SessionKind::Cookie => cookie_api,
               }
            },
            None,
            None,
         )
         .await
   }

   /// Pause the whole pool after X's edge refuses the host rather than the
   /// account. Every session leaves from one address, so the block is shared
   /// and flagging the session that happened to draw it walks the rotation.
   pub fn back_off_edge(&self) {
      let now = time::OffsetDateTime::now_utc().unix_timestamp();
      let previous = self
         .edge_backoff
         .swap(now + EDGE_BACKOFF_SECS, Ordering::Relaxed);
      if previous <= now {
         tracing::warn!(
            "upstream edge refused the host, pausing all sessions for {EDGE_BACKOFF_SECS}s"
         );
      }
   }

   fn edge_blocked(&self) -> bool {
      self.edge_backoff.load(Ordering::Relaxed) > time::OffsetDateTime::now_utc().unix_timestamp()
   }

   /// Whether some other session still has budget for `api`.
   ///
   /// Filtered the same way acquisition is, `required_kind` included, so a
   /// caller pinned to one kind is not told a replacement exists that
   /// acquisition will then refuse.
   pub(crate) async fn has_unlimited(
      &self,
      api: &str,
      required_kind: Option<SessionKind>,
      excluded_id: i64,
   ) -> bool {
      if self.edge_blocked() {
         return false;
      }
      let limits = self.limits.read().await;
      self.sessions.iter().any(|slot| {
         slot.credentials.id != excluded_id
            && required_kind.is_none_or(|kind| slot.credentials.kind == kind)
            && limits
               .get(&slot.credentials.id)
               .is_none_or(|session_limits| {
                  !session_limits.rejected && !session_limits.is_limited(api)
               })
      })
   }

   async fn acquire_with<'a>(
      &self,
      api_for: impl Fn(SessionKind) -> &'a str + Send,
      required_kind: Option<SessionKind>,
      excluded_id: Option<i64>,
   ) -> Result<SessionLease> {
      if self.sessions.is_empty() {
         return Err(Error::NoSessions);
      }
      if self.edge_blocked() {
         return Err(Error::RateLimited);
      }

      let limits = self.limits.read().await;
      let start = self.cursor.fetch_add(1, Ordering::Relaxed);
      let eligible = (0..self.sessions.len())
         .map(|offset| &self.sessions[(start.wrapping_add(offset)) % self.sessions.len()])
         .filter(|slot| {
            excluded_id != Some(slot.credentials.id)
               && required_kind.is_none_or(|kind| slot.credentials.kind == kind)
               && !limits
                  .get(&slot.credentials.id)
                  .is_some_and(|session_limits| session_limits.rejected)
         })
         .collect::<Vec<_>>();

      if eligible.is_empty() {
         return Err(Error::NoSessions);
      }

      // Prefer a non-limited session with a permit immediately available.
      for slot in &eligible {
         let limited = limits
            .get(&slot.credentials.id)
            .is_some_and(|session_limits| {
               session_limits.is_limited(api_for(slot.credentials.kind))
            });
         if !limited && let Ok(permit) = Arc::clone(&slot.permits).try_acquire_owned() {
            return Ok(SessionLease {
               credentials: Arc::clone(&slot.credentials),
               _permit:     permit,
            });
         }
      }

      // Every usable session is busy, so queue on the next non-limited one.
      let chosen = eligible
         .iter()
         .find(|slot| {
            !limits
               .get(&slot.credentials.id)
               .is_some_and(|session_limits| {
                  session_limits.is_limited(api_for(slot.credentials.kind))
               })
         })
         .copied()
         .ok_or(Error::RateLimited)?;
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
         lim.rejected = false;
         // The session responded, so clear its expired global limit
         if lim.limited && !lim.is_limited(api) {
            lim.limited = false;
         }
         lim.update_limit(api, limit, remaining, reset);
      }
      drop(limits);

      if remaining <= 0 && reset > time::OffsetDateTime::now_utc().unix_timestamp() {
         self.persist_now().await;
      } else {
         self.mark_dirty();
      }
   }

   /// Mark one API as rate limited for a session, leaving its other endpoints
   /// usable. Each endpoint has its own budget, so a 429 on one says nothing
   /// about the rest.
   pub async fn mark_endpoint_limited(&self, session_id: i64, api: &str) {
      let mut limits = self.limits.write().await;

      if let Some(lim) = limits.get_mut(&session_id) {
         lim.limit_endpoint(api);
      }
      drop(limits);
      self.persist_now().await;
   }

   /// Mark a session's credentials as refused, taking it out of rotation until
   /// the tokens are replaced.
   pub async fn mark_rejected(&self, session_id: i64) {
      let mut limits = self.limits.write().await;

      if let Some(lim) = limits.get_mut(&session_id) {
         lim.rejected = true;
      }
   }

   /// Mark a session as globally rate limited.
   pub async fn mark_limited(&self, session_id: i64) {
      let mut limits = self.limits.write().await;

      if let Some(lim) = limits.get_mut(&session_id) {
         lim.limited = true;
         lim.limited_at = time::OffsetDateTime::now_utc().unix_timestamp();
      }
      drop(limits);
      self.persist_now().await;
   }

   /// Get session count.
   pub const fn len(&self) -> usize {
      self.sessions.len()
   }

   /// Check if pool is empty.
   pub const fn is_empty(&self) -> bool {
      self.sessions.is_empty()
   }

   /// Cookie header for a cookie session, for requests that fetch x.com's web
   /// app rather than the API. Takes no concurrency permit, since those pages
   /// are outside the per-session API rate limits.
   pub(crate) fn cookie_header(&self) -> Option<String> {
      self
         .sessions
         .iter()
         .map(|slot| &slot.credentials)
         .find(|creds| {
            creds.kind == SessionKind::Cookie
               && !creds.auth_token.is_empty()
               && !creds.ct0.is_empty()
         })
         .map(|creds| format!("auth_token={}; ct0={}", creds.auth_token, creds.ct0))
   }

   /// Get health statistics about the session pool.
   #[expect(
      clippy::iter_over_hash_type,
      reason = "iteration order irrelevant for aggregation"
   )]
   pub async fn get_health(&self) -> HealthResponse {
      let limits = self.limits.read().await;

      let mut limited_count = 0;
      let mut rejected_count = 0;
      let mut total_requests = 0;
      let mut by_api = HashMap::<String, i32>::new();

      for lim in limits.values() {
         // Counted once by effective state, since a session can carry both
         // flags and a global limit expires on its own.
         if lim.rejected {
            rejected_count += 1;
         } else if lim.is_globally_limited() {
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
            rejected:  rejected_count,
            // Endpoint budgets are per API, so this counts only sessions that
            // are unusable for every endpoint.
            available: self
               .sessions
               .len()
               .saturating_sub(limited_count + rejected_count),
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
               rejected:   lim.is_some_and(|sl| sl.rejected),
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
   let mut all_params = params.to_vec();
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
