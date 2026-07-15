pub mod gif_cache;

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

pub use gif_cache::GifCache;

/// Cache entry holding a type-erased value and its expiry.
struct Entry {
   value:   Arc<dyn Any + Send + Sync>,
   expires: Instant,
}

/// In-process cache with TTL-based expiry and a hard entry-count cap.
///
/// Uses `std::sync::RwLock` (not tokio) because the lock is never held across
/// `.await`. All operations are plain `HashMap` lookups or inserts.
/// Stores values as `Arc<dyn Any>` to avoid JSON serialization overhead.
///
/// When an insert exceeds `max_entries`, expired entries are purged first. If
/// the map remains over capacity, the entries with the soonest expiry are
/// dropped until the map is back to 75% of `max_entries`. This approximates
/// LRU for workloads with uniform TTLs without mutating on `get`.
#[derive(Clone)]
pub struct Cache {
   inner:       Arc<RwLock<HashMap<String, Entry>>>,
   max_entries: usize,
}

impl Cache {
   pub fn new(max_entries: usize) -> Self {
      Self {
         inner:       Arc::new(RwLock::new(HashMap::new())),
         max_entries: max_entries.max(16),
      }
   }

   /// Get a value from cache, returning `None` if missing, expired, or
   /// type-mismatched.
   #[expect(clippy::significant_drop_tightening, reason = "entry borrows from map")]
   pub fn get<T>(&self, key: &str) -> Option<T>
   where
      T: Any + Send + Sync + Clone,
   {
      let map = self.inner.read().ok()?;
      let entry = map.get(key)?;
      if entry.expires <= Instant::now() {
         return None;
      }
      entry.value.downcast_ref::<T>().cloned()
   }

   /// Set a value in cache with TTL in seconds.
   pub fn set<T>(&self, cache_key: &str, value: &T, ttl_seconds: u64)
   where
      T: Any + Send + Sync + Clone,
   {
      let cache_entry = Entry {
         value:   Arc::new(value.clone()),
         expires: Instant::now() + Duration::from_secs(ttl_seconds),
      };
      if let Ok(mut map) = self.inner.write() {
         map.insert(cache_key.to_owned(), cache_entry);

         if map.len() > self.max_entries {
            let now = Instant::now();
            map.retain(|_, cached| cached.expires > now);

            if map.len() > self.max_entries {
               let target = self.max_entries * 3 / 4;
               let drop_n = map.len() - target;
               let mut by_expiry: Vec<(Instant, String)> = map
                  .iter()
                  .map(|(stored_key, stored_entry)| (stored_entry.expires, stored_key.clone()))
                  .collect();
               by_expiry.select_nth_unstable_by_key(drop_n, |&(expiry, _)| expiry);
               for (_, eviction_key) in by_expiry.into_iter().take(drop_n) {
                  map.remove(&eviction_key);
               }
            }
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
pub mod keys;
/// Cache TTL constants (in seconds).
pub mod ttl;
