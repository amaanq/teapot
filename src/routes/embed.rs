use axum::{
   Router,
   extract::{
      Path,
      Query,
      State,
   },
   http::{
      HeaderMap,
      header,
   },
   response::{
      Html,
      IntoResponse as _,
      Json,
      Redirect,
      Response,
   },
   routing::get,
};
use serde::Deserialize;

use crate::{
   AppState,
   error::{
      Error,
      Result,
   },
   utils::formatters::format_relative_time,
   views::{
      embed as embed_view,
      layout::strip_html,
   },
};

#[derive(Debug, Deserialize)]
pub struct OEmbedQuery {
   pub text:   Option<String>,
   pub author: Option<String>,
   pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StandardOEmbedQuery {
   pub url:      String,
   #[serde(default)]
   pub maxwidth: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct LegacyEmbedQuery {
   pub id: Option<String>,
}

pub fn router() -> Router<AppState> {
   Router::new()
        .route("/{username}/status/{id}/embed", get(tweet_embed))
        .route("/i/videos/tweet/{id}", get(video_embed))
        // Legacy embed URL support (Twitter's old embed format)
        .route("/embed/Tweet.html", get(legacy_embed_redirect))
        // ActivityPub endpoint for Discord multi-image support
        .route("/users/{username}/statuses/{id}", get(activity_pub_status))
        // oEmbed endpoints
        .route("/owoembed", get(oembed))
        .route("/oembed", get(oembed_standard))
}

/// Redirect legacy `/embed/Tweet.html?id=XXX` to `/i/status/XXX/embed`.
async fn legacy_embed_redirect(Query(query): Query<LegacyEmbedQuery>) -> Response {
   query.id.map_or_else(
      || Redirect::to("/").into_response(),
      |id| Redirect::to(&format!("/i/status/{id}/embed")).into_response(),
   )
}

/// Tweet embed endpoint - returns HTML with proper OG meta tags for
/// Discord/etc.
async fn tweet_embed(
   State(state): State<AppState>,
   Path((_username, id)): Path<(String, String)>,
) -> Result<Response> {
   // Fetch tweet from API
   let tweet = state.api.get_tweet(&id).await?;

   let markup = embed_view::render_tweet_embed(&tweet, &state.config);
   Ok(Html(markup.into_string()).into_response())
}

/// Video embed endpoint - returns HTML with video player.
async fn video_embed(State(state): State<AppState>, Path(id): Path<String>) -> Result<Response> {
   // Fetch tweet from API
   let tweet = state.api.get_tweet(&id).await?;

   if tweet.video.is_none() && tweet.gif.is_none() {
      return Err(Error::NotFound("No video found".into()));
   }

   let markup = embed_view::render_video_embed(&tweet, &state.config);
   Ok(Html(markup.into_string()).into_response())
}

/// `ActivityPub` JSON endpoint for Discord multi-image support.
/// Discord fetches this to get all media attachments for carousel display.
async fn activity_pub_status(
   State(state): State<AppState>,
   Path((username, id)): Path<(String, String)>,
   headers: HeaderMap,
) -> Result<Response> {
   let accept = headers
      .get(header::ACCEPT)
      .and_then(|hv| hv.to_str().ok())
      .unwrap_or("");

   let user_agent = headers
      .get(header::USER_AGENT)
      .and_then(|hv| hv.to_str().ok())
      .unwrap_or("");

   // Serve Mastodon-format JSON for Discord (doesn't send Accept header)
   // and for explicit ActivityPub requests. Discord uses this for the
   // footer timestamp and rich content formatting.
   let is_discord = user_agent.contains("Discordbot");
   if is_discord
      || accept.contains("application/activity+json")
      || accept.contains("application/ld+json")
   {
      // Fetch tweet from API
      let tweet = state.api.get_tweet(&id).await?;

      // Build ActivityPub JSON
      let activity = embed_view::build_activity_pub(&tweet, &state.config);

      return Ok((
         [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
         Json(activity),
      )
         .into_response());
   }

   // Otherwise redirect to normal status page
   Ok(Redirect::to(&format!("/{username}/status/{id}")).into_response())
}

/// oEmbed JSON endpoint for engagement metrics in Discord embed footers.
async fn oembed(
   State(state): State<AppState>,
   Query(query): Query<OEmbedQuery>,
) -> Result<Response> {
   #[derive(serde::Serialize)]
   struct OEmbedData {
      author_name:   String,
      author_url:    String,
      provider_name: String,
      provider_url:  String,
      title:         &'static str,
      #[serde(rename = "type")]
      kind:          &'static str,
      version:       &'static str,
   }

   let text = query.text.unwrap_or_default();
   let author = query.author.unwrap_or_default();
   let status = query.status.unwrap_or_default();

   let author_url = format!("https://x.com/{author}/status/{status}");

   let oembed = OEmbedData {
      author_name: text,
      author_url,
      provider_name: state.config.server.title.clone(),
      provider_url: state.config.url_prefix().to_owned(),
      title: "Post",
      kind: "rich",
      version: "1.0",
   };

   Ok((
      [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
      Json(oembed),
   )
      .into_response())
}

#[derive(serde::Serialize)]
struct StandardOEmbed {
   version:       &'static str,
   #[serde(rename = "type")]
   kind:          &'static str,
   author_name:   String,
   author_url:    String,
   provider_name: String,
   provider_url:  String,
   html:          String,
   width:         u32,
   #[serde(skip_serializing_if = "Option::is_none")]
   height:        Option<u32>,
   cache_age:     u32,
}

/// Standard oEmbed endpoint: `/oembed?url=https://teapot.example.com/user/status/123`.
///
/// Parses the tweet ID from the URL, fetches the tweet, and returns a full
/// oEmbed response per <https://oembed.com>.
async fn oembed_standard(
   State(state): State<AppState>,
   Query(query): Query<StandardOEmbedQuery>,
) -> Result<Response> {
   // Parse tweet ID from URL: /{username}/status/{id}
   let tweet_id = extract_tweet_id(&query.url)
      .ok_or_else(|| Error::InvalidUrl("Could not parse tweet URL".into()))?;

   let tweet = state.api.get_tweet(tweet_id).await?;
   let url_prefix = state.config.url_prefix();

   let author_name = format!("{} (@{})", tweet.user.fullname, tweet.user.username);
   let author_url = format!("{url_prefix}/{}", tweet.user.username);
   let tweet_text = strip_html(&tweet.text);

   // Build a minimal HTML embed
   let html = format!(
      r#"<blockquote><p>{}</p>&mdash; {} <a href="{url_prefix}/{}/status/{}">{}</a></blockquote>"#,
      tweet_text,
      author_name,
      tweet.user.username,
      tweet.id,
      tweet.time.map(format_relative_time).unwrap_or_default(),
   );

   let data = StandardOEmbed {
      version: "1.0",
      kind: "rich",
      author_name,
      author_url,
      provider_name: state.config.server.title.clone(),
      provider_url: url_prefix.to_owned(),
      html,
      width: query.maxwidth.unwrap_or(550),
      height: None,
      cache_age: 3600,
   };

   Ok((
      [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
      Json(data),
   )
      .into_response())
}

/// Extract tweet ID from a URL path like `/user/status/123` or full URL.
fn extract_tweet_id(url: &str) -> Option<&str> {
   // Try path segments: look for "status" or "statuses" followed by the ID
   let path = url
      .strip_prefix("http://")
      .or_else(|| url.strip_prefix("https://"))
      .map_or(url, |stripped| {
         stripped.split_once('/').map_or("", |(_, rest)| rest)
      });

   let segments: Vec<&str> = path.split('/').collect();
   for window in segments.windows(2) {
      assert!(window.len() > 1, "windows(2) guarantees length 2");
      if (window[0] == "status" || window[0] == "statuses") && !window[1].is_empty() {
         let id = window[1].split('?').next().unwrap_or(window[1]);
         if id.chars().all(|ch| ch.is_ascii_digit()) {
            return Some(id);
         }
      }
   }
   None
}
