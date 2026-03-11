pub mod gif_cache;

pub use gif_cache::GifCache;

use std::{
   any::Any,
   collections::HashMap,
   sync::{
      Arc,
      RwLock,
   },
   time::{
      Duration,
      Instant,
   },
};

/// Entry in the cache: type-erased value + expiry.
struct Entry {
   value:   Arc<dyn Any + Send + Sync>,
   expires: Instant,
}

/// In-process cache with TTL-based expiry.
///
/// Uses `std::sync::RwLock` (not tokio) because the lock is never held across
/// `.await` — all operations are plain `HashMap` lookups/inserts.
/// Stores values as `Arc<dyn Any>` to avoid JSON serialization overhead.
#[derive(Clone)]
pub struct Cache {
   inner: Arc<RwLock<HashMap<String, Entry>>>,
}

impl Cache {
   pub fn new() -> Self {
      Self {
         inner: Arc::new(RwLock::new(HashMap::new())),
      }
   }

   /// Get a value from cache, returning `None` if missing, expired, or
   /// type-mismatched.
   #[expect(clippy::significant_drop_tightening, reason = "entry borrows from map")]
   pub fn get<T: Any + Send + Sync + Clone>(&self, key: &str) -> Option<T> {
      let map = self.inner.read().ok()?;
      let entry = map.get(key)?;
      if entry.expires <= Instant::now() {
         return None;
      }
      entry.value.downcast_ref::<T>().cloned()
   }

   /// Set a value in cache with TTL in seconds.
   pub fn set<T: Any + Send + Sync + Clone>(&self, key: &str, value: &T, ttl_seconds: u64) {
      let entry = Entry {
         value:   Arc::new(value.clone()),
         expires: Instant::now() + Duration::from_secs(ttl_seconds),
      };
      if let Ok(mut map) = self.inner.write() {
         map.insert(key.to_owned(), entry);

         // Lazy eviction: purge expired entries when map grows large
         if map.len() > 4096 {
            let now = Instant::now();
            map.retain(|_, cached| cached.expires > now);
         }
      }
   }

   /// Delete a key from cache.
   pub fn delete(&self, key: &str) {
      if let Ok(mut map) = self.inner.write() {
         map.remove(key);
      }
   }
}

/// Cache key builders.
pub mod keys {
   pub fn user(username: &str) -> String {
      format!("u:{}", username.to_lowercase())
   }

   pub fn profile(username: &str) -> String {
      format!("p:{}", username.to_lowercase())
   }

   pub fn timeline(username: &str, kind: &str) -> String {
      format!("tl:{}:{kind}", username.to_lowercase())
   }

   pub fn list(id: &str) -> String {
      format!("l:{id}")
   }

   pub fn list_members(id: &str) -> String {
      format!("lm:{id}")
   }

   pub fn conversation(id: &str) -> String {
      format!("conv:{id}")
   }

   pub fn rss(key: &str) -> String {
      format!("rss:{key}")
   }

   /// User ID to username mapping.
   pub fn user_id(id: &str) -> String {
      format!("uid:{id}")
   }

   pub fn rss_user(username: &str) -> String {
      rss(&format!("user:{}", username.to_lowercase()))
   }

   pub fn rss_replies(username: &str) -> String {
      rss(&format!("replies:{}", username.to_lowercase()))
   }

   pub fn rss_media(username: &str) -> String {
      rss(&format!("media:{}", username.to_lowercase()))
   }

   pub fn rss_search(query: &str) -> String {
      rss(&format!("search:{}", query.to_lowercase()))
   }

   pub fn rss_user_search(username: &str, query: &str) -> String {
      rss(&format!(
         "usersearch:{}:{}",
         username.to_lowercase(),
         query.to_lowercase()
      ))
   }

   pub fn rss_list(id: &str) -> String {
      rss(&format!("list:{id}"))
   }

   pub fn rss_list_slug(username: &str, slug: &str) -> String {
      rss(&format!("listslug:{}:{slug}", username.to_lowercase()))
   }

   pub fn rss_thread(tweet_id: &str) -> String {
      rss(&format!("thread:{tweet_id}"))
   }
}

/// Cache TTL constants (in seconds).
pub mod ttl {
   /// Standard TTL for user/profile/tweet data (5 minutes).
   pub const DEFAULT: u64 = 300;
   /// Long TTL for immutable mappings like user ID -> username (24 hours).
   pub const USER_ID_MAPPING: u64 = 86400;
}
