//! Transaction ID generation for Twitter/X API requests.
//!
//! Uses the `xitter-txid` crate to generate client transaction IDs
//! matching what the X web app sends. This is required for cookie-based
//! sessions to get full API responses (e.g., conversation-grouped entries
//! in `UserTweetsAndReplies`).

use std::{
   sync::Arc,
   time::{
      Duration,
      Instant,
   },
};

use axum::http::{
   HeaderMap,
   header,
};
use tokio::sync::{
   Mutex,
   RwLock,
};
use xitter_txid::ClientTransaction;

use super::{
   endpoints,
   http::HttpClient,
};

/// Cached transaction ID client that refreshes periodically.
#[derive(Clone)]
pub struct TidClient {
   inner:      Arc<RwLock<Option<ClientTransaction>>>,
   http:       HttpClient,
   last_fetch: Arc<Mutex<Instant>>,
}

/// How often to refresh the TID client.
const REFRESH_INTERVAL: Duration = Duration::from_hours(1);

impl TidClient {
   pub fn new(http: HttpClient) -> Self {
      Self {
         inner: Arc::new(RwLock::new(None)),
         http,
         last_fetch: Arc::new(Mutex::new(
            Instant::now().checked_sub(REFRESH_INTERVAL).unwrap(),
         )),
      }
   }

   /// Generate a transaction ID for a request path, or [`None`] if TID is
   /// unavailable.
   pub async fn generate(&self, path: &str) -> Option<String> {
      self.ensure_fresh().await;
      let guard = self.inner.read().await;
      guard
         .as_ref()
         .map(|ct| ct.generate_transaction_id("GET", path))
   }

   /// Refresh the TID client if stale. Uses `try_lock` so only one task
   /// performs the refresh. Concurrent callers skip it and use the existing
   /// (possibly stale) client.
   async fn ensure_fresh(&self) {
      let Ok(mut last) = self.last_fetch.try_lock() else {
         return; // another task is already refreshing
      };

      if last.elapsed() < REFRESH_INTERVAL {
         return;
      }

      match self.fetch_client().await {
         Ok(ct) => {
            *self.inner.write().await = Some(ct);
            *last = Instant::now();
            tracing::info!("TID client refreshed");
         },
         Err(err) => {
            tracing::warn!("Failed to refresh TID client: {err}");
         },
      }
   }

   /// Fetch the x.com homepage and ondemand JS to create a new
   /// [`ClientTransaction`].
   async fn fetch_client(&self) -> Result<ClientTransaction, String> {
      let mut ua_header = HeaderMap::new();
      ua_header.insert(header::USER_AGENT, endpoints::USER_AGENT.parse().unwrap());

      let home_html = self
         .http
         .get_with_headers("https://x.com", &ua_header)
         .await
         .map_err(|err| format!("fetch x.com: {err}"))?
         .text()
         .await
         .map_err(|err| format!("read x.com body: {err}"))?;

      let js_url = ClientTransaction::extract_ondemand_url(&home_html)
         .map_err(|err| format!("extract ondemand URL: {err}"))?;

      let js_text = self
         .http
         .get_with_headers(&js_url, &ua_header)
         .await
         .map_err(|err| format!("fetch ondemand JS: {err}"))?
         .text()
         .await
         .map_err(|err| format!("read ondemand JS body: {err}"))?;

      ClientTransaction::new(&home_html, &js_text)
         .map_err(|err| format!("create TID client: {err}"))
   }
}
