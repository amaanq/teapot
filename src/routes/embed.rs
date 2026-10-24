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
      IntoResponse as _,
      Json,
      Redirect,
      Response,
   },
   routing::get,
};
use serde::Deserialize;
use topcoat::{
   context::Cx,
   router::{
      page,
      path_param,
   },
   view::{
      View,
      view,
   },
};

use super::helpers::get_cached_translation;
use crate::{
   AppState,
   cache::{
      keys as cache_keys,
      ttl,
   },
   error::{
      Error,
      Result,
   },
   routes::helpers::{
      state,
      user_agent,
   },
   types::timeline::{
      Conversation,
      RankingMode,
   },
   utils::{
      entity_expander::html_escape,
      formatters::format_relative_time,
   },
   views::{
      document::strip_html,
      embed::{
         has_playable_video,
         tweet_embed,
         video_embed,
      },
      embed_activity,
      embed_activity::ActivityId,
   },
};

path_param!(id);

#[derive(Debug, Deserialize)]
pub struct OEmbedQuery {
   pub text:     Option<String>,
   pub author:   Option<String>,
   pub status:   Option<String>,
   pub provider: Option<String>,
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
        // Legacy embed URL support (Twitter's old embed format)
        .route("/embed/Tweet.html", get(legacy_embed_redirect))
        // ActivityPub endpoint for Discord multi-image support
        .route("/users/{username}/statuses/{status}", get(activity_pub_status))
        // Discord turns the Mastodon-looking discovery URL above into this
        // API request. The discovery URL itself is not the JSON endpoint.
        .route("/api/v1/statuses/{status}", get(mastodon_status))
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
#[page("/{username}/status/{id}/embed")]
pub async fn tweet_embed_page(cx: &Cx) -> topcoat::Result<impl View> {
   let tweet = cached_conversation(state(cx), path_param::<Id>(cx))
      .await?
      .tweet;
   let discord_activity = user_agent(cx).contains("Discordbot");
   Ok(view! {
      tweet_embed(tweet: &tweet, discord_activity: discord_activity)
   })
}

/// Video embed endpoint - returns HTML with video player.
#[page("/i/videos/tweet/{id}")]
pub async fn video_embed_page(cx: &Cx) -> topcoat::Result<impl View> {
   let tweet = cached_conversation(state(cx), path_param::<Id>(cx))
      .await?
      .tweet;
   if !has_playable_video(&tweet) {
      return Err(Error::NotFound("No video found".into()).into());
   }
   Ok(view! {
      video_embed(tweet: &tweet)
   })
}

/// `ActivityPub` JSON endpoint for Discord multi-image support.
/// Discord fetches this to get all media attachments for carousel display.
async fn activity_pub_status(
   State(state): State<AppState>,
   Path((username, status)): Path<(String, String)>,
   headers: HeaderMap,
) -> Result<Response> {
   let activity = status.parse::<ActivityId>()?;
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
      return activity_response(&state, activity).await;
   }

   // Otherwise redirect to normal status page
   Ok(Redirect::to(&format!("/{username}/status/{}", activity.tweet())).into_response())
}

/// Mastodon API v1 status endpoint used by Discord after discovery.
async fn mastodon_status(
   State(state): State<AppState>,
   Path(status): Path<String>,
) -> Result<Response> {
   activity_response(&state, status.parse()?).await
}

async fn activity_response(state: &AppState, activity: ActivityId) -> Result<Response> {
   let conversation = cached_conversation(state, &activity.tweet().to_string()).await?;
   let reply_id = conversation.tweet.reply_id;
   let mut replied_to = if reply_id > 0 {
      conversation
         .before
         .content
         .iter()
         .find(|candidate| candidate.id == reply_id)
         .cloned()
   } else {
      None
   };
   let mut tweet = conversation.tweet;
   if let Some(idx) = activity.photo_idx() {
      tweet.keep_only_photo(idx);
   }

   if replied_to.is_none() && reply_id > 0 && reply_id != tweet.id {
      replied_to = cached_conversation(state, &reply_id.to_string())
         .await
         .ok()
         .map(|context| context.tweet);
   }

   if tweet.is_translatable {
      let kagi = &state.config.config.kagi_token;
      let token = (!kagi.is_empty()).then_some(kagi.as_str());
      if let Ok(translation) = get_cached_translation(state, &tweet, token).await
         && !translation.text.is_empty()
      {
         tweet.translation = Some(translation);
      }
   }

   let mut note = replied_to.as_ref().map_or_else(
      || embed_activity::build_activity_pub(&tweet, &state.config),
      |original| {
         embed_activity::build_activity_pub_with_reply(&tweet, Some(original), &state.config)
      },
   );
   note.id = activity.to_string();
   Ok((
      [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
      Json(note),
   )
      .into_response())
}

async fn cached_conversation(state: &AppState, id: &str) -> Result<Conversation> {
   let cache_key = cache_keys::conversation(id);
   if let Some(cached) = state.cache.get::<Conversation>(&cache_key) {
      return Ok(cached);
   }

   let mut conversation = state
      .api
      .get_conversation(id, None, RankingMode::Relevance)
      .await?;
   state
      .api
      .resolve_unavailable_quote(&mut conversation.tweet)
      .await;
   state.cache.set(&cache_key, &conversation, ttl::DEFAULT);
   Ok(conversation)
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
   let provider = query.provider.filter(|value| !value.is_empty());

   let author_url = format!("https://x.com/{author}/status/{status}");

   let oembed = OEmbedData {
      author_name:   text,
      author_url:    author_url.clone(),
      provider_name: provider
         .clone()
         .unwrap_or_else(|| state.config.server.title.clone()),
      provider_url:  provider.map_or_else(|| state.config.url_prefix().to_owned(), |_| author_url),
      title:         "Embed",
      kind:          "rich",
      version:       "1.0",
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
   let tweet_id = extract_tweet_id(&query.url)
      .ok_or_else(|| Error::InvalidUrl("Could not parse tweet URL".into()))?;

   let tweet = state.api.get_tweet(tweet_id).await?;
   let url_prefix = state.config.url_prefix();

   let author_name = format!("{} (@{})", tweet.user.fullname, tweet.user.username);
   let author_url = format!("{url_prefix}/{}", tweet.user.username);
   let tweet_text = strip_html(&tweet.text);

   let html = format!(
      r#"<blockquote><p>{}</p>— {} <a href="{}">{}</a></blockquote>"#,
      html_escape(&tweet_text),
      html_escape(&author_name),
      html_escape(&format!(
         "{url_prefix}/{}/status/{}",
         tweet.user.username, tweet.id
      )),
      tweet.time.map_or_default(format_relative_time),
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
   let path = url
      .strip_prefix("http://")
      .or_else(|| url.strip_prefix("https://"))
      .map_or(url, |stripped| {
         stripped.split_once('/').map_or("", |(_, rest)| rest)
      });

   let segments = path.split('/').collect::<Vec<&str>>();
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
