use axum::{
   Router,
   body::Body,
   extract::{
      Path,
      State,
   },
   http::{
      StatusCode,
      header,
   },
   response::{
      IntoResponse as _,
      Response,
   },
   routing::get,
};

use crate::{
   AppState,
   error::{
      Error,
      Result,
   },
   utils::{
      formatters,
      hmac,
   },
};

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/pic/{url}", get(pic_proxy))
      .route("/pic/enc/{url}", get(pic_proxy_encoded))
      .route("/pic/orig/{url}", get(pic_orig_proxy))
      .route("/pic/orig/enc/{url}", get(pic_orig_proxy_encoded))
      .route("/video/{sig}/{url}", get(video_proxy))
      .route("/video/enc/{sig}/{url}", get(video_proxy_encoded))
}

async fn pic_proxy(State(state): State<AppState>, Path(url): Path<String>) -> Result<Response> {
   // Reject amplify_video URLs to prevent video thumbnails from being matched
   if url.contains("/amplify_video/") {
      return Err(Error::InvalidUrl("Not an image URL".into()));
   }
   proxy_image(&state, &url, false).await
}

async fn pic_proxy_encoded(
   State(state): State<AppState>,
   Path(url): Path<String>,
) -> Result<Response> {
   let decoded = formatters::base64_decode_url(&url)
      .ok_or_else(|| Error::InvalidUrl("Invalid base64 encoding".into()))?;
   if decoded.contains("/amplify_video/") {
      return Err(Error::InvalidUrl("Not an image URL".into()));
   }
   proxy_image(&state, &decoded, false).await
}

async fn pic_orig_proxy(
   State(state): State<AppState>,
   Path(url): Path<String>,
) -> Result<Response> {
   if url.contains("/amplify_video/") {
      return Err(Error::InvalidUrl("Not an image URL".into()));
   }
   proxy_image(&state, &url, true).await
}

async fn pic_orig_proxy_encoded(
   State(state): State<AppState>,
   Path(url): Path<String>,
) -> Result<Response> {
   let decoded = formatters::base64_decode_url(&url)
      .ok_or_else(|| Error::InvalidUrl("Invalid base64 encoding".into()))?;
   if decoded.contains("/amplify_video/") {
      return Err(Error::InvalidUrl("Not an image URL".into()));
   }
   proxy_image(&state, &decoded, true).await
}

async fn video_proxy(
   State(state): State<AppState>,
   Path((sig, url)): Path<(String, String)>,
) -> Result<Response> {
   // URL-decode the URL first
   let decoded_url = percent_encoding::percent_decode_str(&url)
      .decode_utf8()
      .map_err(|_| Error::InvalidUrl("Invalid URL encoding".into()))?
      .to_string();

   // Verify HMAC signature using utility function
   if !hmac::verify(&decoded_url, &sig, &state.config.config.hmac_key) {
      return Err(Error::HmacVerification);
   }

   proxy_video(&state, &decoded_url).await
}

async fn video_proxy_encoded(
   State(state): State<AppState>,
   Path((sig, url)): Path<(String, String)>,
) -> Result<Response> {
   let decoded = formatters::base64_decode_url(&url)
      .ok_or_else(|| Error::InvalidUrl("Invalid base64 encoding".into()))?;

   // Verify HMAC signature using utility function
   if !hmac::verify(&decoded, &sig, &state.config.config.hmac_key) {
      return Err(Error::HmacVerification);
   }

   proxy_video(&state, &decoded).await
}

async fn proxy_image(state: &AppState, url: &str, original: bool) -> Result<Response> {
   if url.is_empty() || url == "/" {
      return Err(Error::InvalidUrl("Empty image URL".into()));
   }

   let full_url = if url.starts_with("http") {
      url.to_owned()
   } else {
      format!("https://pbs.twimg.com{url}")
   };

   let full_url = if original {
      format!("{full_url}?name=orig")
   } else {
      full_url
   };

   let response = state.http_client.get(&full_url).await?;

   let content_type = response
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|hv| hv.to_str().ok())
      .unwrap_or("image/jpeg")
      .to_owned();

   let bytes = response.bytes().await?;

   Ok((
      StatusCode::OK,
      [
         (header::CONTENT_TYPE, content_type),
         (header::CACHE_CONTROL, "max-age=604800".to_owned()),
      ],
      bytes,
   )
      .into_response())
}

async fn proxy_video(state: &AppState, url: &str) -> Result<Response> {
   let response = state.http_client.get(url).await?;

   if !response.status().is_success() {
      return Err(Error::InvalidUrl(format!(
         "Video fetch failed: {}",
         response.status()
      )));
   }

   let content_type = response
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|hv| hv.to_str().ok())
      .unwrap_or("video/mp4")
      .to_owned();

   let bytes = response.bytes().await?;

   Ok((
      StatusCode::OK,
      [
         (header::CONTENT_TYPE, content_type),
         (header::CACHE_CONTROL, "max-age=604800".to_owned()),
      ],
      bytes,
   )
      .into_response())
}
