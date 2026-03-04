use axum::{
   Router,
   extract::{
      Path,
      Query,
      State,
   },
   response::{
      IntoResponse as _,
      Redirect,
      Response,
   },
   routing::get,
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::{
   AppState,
   config::Config,
   error::{
      Error,
      Result,
   },
   types::Prefs,
};

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/i/redirect", get(redirect_handler))
      .route("/t.co/{url}", get(tco_resolver))
      .route("/cards/{card}/{id}", get(card_resolver))
}

#[derive(Debug, Deserialize)]
pub struct RedirectQuery {
   pub url: Option<String>,
}

/// Handle `/i/redirect?url=...` - decode and transform URL before redirecting.
async fn redirect_handler(
   State(state): State<AppState>,
   Query(query): Query<RedirectQuery>,
   jar: CookieJar,
) -> Result<Response> {
   let url = query.url.as_deref().unwrap_or("");

   if url.is_empty() {
      return Err(Error::NotFound("No URL provided".into()));
   }

   // Decode URL if needed
   let decoded_url = percent_decode(url);

   // Get user preferences for URL replacements
   let prefs = Prefs::from_cookies(&jar, &state.config);

   // Apply URL transformations based on preferences
   let transformed = apply_url_replacements(&decoded_url, &prefs, &state.config);

   Ok(Redirect::to(&transformed).into_response())
}

/// Resolve `t.co` short URLs.
async fn tco_resolver(
   State(state): State<AppState>,
   Path(url): Path<String>,
   jar: CookieJar,
) -> Result<Response> {
   let full_url = format!("https://t.co/{url}");

   // Resolve the redirect
   let resolved = resolve_url(&state, &full_url).await?;

   // Get user preferences for URL replacements
   let prefs = Prefs::from_cookies(&jar, &state.config);

   // Apply URL transformations
   let transformed = apply_url_replacements(&resolved, &prefs, &state.config);

   Ok(Redirect::to(&transformed).into_response())
}

/// Resolve Twitter card URLs.
async fn card_resolver(
   State(state): State<AppState>,
   Path((card, id)): Path<(String, String)>,
   jar: CookieJar,
) -> Result<Response> {
   let full_url = format!("https://cards.twitter.com/cards/{card}/{id}");

   // Resolve the redirect
   let resolved = resolve_url(&state, &full_url).await?;

   // Get user preferences for URL replacements
   let prefs = Prefs::from_cookies(&jar, &state.config);

   // Apply URL transformations
   let transformed = apply_url_replacements(&resolved, &prefs, &state.config);

   Ok(Redirect::to(&transformed).into_response())
}

/// Resolve a URL by following redirects and returning the final Location
/// header.
///
/// hyper doesn't follow redirects by default (unlike reqwest), so a simple HEAD
/// request gives us the redirect Location directly.
async fn resolve_url(state: &AppState, url: &str) -> Result<String> {
   let response = state
      .http_client
      .head(url)
      .await
      .map_err(|err| Error::Internal(format!("Failed to resolve URL: {err}")))?;

   // Get Location header for redirect
   if let Some(location) = response.headers().get("location") {
      let resolved = location
         .to_str()
         .map_err(|_| Error::Internal("Invalid location header".into()))?
         .to_owned();
      Ok(resolved)
   } else {
      // No redirect, return original URL
      Ok(url.to_owned())
   }
}

/// Apply URL replacements based on user preferences.
fn apply_url_replacements(url: &str, prefs: &Prefs, config: &Config) -> String {
   let mut result = url.to_owned();

   // Replace Twitter URLs — swap full origin so scheme+port are correct
   let twitter_prefix = if prefs.replace_twitter.is_empty() {
      config.url_prefix().to_owned()
   } else {
      format!("https://{}", prefs.replace_twitter.trim_end_matches('/'))
   };

   for origin in [
      "https://mobile.twitter.com",
      "https://twitter.com",
      "https://x.com",
   ] {
      result = result.replace(origin, &twitter_prefix);
   }

   // Replace YouTube URLs if configured
   if !prefs.replace_youtube.is_empty() {
      result = result
         .replace("www.youtube.com", &prefs.replace_youtube)
         .replace("youtube.com", &prefs.replace_youtube)
         .replace("youtu.be", &prefs.replace_youtube);
   }

   // Replace Reddit URLs if configured
   if !prefs.replace_reddit.is_empty() {
      result = result
         .replace("old.reddit.com", &prefs.replace_reddit)
         .replace("www.reddit.com", &prefs.replace_reddit)
         .replace("reddit.com", &prefs.replace_reddit);
   }

   result
}

/// Percent-decode a URL string.
fn percent_decode(url: &str) -> String {
   percent_encoding::percent_decode_str(url)
      .decode_utf8()
      .unwrap_or_else(|_| url.into())
      .into_owned()
}
