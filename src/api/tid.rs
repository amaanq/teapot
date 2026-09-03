//! Transaction ID generation for Twitter/X API requests.
//!
//! Uses the `xitter-txid` crate to generate client transaction IDs
//! matching what the X web app sends. This is required for cookie-based
//! sessions to get full API responses (e.g., conversation-grouped entries
//! in `UserTweetsAndReplies`).

#![expect(
   clippy::module_name_repetitions,
   reason = "the module is the namespace and the prefix names the domain"
)]
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
use xitter_txid::transaction::ClientTransaction;

use super::{
   auth::SessionPool,
   endpoints,
   http::HttpClient,
};

/// Cached transaction ID client that refreshes periodically.
#[derive(Clone)]
pub struct TidClient {
   inner:      Arc<RwLock<Option<ClientTransaction>>>,
   http:       HttpClient,
   sessions:   SessionPool,
   last_fetch: Arc<Mutex<Instant>>,
}

/// How often to refresh the TID client.
const REFRESH_INTERVAL: Duration = Duration::from_hours(1);

/// How long to wait before retrying a failed bootstrap.
const RETRY_INTERVAL: Duration = Duration::from_mins(5);

impl TidClient {
   pub fn new(http: HttpClient, sessions: SessionPool) -> Self {
      Self {
         inner: Arc::new(RwLock::new(None)),
         http,
         sessions,
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
            // Back off, or a persistent failure re-bootstraps on every request.
            *last = Instant::now()
               .checked_sub(REFRESH_INTERVAL.saturating_sub(RETRY_INTERVAL))
               .unwrap_or_else(Instant::now);
         },
      }
   }

   /// Fetch the x.com homepage and ondemand JS to create a new
   /// [`ClientTransaction`].
   async fn fetch_client(&self) -> Result<ClientTransaction, String> {
      let mut headers = HeaderMap::new();
      headers.insert(header::USER_AGENT, endpoints::USER_AGENT.parse().unwrap());
      headers.insert(
         header::ACCEPT,
         header::HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
         ),
      );
      headers.insert(
         "sec-fetch-dest",
         header::HeaderValue::from_static("document"),
      );
      headers.insert(
         "sec-fetch-mode",
         header::HeaderValue::from_static("navigate"),
      );
      headers.insert("sec-fetch-site", header::HeaderValue::from_static("none"));

      // Logged-out visitors get a stripped shell built from a different bundle
      // that carries no chunk manifest, so the bootstrap needs a session cookie
      // to reach the client-web app the transaction ID is derived from.
      let cookie = self
         .sessions
         .cookie_header()
         .ok_or("no cookie session available for TID bootstrap")?;
      headers.insert(
         header::COOKIE,
         cookie
            .parse()
            .map_err(|_| "invalid cookie header value".to_owned())?,
      );

      let home_html = self
         .http
         .get_with_headers("https://x.com", &headers)
         .await
         .map_err(|err| format!("fetch x.com: {err}"))?
         .text()
         .await
         .map_err(|err| format!("read x.com body: {err}"))?;

      let js_url = ClientTransaction::extract_ondemand_url(&home_html)
         .map_err(|err| format!("extract ondemand URL: {err}"))?;

      let js_text = self
         .http
         .get_with_headers(&js_url, &headers)
         .await
         .map_err(|err| format!("fetch ondemand JS: {err}"))?
         .text()
         .await
         .map_err(|err| format!("read ondemand JS body: {err}"))?;

      ClientTransaction::new(&home_html, &js_text)
         .map_err(|err| format!("create TID client: {err}"))
   }
}
