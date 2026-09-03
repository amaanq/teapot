//! The in-process cache itself.

use std::{
   any::Any,
   collections::{
      HashMap,
      HashSet,
   },
   sync::{
      Arc,
      Mutex,
      RwLock,
   },
   time::{
      Duration,
      Instant,
   },
};

/// Cache entry holding a type-erased value and its expiry.
struct Entry {
   value:       Arc<dyn Any + Send + Sync>,
   fresh_until: Instant,
   stale_until: Instant,
}

/// A lookup that may be served after its fresh window has closed.
pub enum Hit<T> {
   Fresh(T),
   Stale(T),
}

/// Clears the in-flight mark when the refresher finishes or panics.
pub struct RefreshGuard {
   cache: Cache,
   key:   String,
}

impl Drop for RefreshGuard {
   fn drop(&mut self) {
      self.cache.finish_refresh(&self.key);
   }
}

/// In-process cache with TTL-based expiry and a hard entry-count cap.
///
/// When an insert exceeds `max_entries`, expired entries are purged first. If
/// the map remains over capacity, the entries with the soonest expiry are
/// dropped until the map is back to 75% of `max_entries`. This approximates
/// LRU for workloads with uniform TTLs without mutating on `get`.
#[derive(Clone)]
pub struct Cache {
   inner:       Arc<RwLock<HashMap<String, Entry>>>,
   inflight:    Arc<Mutex<HashSet<String>>>,
   max_entries: usize,
}

impl Cache {
   pub fn new(max_entries: usize) -> Self {
      Self {
         inner:       Arc::new(RwLock::new(HashMap::new())),
         inflight:    Arc::new(Mutex::new(HashSet::new())),
         max_entries: max_entries.max(16),
      }
   }

   /// Get a value from cache, returning [`None`] if missing, expired, or
   /// type-mismatched.
   pub fn get<T>(&self, key: &str) -> Option<T>
   where
      T: Any + Send + Sync + Clone,
   {
      match self.lookup(key) {
         Some(Hit::Fresh(value)) => Some(value),
         _ => None,
      }
   }

   /// Fresh or stale value. Stale is only possible for keys stored with
   /// [`set_swr`](Self::set_swr).
   #[expect(clippy::significant_drop_tightening, reason = "entry borrows from map")]
   pub fn lookup<T>(&self, key: &str) -> Option<Hit<T>>
   where
      T: Any + Send + Sync + Clone,
   {
      let map = self.inner.read().ok()?;
      let entry = map.get(key)?;
      let now = Instant::now();
      if entry.stale_until <= now {
         return None;
      }
      let value = entry.value.downcast_ref::<T>()?.clone();
      if entry.fresh_until > now {
         Some(Hit::Fresh(value))
      } else {
         Some(Hit::Stale(value))
      }
   }

   /// Set a value in cache with TTL in seconds.
   pub fn set<T>(&self, cache_key: &str, value: &T, ttl_seconds: u64)
   where
      T: Any + Send + Sync + Clone,
   {
      self.insert(cache_key, value, ttl_seconds, 0);
   }

   /// Fresh for `fresh_seconds`, then served stale for `stale_seconds` more.
   pub fn set_swr<T>(&self, cache_key: &str, value: &T, fresh_seconds: u64, stale_seconds: u64)
   where
      T: Any + Send + Sync + Clone,
   {
      self.insert(cache_key, value, fresh_seconds, stale_seconds);
   }

   /// Mark `key` as refreshing, or [`None`] if a refresh is already running.
   pub fn start_refresh(&self, key: &str) -> Option<RefreshGuard> {
      let inserted = self.inflight.lock().ok()?.insert(key.to_owned());
      inserted.then(|| {
         RefreshGuard {
            cache: self.clone(),
            key:   key.to_owned(),
         }
      })
   }

   fn finish_refresh(&self, key: &str) {
      if let Ok(mut inflight) = self.inflight.lock() {
         inflight.remove(key);
      }
   }

   fn insert<T>(&self, cache_key: &str, value: &T, fresh_seconds: u64, stale_seconds: u64)
   where
      T: Any + Send + Sync + Clone,
   {
      let now = Instant::now();
      let fresh_until = now + Duration::from_secs(fresh_seconds);
      let cache_entry = Entry {
         value: Arc::new(value.clone()),
         fresh_until,
         stale_until: fresh_until + Duration::from_secs(stale_seconds),
      };
      if let Ok(mut map) = self.inner.write() {
         map.insert(cache_key.to_owned(), cache_entry);

         if map.len() > self.max_entries {
            map.retain(|_, cached| cached.stale_until > now);

            if map.len() > self.max_entries {
               let target = self.max_entries * 3 / 4;
               let drop_n = map.len() - target;
               let mut by_expiry: Vec<(Instant, String)> = map
                  .iter()
                  .map(|(stored_key, stored_entry)| (stored_entry.stale_until, stored_key.clone()))
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
