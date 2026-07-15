use std::{
   collections::VecDeque,
   time::Duration,
};

use tokio::{
   sync::{
      Mutex,
      Semaphore,
      SemaphorePermit,
   },
   time::Instant,
};

use crate::error::{
   Error,
   Result,
};

const MAX_TRANSLATIONS_PER_WINDOW: usize = 60;
const WINDOW: Duration = Duration::from_secs(60);
const MAX_CONCURRENT_TRANSLATIONS: usize = 2;

/// Global guard for translation backends.
pub struct TranslationLimiter {
   recent:     Mutex<VecDeque<Instant>>,
   concurrent: Semaphore,
}

impl TranslationLimiter {
   pub fn new() -> Self {
      Self {
         recent:     Mutex::new(VecDeque::new()),
         concurrent: Semaphore::new(MAX_CONCURRENT_TRANSLATIONS),
      }
   }

   pub async fn acquire(&self) -> Result<SemaphorePermit<'_>> {
      let now = Instant::now();
      let mut recent = self.recent.lock().await;
      while recent
         .front()
         .is_some_and(|started| now.duration_since(*started) >= WINDOW)
      {
         recent.pop_front();
      }
      if recent.len() >= MAX_TRANSLATIONS_PER_WINDOW {
         return Err(Error::RateLimited);
      }
      recent.push_back(now);
      drop(recent);

      self
         .concurrent
         .acquire()
         .await
         .map_err(|_| Error::Internal("translation limiter closed".into()))
   }
}
