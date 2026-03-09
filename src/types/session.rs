use std::collections::HashMap;

use serde::{
   Deserialize,
   Serialize,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimit {
   pub limit:     i32,
   pub remaining: i32,
   pub reset:     i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
   #[default]
   OAuth,
   Cookie,
}

/// Immutable session credentials, loaded once and `Arc`-shared.
#[derive(Debug, Clone, Serialize)]
pub struct SessionCredentials {
   pub id:           i64,
   pub username:     String,
   pub kind:         SessionKind,
   pub oauth_token:  String,
   pub oauth_secret: String,
   pub auth_token:   String,
   pub ct0:          String,
}

/// Mutable rate-limit state, stored separately in the pool.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionLimits {
   pub pending:    i32,
   pub limited:    bool,
   pub limited_at: i64,
   pub apis:       HashMap<String, RateLimit>,
}

/// How long a globally-limited session stays limited before auto-recovery (15
/// min).
const GLOBAL_LIMIT_DURATION_SECS: i64 = 15 * 60;

impl SessionLimits {
   /// Check if rate limited for a specific API.
   pub fn is_limited(&self, api: &str) -> bool {
      if self.limited {
         let now = time::OffsetDateTime::now_utc().unix_timestamp();
         if now - self.limited_at < GLOBAL_LIMIT_DURATION_SECS {
            return true;
         }
         // Expired — will be cleared on next mutable access
      }
      if let Some(limit) = self.apis.get(api) {
         let now = time::OffsetDateTime::now_utc().unix_timestamp();
         if limit.remaining == 0 && limit.reset > now {
            return true;
         }
      }
      false
   }

   pub fn update_limit(&mut self, api: &str, limit: i32, remaining: i32, reset: i64) {
      self.apis.insert(api.to_owned(), RateLimit {
         limit,
         remaining,
         reset,
      });
   }
}

/// Authentication session for Twitter API (used for JSONL deserialization).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
   pub id:         i64,
   pub username:   String,
   pub pending:    i32,
   pub limited:    bool,
   pub limited_at: i64,
   pub apis:       HashMap<String, RateLimit>,
   pub kind:       SessionKind,

   // OAuth credentials
   pub oauth_token:  String,
   pub oauth_secret: String,

   // Cookie credentials
   pub auth_token: String,
   pub ct0:        String,
}

impl Session {
   /// Split into immutable credentials and mutable rate-limit state.
   pub fn into_credentials_and_limits(self) -> (SessionCredentials, SessionLimits) {
      (
         SessionCredentials {
            id:           self.id,
            username:     self.username,
            kind:         self.kind,
            oauth_token:  self.oauth_token,
            oauth_secret: self.oauth_secret,
            auth_token:   self.auth_token,
            ct0:          self.ct0,
         },
         SessionLimits {
            pending:    self.pending,
            limited:    self.limited,
            limited_at: self.limited_at,
            apis:       self.apis,
         },
      )
   }
}
