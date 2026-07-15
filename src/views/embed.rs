use std::fmt::Write as _;

use maud::{
   DOCTYPE,
   Markup,
   html,
};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;

use super::{
   layout::strip_html,
   renderutils::community_note_to_html,
   tweet::TweetRenderer,
};
use crate::{
   config::{
      Config,
      GifTranscodingMode,
   },
   types::{
      CardKind,
      Gif,
      Photo,
      Prefs,
      Tweet,
      Video,
   },
   utils::{
      entity_expander::{
         expand_entities_for_x,
         html_escape,
      },
      formatters,
   },
};

// ── Helpers ──────────────────────────────────────────────────────────

/// Compute aspect ratio from integer dimensions.
fn aspect_ratio(width: i32, height: i32) -> f64 {
   f64::from(width) / f64::from(height)
}

// ── OG/embed media helpers (extracted from Tweet methods) ──────────────

/// Get images for `OpenGraph` meta tags (photos > video thumb > gif thumb >
/// card image).
pub fn og_images(tweet: &Tweet) -> Vec<&str> {
   if !tweet.photos.is_empty() {
      return tweet
         .photos
         .iter()
         .map(|photo| photo.url.as_str())
         .collect();
   }
   let thumb = tweet
      .video
      .as_ref()
      .map(|vid| vid.thumb.as_str())
      .or_else(|| tweet.gif.as_ref().map(|gif| gif.thumb.as_str()))
      .or_else(|| tweet.card.as_ref().map(|card| card.image.as_str()))
      .filter(|th| !th.is_empty());
   thumb.into_iter().collect()
}

/// Try `f` on the tweet and keep track of which tweet supplied the media.
fn with_quote_media_source<'a, T>(
   tweet: &'a Tweet,
   func: impl Fn(&'a Tweet) -> Option<&'a T>,
) -> Option<(&'a Tweet, &'a T)> {
   func(tweet).map(|media| (tweet, media)).or_else(|| {
      if tweet.has_media() {
         return None;
      }
      let quote = tweet.quote.as_deref()?;
      func(quote).map(|media| (quote, media))
   })
}

/// Get OG images, inheriting from quote tweet if this tweet has no media.
pub fn og_images_with_quote(tweet: &Tweet) -> Vec<&str> {
   let images = og_images(tweet);
   if !images.is_empty() {
      return images;
   }
   tweet
      .quote
      .as_deref()
      .filter(|_| !tweet.has_media())
      .map_or_else(Vec::new, og_images)
}

fn photos_with_quote(tweet: &Tweet) -> &[Photo] {
   if !tweet.photos.is_empty() {
      &tweet.photos
   } else if !tweet.has_media()
      && let Some(quote) = tweet.quote.as_deref()
   {
      &quote.photos
   } else {
      &[]
   }
}

/// Get the video source tweet and media, falling back to quote tweet.
fn video_source_with_quote(tweet: &Tweet) -> Option<(&Tweet, &Video)> {
   with_quote_media_source(tweet, |tw| tw.video.as_ref())
}

/// Get the GIF source tweet and media, falling back to quote tweet.
fn gif_source_with_quote(tweet: &Tweet) -> Option<(&Tweet, &Gif)> {
   with_quote_media_source(tweet, |tw| tw.gif.as_ref())
}

struct VideoEmbedMedia<'a> {
   thumbnail_url: &'a str,
   stream_url:    &'a str,
   width:         i32,
   height:        i32,
   is_gif:        bool,
}

fn video_embed_media(tweet: &Tweet) -> Option<VideoEmbedMedia<'_>> {
   if let Some((_, video)) = video_source_with_quote(tweet)
      && let Some(stream_url) = video.best_mp4_url()
   {
      let (width, height) = video.best_dimensions();
      return Some(VideoEmbedMedia {
         thumbnail_url: video.thumb.as_str(),
         stream_url,
         width,
         height,
         is_gif: false,
      });
   }

   let (_, gif) = gif_source_with_quote(tweet)?;
   if gif.url.is_empty() {
      return None;
   }
   Some(VideoEmbedMedia {
      thumbnail_url: gif.thumb.as_str(),
      stream_url:    gif.url.as_str(),
      width:         480,
      height:        480,
      is_gif:        true,
   })
}

/// Whether `/i/videos/tweet/{id}` can render playable media for this tweet.
pub fn has_playable_video(tweet: &Tweet) -> bool {
   video_embed_media(tweet).is_some()
}

/// Build a rich embed description from a tweet, including quote tweets and
/// polls.
pub fn build_embed_description(tweet: &Tweet) -> String {
   let mut desc = tweet.translation.as_ref().map_or_else(
      || strip_html(&tweet.text),
      |tl| {
         let mut translated = tl.text.clone();
         let _ = write!(
            translated,
            "\n\n[{}]\n{}",
            tl.source_lang_display,
            strip_html(&tweet.text)
         );
         translated
      },
   );

   // Append quote tweet text
   if let Some(ref quote) = tweet.quote {
      let _ = write!(
         desc,
         "\n\nQuoting {} (@{}):\n\u{201C}{}\u{201D}",
         quote.user.fullname,
         quote.user.username,
         strip_html(&quote.text)
      );
   }

   // Append poll bar chart
   if let Some(ref poll) = tweet.poll {
      let total_votes = poll.values.iter().sum::<i64>();
      desc.push('\n');

      for (option, &votes) in poll.options.iter().zip(poll.values.iter()) {
         #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "percentage is always 0..100, vote counts fit in f64"
         )]
         let pct = if total_votes > 0 {
            (votes as f64 / total_votes as f64 * 100.0).round() as u32
         } else {
            0
         };
         // Scale bar to ~17 chars max width
         let bar_len = (pct as usize * 17 + 50) / 100;
         let bar = "\u{2588}".repeat(bar_len);
         let _ = write!(desc, "\n{bar} {option}  ({pct}%)");
      }

      let _ = write!(
         desc,
         "\n\n{total_votes} votes \u{2022} {}",
         poll.status_text()
      );
   }

   desc
}

fn text_without_urls(text: &str) -> String {
   strip_html(text)
      .replace("&amp;", "&")
      .replace("&quot;", "\"")
      .replace("&#39;", "'")
      .replace("&apos;", "'")
      .replace("&lt;", "<")
      .replace("&gt;", ">")
      .split_whitespace()
      .filter(|part| !part.starts_with("http://") && !part.starts_with("https://"))
      .collect::<Vec<_>>()
      .join(" ")
}

fn broadcast_title_repeats_tweet(tweet: &Tweet, title: &str) -> bool {
   let tweet_text = text_without_urls(&tweet.text);
   !tweet_text.is_empty() && tweet_text.eq_ignore_ascii_case(title.trim())
}

fn build_discord_embed_description(tweet: &Tweet) -> String {
   let mut description = build_embed_description(tweet);
   let Some(card) = tweet.card.as_ref().filter(|card| {
      matches!(card.kind, CardKind::Broadcast | CardKind::Periscope) && !card.title.is_empty()
   }) else {
      return description;
   };

   if !broadcast_title_repeats_tweet(tweet, &card.title)
      && !description.trim().ends_with(card.title.trim())
   {
      let _ = write!(description, "\n\n📺 {}", card.title);
   }

   description
}

/// Render OG image, twitter:image, and video/gif meta tags for a tweet.
/// Shared between `render_tweet_embed` and `render_status_page`.
fn render_media_meta_tags(tweet: &Tweet, config: &Config, url_prefix: &str) -> Markup {
   let images = og_images_with_quote(tweet);
   let photos = photos_with_quote(tweet);
   let video_source = video_source_with_quote(tweet);
   let has_video = video_source.is_some();
   // GIF tweets provide their own og:image (the transcoded .gif URL)
   // in the GIF branch below, so skip the thumbnail from the image loop.
   let gif_source = gif_source_with_quote(tweet);
   let has_gif = gif_source.is_some();

   html! {
       @if has_video {
           meta property="og:type" content="video.other";
       } @else if !images.is_empty() {
           meta property="og:type" content="photo";
       } @else {
           meta property="og:type" content="article";
       }

       // Image meta tags (skip for GIF tweets — GIF branch provides og:image)
       @if !has_gif {
           @for image in &images {
               @let pic_url = formatters::get_pic_url(image, config.config.base64_media);
               @let full_pic_url = format!("{url_prefix}{pic_url}");
               meta property="og:image" content=(full_pic_url);
               @if let Some(photo) = photos.iter().find(|photo| photo.url == *image) {
                   @if photo.width > 0 && photo.height > 0 {
                       meta property="og:image:width" content=(photo.width);
                       meta property="og:image:height" content=(photo.height);
                   }
               }
           }
       }

       @if has_video {
           meta property="twitter:image" content="0";
       } @else if !has_gif {
           @for image in &images {
               @let pic_url = formatters::get_pic_url(image, config.config.base64_media);
               @let full_pic_url = format!("{url_prefix}{pic_url}");
               meta property="twitter:image" content=(full_pic_url);
               @if let Some(photo) = photos.iter().find(|photo| photo.url == *image) {
                   @if photo.width > 0 && photo.height > 0 {
                       meta property="twitter:image:width" content=(photo.width);
                       meta property="twitter:image:height" content=(photo.height);
                   }
               }
           }
       }

       // Video meta tags for inline playback
       @if let Some((media_tweet, video)) = video_source {
           @let (raw_w, raw_h) = video.best_dimensions();
           @let (width, height) = formatters::scale_dimensions_for_embed(raw_w, raw_h);
           @let embed_url = formatters::get_video_embed_url(config, media_tweet.id);

           @if let Some(mp4_url) = video.best_mp4_url() {
               @let vid_url = formatters::get_vid_url(mp4_url, &config.config.hmac_key, config.config.base64_media);
               @let full_vid_url = format!("{url_prefix}{vid_url}");

               meta property="og:video" content=(full_vid_url);
               meta property="og:video:secure_url" content=(full_vid_url);
               meta property="og:video:type" content="video/mp4";
               meta property="og:video:width" content=(width);
               meta property="og:video:height" content=(height);

               meta name="twitter:card" content="player";
               meta name="twitter:player" content=(embed_url);
               meta name="twitter:player:width" content=(width);
               meta name="twitter:player:height" content=(height);
               meta name="twitter:player:stream" content=(full_vid_url);
               meta name="twitter:player:stream:content_type" content="video/mp4";
           }
       } @else if let Some((_, gif)) = gif_source {
           // GIF tweets: point og:image directly at the transcoded GIF.
           // Discord's image proxy will fetch it, get image/gif content,
           // and render it animated.
           @match config.gif_transcoding.mode {
               GifTranscodingMode::Local => {
                   @let gif_url = formatters::get_gif_url(&gif.url, &config.config.hmac_key, config.config.base64_media);
                   @let full_gif_url = format!("{url_prefix}{gif_url}");
                   meta property="og:image" content=(full_gif_url);
                   meta property="twitter:image" content=(full_gif_url);
               },
               GifTranscodingMode::External => {
                   @let ext_gif_url = formatters::get_external_gif_url(&gif.url, &config.gif_transcoding.external_domain);
                   meta property="og:image" content=(ext_gif_url);
                   meta property="twitter:image" content=(ext_gif_url);
               },
               GifTranscodingMode::Off => {
                   @let thumb_url = formatters::get_pic_url(&gif.thumb, config.config.base64_media);
                   @let full_thumb_url = format!("{url_prefix}{thumb_url}");
                   meta property="og:image" content=(full_thumb_url);
                   meta property="twitter:image" content=(full_thumb_url);
               },
           }
           meta name="twitter:card" content="summary_large_image";
       } @else if !images.is_empty() {
           meta name="twitter:card" content="summary_large_image";
       } @else {
           meta name="twitter:card" content="summary";
       }
   }
}

/// Discord normally gets tweet media from `/api/v1/statuses/{id}`. Broadcasts
/// keep an OG fallback because Discord sometimes skips that second request.
fn render_discord_activity_meta_tags(tweet: &Tweet, config: &Config, url_prefix: &str) -> Markup {
   let has_video = video_source_with_quote(tweet).is_some();
   let has_gif = gif_source_with_quote(tweet).is_some();
   let has_photo = !tweet.photos.is_empty()
      || (!tweet.has_media()
         && tweet
            .quote
            .as_deref()
            .is_some_and(|quote| !quote.photos.is_empty()));
   let broadcast_image = tweet.card.as_ref().and_then(|card| {
      (matches!(card.kind, CardKind::Broadcast | CardKind::Periscope) && !card.image.is_empty())
         .then_some(card.image.as_str())
   });
   let has_activity_media = has_video || has_gif || has_photo || broadcast_image.is_some();

   html! {
       @if let Some(image) = broadcast_image {
           @let image_path = formatters::get_pic_url(image, config.config.base64_media);
           @let image_url = format!("{url_prefix}{image_path}");
           meta property="og:image" content=(image_url);
           meta property="og:image:width" content="1920";
           meta property="og:image:height" content="1080";
           meta property="twitter:image" content=(image_url);
           meta property="twitter:image:width" content="1920";
           meta property="twitter:image:height" content="1080";
       } @else if !has_activity_media {
           @let avatar_path = formatters::get_pic_url(
               &tweet.user.user_pic,
               config.config.base64_media,
           );
           @let avatar_url = format!("{url_prefix}{avatar_path}");
           meta property="og:image" content=(avatar_url);
           meta property="twitter:image" content="0";
       }

       @if has_video {
           meta property="twitter:card" content="player";
       } @else if has_gif || has_photo || broadcast_image.is_some() {
           meta property="twitter:card" content="summary_large_image";
       } @else {
           meta property="twitter:card" content="summary";
       }
   }
}

fn content_disclosure_label(tweet: &Tweet, config: &Config) -> Option<String> {
   let mut labels = Vec::new();
   if tweet.paid_promotion {
      labels.push(format!("{} Paid partnership", config.config.paid_emoji));
   }
   if tweet.ai_generated {
      labels.push(format!("{} Made with AI", config.config.ai_emoji));
   }
   (!labels.is_empty()).then(|| labels.join(" · "))
}

fn embed_provider_name(tweet: &Tweet, config: &Config) -> String {
   content_disclosure_label(tweet, config).map_or_else(
      || config.server.title.clone(),
      |label| format!("{} · {label}", config.server.title),
   )
}

/// Build the oEmbed URL for a tweet's engagement metrics.
fn build_oembed_url(tweet: &Tweet, config: &Config) -> String {
   let engagement_text = formatters::format_engagement_text(
      tweet.stats.likes,
      tweet.stats.retweets,
      tweet.stats.replies,
      tweet.stats.views,
   );
   let mut url = format!(
      "{}/owoembed?text={}&author={}&status={}",
      config.url_prefix(),
      formatters::url_encode(&engagement_text),
      formatters::url_encode(&tweet.user.username),
      tweet.id,
   );
   let provider = content_disclosure_label(tweet, config)
      .map(|_| embed_provider_name(tweet, config))
      .or_else(|| {
         (video_source_with_quote(tweet).is_some() && !engagement_text.is_empty())
            .then(|| engagement_text.clone())
      });
   if let Some(provider) = provider {
      let _ = write!(url, "&provider={}", formatters::url_encode(&provider));
   }
   url
}

/// Render a tweet embed with proper OG meta tags for Discord.
#[expect(
   clippy::module_name_repetitions,
   reason = "public API name is clear and conventional"
)]
pub fn render_tweet_embed(tweet: &Tweet, config: &Config, discord_activity: bool) -> Markup {
   let url_prefix = config.url_prefix();
   let oembed_url = build_oembed_url(tweet, config);
   let description = if discord_activity {
      build_discord_embed_description(tweet)
   } else {
      build_embed_description(tweet)
   };

   html! {
       (DOCTYPE)
       html lang="en" {
           head {
               meta charset="utf-8";
               meta name="viewport" content="width=device-width, initial-scale=1.0";

               meta property="og:site_name" content=(embed_provider_name(tweet, config));
               meta property="og:title" content=(format!("@{}", tweet.user.username));
               meta property="og:description" content=(description);

               @if discord_activity {
                   meta property="theme-color" content="#005ace";
                   (render_discord_activity_meta_tags(tweet, config, url_prefix))
               } @else {
                   (render_media_meta_tags(tweet, config, url_prefix))
               }

               @if discord_activity {
                   link rel="alternate"
                       href=(format!("{url_prefix}/users/{}/statuses/{}", tweet.user.username, tweet.id))
                       type="application/activity+json";
               }

               // oEmbed link for engagement metrics in embed footer
               link rel="alternate"
                   href=(oembed_url)
                   type="application/json+oembed";

               title { "@" (tweet.user.username) " on teapot" }
               link rel="stylesheet" type="text/css" href=(super::layout::STYLE_CSS);
               link rel="stylesheet" type="text/css" href=(super::layout::FONTELLO_CSS);
           }
           body {
               div class="tweet-embed" {
                   (TweetRenderer::new(tweet, config, true).render())
               }
           }
       }
   }
}

/// Render video embed page.
#[expect(
   clippy::module_name_repetitions,
   reason = "public API name is clear and conventional"
)]
pub fn render_video_embed(tweet: &Tweet, config: &Config) -> Markup {
   let url_prefix = config.url_prefix();

   let media = video_embed_media(tweet);
   let thumbnail_path = media
      .as_ref()
      .map(|media| formatters::get_pic_url(media.thumbnail_url, config.config.base64_media))
      .unwrap_or_default();
   let thumbnail_url = if thumbnail_path.is_empty() {
      String::new()
   } else {
      format!("{url_prefix}{thumbnail_path}")
   };
   let stream_path = media.as_ref().map(|media| {
      formatters::get_vid_url(
         media.stream_url,
         &config.config.hmac_key,
         config.config.base64_media,
      )
   });
   let stream_url = stream_path
      .as_ref()
      .map(|path| format!("{url_prefix}{path}"));
   let (width, height) = media
      .as_ref()
      .map_or((1280, 720), |media| (media.width, media.height));
   let is_gif = media.as_ref().is_some_and(|media| media.is_gif);

   html! {
       (DOCTYPE)
       html lang="en" {
           head {
               meta charset="utf-8";
               meta name="viewport" content="width=device-width, initial-scale=1.0";

               meta property="og:type" content="video";
               @if !thumbnail_url.is_empty() {
                   meta property="og:image" content=(thumbnail_url);
               }

               // Twitter player card
               meta name="twitter:card" content="player";
               meta name="twitter:player:width" content=(width);
               meta name="twitter:player:height" content=(height);

               @if let Some(url) = stream_url.as_deref() {
                   meta name="twitter:player:stream" content=(url);
                   meta name="twitter:player:stream:content_type" content="video/mp4";
               }

               title { "Video" }
               link rel="stylesheet" type="text/css" href=(super::layout::STYLE_CSS);
               link rel="stylesheet" type="text/css" href=(super::layout::FONTELLO_CSS);
           }
           body {
               div class="embed-video" {
                   video controls="" preload="metadata" poster=(thumbnail_path) autoplay[is_gif] muted[is_gif] loop[is_gif] playsinline[is_gif] {
                       @if let Some(path) = stream_path.as_deref() {
                           source src=(path) type="video/mp4";
                       }
                   }
               }
           }
       }
   }
}

/// Mastodon API v1-compatible status for Discord embed support.
/// Discord uses `created_at` for the footer timestamp and `content`
/// for rich text rendering (blockquotes for quoted tweets).
#[derive(Debug, Serialize)]
pub struct ActivityPubNote {
   pub id:                     String,
   pub url:                    String,
   pub uri:                    String,
   pub created_at:             String,
   pub edited_at:              Option<String>,
   pub reblog:                 Option<()>,
   pub in_reply_to_id:         Option<String>,
   pub in_reply_to_account_id: Option<String>,
   pub language:               String,
   pub content:                String,
   pub spoiler_text:           String,
   pub sensitive:              bool,
   pub visibility:             String,
   pub application:            MastodonApplication,
   pub media_attachments:      Vec<MediaAttachment>,
   pub account:                MastodonAccount,
   pub mentions:               Vec<()>,
   pub tags:                   Vec<()>,
   pub emojis:                 Vec<()>,
   pub card:                   Option<MastodonPreviewCard>,
   pub poll:                   Option<()>,
}

#[derive(Debug, Serialize)]
pub struct MastodonPreviewCard {
   pub url:           String,
   pub title:         String,
   pub description:   String,
   #[serde(rename = "type")]
   pub type_:         String,
   pub authors:       Vec<()>,
   pub author_name:   String,
   pub author_url:    String,
   pub provider_name: String,
   pub provider_url:  String,
   pub html:          String,
   pub width:         i32,
   pub height:        i32,
   pub image:         Option<String>,
   pub embed_url:     String,
   pub blurhash:      Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MastodonApplication {
   pub name:    String,
   pub website: Option<String>,
}

#[derive(Debug, Serialize)]
#[expect(
   clippy::struct_excessive_bools,
   reason = "mirrors the Mastodon API account schema Discord expects"
)]
pub struct MastodonAccount {
   pub id:               String,
   pub display_name:     String,
   pub username:         String,
   pub acct:             String,
   pub url:              String,
   pub uri:              String,
   pub created_at:       String,
   pub locked:           bool,
   pub bot:              bool,
   pub discoverable:     bool,
   pub indexable:        bool,
   pub group:            bool,
   pub avatar:           String,
   pub avatar_static:    String,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub header:           Option<String>,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub header_static:    Option<String>,
   pub followers_count:  i64,
   pub following_count:  i64,
   pub statuses_count:   i64,
   pub hide_collections: bool,
   pub noindex:          bool,
   pub emojis:           Vec<()>,
   pub roles:            Vec<()>,
   pub fields:           Vec<()>,
}

#[derive(Debug, Serialize)]
pub struct MediaAttachment {
   pub id:                 String,
   #[serde(rename = "type")]
   pub type_:              String,
   pub url:                String,
   pub preview_url:        Option<String>,
   pub remote_url:         Option<String>,
   pub preview_remote_url: Option<String>,
   pub text_url:           Option<String>,
   pub description:        Option<String>,
   pub meta:               Option<MediaMeta>,
}

#[derive(Debug, Serialize)]
pub struct MediaMeta {
   pub original: MediaDimensions,
}

#[derive(Debug, Serialize)]
pub struct MediaDimensions {
   pub width:  i32,
   pub height: i32,
   pub size:   String,
   pub aspect: f64,
}

/// Create a Mastodon-compatible media attachment.
fn make_attachment(
   type_: &str,
   url: String,
   preview: Option<String>,
   description: Option<String>,
   width: i32,
   height: i32,
) -> MediaAttachment {
   MediaAttachment {
      id: "114163769487684704".to_owned(),
      type_: type_.to_owned(),
      url,
      preview_url: preview,
      remote_url: None,
      preview_remote_url: None,
      text_url: None,
      description,
      meta: Some(MediaMeta {
         original: MediaDimensions {
            width,
            height,
            size: format!("{width}x{height}"),
            aspect: aspect_ratio(width, height),
         },
      }),
   }
}

const fn dimensions_or(width: i32, height: i32, fallback: (i32, i32)) -> (i32, i32) {
   if width > 0 && height > 0 {
      (width, height)
   } else {
      fallback
   }
}

fn full_media_url(url_prefix: &str, path: &str) -> String {
   format!("{url_prefix}{path}")
}

fn application_name(source: &str) -> String {
   let mut name = String::with_capacity(source.len());
   let mut in_tag = false;
   for character in source.chars() {
      match character {
         '<' => in_tag = true,
         '>' => in_tag = false,
         _ if !in_tag => name.push(character),
         _ => {},
      }
   }
   name
}

fn card_image_dimensions(url: &str) -> (i32, i32) {
   url.split(['?', '&'])
      .find_map(|part| {
         let dimensions = part.strip_prefix("name=")?;
         let (width, height) = dimensions.split_once('x')?;
         Some((width.parse().ok()?, height.parse().ok()?))
      })
      .unwrap_or_default()
}

fn build_preview_card(
   tweet: &Tweet,
   config: &Config,
   url_prefix: &str,
) -> Option<MastodonPreviewCard> {
   let card = tweet.card.as_ref().filter(|card| {
      !card.url.is_empty()
         && !matches!(card.kind, CardKind::Hidden | CardKind::Unknown)
         && tweet.poll.is_none()
   })?;
   let url = if card.url.starts_with("http://") || card.url.starts_with("https://") {
      card.url.clone()
   } else {
      format!("{url_prefix}{}", card.url)
   };
   let (width, height) = match card_image_dimensions(&card.image) {
      (0, 0) if matches!(card.kind, CardKind::Broadcast | CardKind::Periscope) => (1920, 1080),
      dimensions => dimensions,
   };
   let image = (!card.image.is_empty()).then(|| {
      full_media_url(
         url_prefix,
         &formatters::get_pic_url(&card.image, config.config.base64_media),
      )
   });

   Some(MastodonPreviewCard {
      url: url.clone(),
      title: card.title.clone(),
      description: card.text.clone(),
      type_: "link".to_owned(),
      authors: vec![],
      author_name: String::new(),
      author_url: String::new(),
      provider_name: card.dest.clone(),
      provider_url: url,
      html: String::new(),
      width,
      height,
      image,
      embed_url: String::new(),
      blurhash: None,
   })
}

/// Build media attachments for a single tweet's photos/video/gif.
fn build_media_attachments(
   tweet: &Tweet,
   config: &Config,
   url_prefix: &str,
) -> Vec<MediaAttachment> {
   let mut attachments = Vec::new();

   if let Some(video) = tweet.video.as_ref() {
      let (raw_width, raw_height) = video.best_dimensions();
      let (width, height) = formatters::scale_dimensions_for_embed(raw_width, raw_height);
      if let Some(mp4_url) = video.best_mp4_url() {
         let poster = tweet
            .photos
            .first()
            .map_or(video.thumb.as_str(), |photo| photo.url.as_str());
         attachments.push(make_attachment(
            "video",
            full_media_url(
               url_prefix,
               &formatters::get_vid_url(
                  mp4_url,
                  &config.config.hmac_key,
                  config.config.base64_media,
               ),
            ),
            Some(full_media_url(
               url_prefix,
               &formatters::get_pic_url(poster, config.config.base64_media),
            )),
            None,
            width,
            height,
         ));
      }
   }

   for photo in &tweet.photos {
      let (width, height) = dimensions_or(photo.width, photo.height, (1200, 675));
      attachments.push(make_attachment(
         "image",
         full_media_url(
            url_prefix,
            &formatters::get_orig_pic_url(&photo.url, config.config.base64_media),
         ),
         None,
         (!photo.alt_text.is_empty()).then(|| photo.alt_text.clone()),
         width,
         height,
      ));
   }

   if let Some(ref gif) = tweet.gif
      && !gif.url.is_empty()
   {
      let (width, height) = dimensions_or(gif.width, gif.height, (480, 480));
      let description = (!gif.alt_text.is_empty()).then(|| gif.alt_text.clone());
      match config.gif_transcoding.mode {
         GifTranscodingMode::Local => {
            attachments.push(make_attachment(
               "image",
               full_media_url(
                  url_prefix,
                  &formatters::get_gif_url(
                     &gif.url,
                     &config.config.hmac_key,
                     config.config.base64_media,
                  ),
               ),
               None,
               description,
               width,
               height,
            ));
         },
         GifTranscodingMode::External => {
            attachments.push(make_attachment(
               "image",
               formatters::get_external_gif_url(&gif.url, &config.gif_transcoding.external_domain),
               None,
               description,
               width,
               height,
            ));
         },
         GifTranscodingMode::Off => {
            attachments.push(make_attachment(
               "video",
               full_media_url(
                  url_prefix,
                  &formatters::get_vid_url(
                     &gif.url,
                     &config.config.hmac_key,
                     config.config.base64_media,
                  ),
               ),
               Some(full_media_url(
                  url_prefix,
                  &formatters::get_pic_url(&gif.thumb, config.config.base64_media),
               )),
               description,
               width,
               height,
            ));
         },
      }
   }

   attachments
}

fn append_media_overflow(content: &mut String, tweet: &Tweet, config: &Config) {
   if tweet.additional_videos.is_empty() {
      return;
   }

   let count = usize::from(tweet.video.is_some())
      + tweet.additional_videos.len()
      + tweet.photos.len()
      + usize::from(tweet.gif.is_some());
   let url = format!(
      "{}/{}/status/{}",
      config.url_prefix(),
      tweet.user.username,
      tweet.id
   );
   let _ = write!(
      content,
      r#"<sub>🗂️ <a href="{}">View all {count} media</a></sub><br>"#,
      html_escape(&url)
   );
}

fn preserve_activity_spacing(html: &str) -> String {
   const MAX_RUN: usize = 64;

   let mut output = String::with_capacity(html.len());
   let mut spaces = 0_usize;
   let flush_spaces = |target: &mut String, run_length: usize| {
      if run_length == 1 {
         target.push(' ');
      } else if run_length > 1 {
         for index in 0..run_length.min(MAX_RUN) {
            if index.is_multiple_of(2) {
               target.push_str("&nbsp;");
            } else {
               target.push(' ');
            }
         }
         if run_length > MAX_RUN {
            target.push(' ');
         }
      }
   };

   for character in html.chars() {
      if character == ' ' {
         spaces += 1;
         continue;
      }
      flush_spaces(&mut output, spaces);
      spaces = 0;
      output.push(character);
   }
   flush_spaces(&mut output, spaces);
   output
}

fn activity_text(tweet: &Tweet) -> String {
   let expanded = expand_entities_for_x(tweet.text.trim(), &tweet.entities).replace('\n', "<br>︀︀");
   preserve_activity_spacing(&expanded)
}

fn activity_text_limited(tweet: &Tweet, max_chars: usize) -> String {
   let text = tweet.text.trim();
   if text.chars().count() <= max_chars {
      return activity_text(tweet);
   }

   let prefix = text.chars().take(max_chars).collect::<String>();
   let mut cutoff = prefix
      .rfind(char::is_whitespace)
      .map(|byte_index| prefix[..byte_index].chars().count())
      .filter(|&char_index| char_index >= max_chars * 3 / 4)
      .unwrap_or(max_chars);

   if let Some(entity) = tweet
      .entities
      .iter()
      .find(|entity| entity.indices.0 < cutoff && entity.indices.1 > cutoff)
   {
      cutoff = entity.indices.0;
   }

   let truncated = text.chars().take(cutoff).collect::<String>();
   let entities = tweet
      .entities
      .iter()
      .filter(|entity| entity.indices.1 <= cutoff)
      .cloned()
      .collect::<Vec<_>>();
   let expanded = expand_entities_for_x(truncated.trim_end(), &entities).replace('\n', "<br>︀︀");
   let expanded = preserve_activity_spacing(&expanded);
   format!("{expanded}…")
}

fn activity_text_with_reply_mentions(tweet: &Tweet) -> String {
   let text = activity_text(tweet);
   if !tweet.reply_mentions_stripped {
      return text;
   }
   let Some(first_reply) = tweet.reply.first() else {
      return text;
   };
   if tweet
      .text
      .trim_start()
      .get(..=first_reply.len())
      .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&format!("@{first_reply}")))
   {
      return text;
   }

   let mentions = tweet
      .reply
      .iter()
      .map(|username| {
         let username = html_escape(username);
         format!(r#"<a href="https://x.com/{username}">@{username}</a>"#)
      })
      .collect::<Vec<_>>()
      .join(" ");
   format!("{mentions} {text}")
}

fn engagement_html(tweet: &Tweet) -> String {
   let status_id = tweet.id;
   let mut parts = Vec::new();

   if tweet.stats.replies > 0 {
      parts.push(format!(
         r#"<a href="https://x.com/intent/tweet?in_reply_to={status_id}">💬</a> {}"#,
         formatters::abbreviate_number(tweet.stats.replies)
      ));
   }
   if tweet.stats.retweets > 0 {
      parts.push(format!(
         r#"<a href="https://x.com/intent/retweet?tweet_id={status_id}">🔁</a> {}"#,
         formatters::abbreviate_number(tweet.stats.retweets)
      ));
   }
   if tweet.stats.likes > 0 {
      parts.push(format!(
         r#"<a href="https://x.com/intent/like?tweet_id={status_id}">❤️</a> {}"#,
         formatters::abbreviate_number(tweet.stats.likes)
      ));
   }
   if tweet.stats.views > 0 {
      parts.push(format!(
         "👁️ {}",
         formatters::abbreviate_number(tweet.stats.views)
      ));
   }

   if parts.is_empty() {
      String::new()
   } else {
      format!("<b>{}&ensp;</b>", parts.join("&ensp;"))
   }
}

fn append_poll(content: &mut String, tweet: &Tweet) {
   let Some(poll) = tweet.poll.as_ref() else {
      return;
   };

   let total_votes = if poll.votes > 0 {
      poll.votes
   } else {
      poll.values.iter().sum()
   };
   content.push_str("<blockquote>");

   for (option, votes) in poll.options.iter().zip(&poll.values) {
      let percentage = if total_votes > 0 {
         (votes.saturating_mul(100) + total_votes / 2) / total_votes
      } else {
         0
      }
      .clamp(0, 100);
      let bar_len = usize::try_from(percentage * 32 / 100).unwrap_or_default();
      let _ = write!(
         content,
         "{}<br><b>{}</b>&emsp;{percentage}%<br>︀︀︀<br>︀",
         "█".repeat(bar_len),
         html_escape(option),
      );
   }

   let _ = write!(
      content,
      "{} votes · {}</blockquote>",
      formatters::abbreviate_number(total_votes),
      html_escape(&poll.status_text()),
   );
}

fn append_quote(content: &mut String, tweet: &Tweet) {
   let Some(quote) = tweet.quote.as_ref() else {
      return;
   };

   let quote_url = format!("https://x.com/{}/status/{}", quote.user.username, quote.id);
   let profile_url = format!("https://x.com/{}", quote.user.username);
   let _ = write!(
      content,
      r#"<blockquote><b><a href="{quote_url}">Quoting</a> {} (<a href="{profile_url}">@{}</a>)</b><br>︀<br>{}</blockquote>"#,
      html_escape(&quote.user.fullname),
      html_escape(&quote.user.username),
      activity_text(quote),
   );
}

fn reply_context<'a>(tweet: &Tweet, replied_to: Option<&'a Tweet>) -> Option<&'a Tweet> {
   replied_to.filter(|original| {
      tweet.reply_id > 0 && original.id == tweet.reply_id && !original.text.trim().is_empty()
   })
}

fn append_reply_context(content: &mut String, original: &Tweet) {
   const MAX_CHARS: usize = 220;

   let status_url = format!(
      "https://x.com/{}/status/{}",
      original.user.username, original.id
   );
   let profile_url = format!("https://x.com/{}", original.user.username);
   let username = html_escape(&original.user.username);
   let _ = write!(
      content,
      r#"<blockquote><b>↩ <a href="{status_url}">Original post</a> · <a href="{profile_url}">@{username}</a></b><br>{}</blockquote>"#,
      activity_text_limited(original, MAX_CHARS),
   );
}

fn append_community_note(content: &mut String, tweet: &Tweet) {
   let Some(note) = tweet.note.as_ref() else {
      return;
   };

   let note_html = community_note_to_html(note).replace('\n', "<br>︀︀");
   let _ = write!(
      content,
      "<blockquote><b>📝 Community Note</b><br>{note_html}</blockquote>",
   );
}

fn append_article_preview(content: &mut String, tweet: &Tweet, config: &Config) {
   let Some(card) = tweet
      .card
      .as_ref()
      .filter(|card| card.kind == CardKind::Article && !card.title.is_empty())
   else {
      return;
   };

   if content == "<br><br>" {
      content.clear();
   }

   let url = if card.url.starts_with("http://") || card.url.starts_with("https://") {
      card.url.clone()
   } else {
      format!("{}{}", config.url_prefix(), card.url)
   };
   let description = preserve_activity_spacing(&html_escape(&card.text).replace('\n', "<br>"));
   let _ = write!(
      content,
      r#"<blockquote><b>📄 <a href="{}">{}</a></b>"#,
      html_escape(&url),
      html_escape(&card.title),
   );
   if !description.is_empty() {
      let _ = write!(content, "<br>{description}");
   }
   content.push_str("</blockquote>");
}

fn append_audio_space_preview(content: &mut String, tweet: &Tweet) {
   let Some(card) = tweet
      .card
      .as_ref()
      .filter(|card| card.kind == CardKind::Audiospace && !card.url.is_empty())
   else {
      return;
   };

   if content == "<br><br>" {
      content.clear();
   }

   let title = if card.title.is_empty() {
      "X Space"
   } else {
      &card.title
   };
   let _ = write!(
      content,
      r#"<blockquote><b>🎙️ <a href="{}">{}</a></b>"#,
      html_escape(&card.url),
      html_escape(title),
   );
   if !card.text.is_empty() {
      let _ = write!(content, "<br>{}", html_escape(&card.text));
   }
   if !card.dest.is_empty() {
      let _ = write!(content, "<br>{}", html_escape(&card.dest));
   }
   content.push_str("</blockquote>");
}

fn append_broadcast_preview(content: &mut String, tweet: &Tweet) {
   let Some(card) = tweet.card.as_ref().filter(|card| {
      matches!(card.kind, CardKind::Broadcast | CardKind::Periscope) && !card.url.is_empty()
   }) else {
      return;
   };

   if content == "<br><br>" {
      content.clear();
   }

   let title = if card.title.is_empty() {
      "Live broadcast"
   } else {
      &card.title
   };
   if broadcast_title_repeats_tweet(tweet, title) {
      content.push_str("<blockquote>📺");
      if !card.text.is_empty() {
         let _ = write!(content, " {}", html_escape(&card.text));
      }
      if !card.dest.is_empty() {
         let _ = write!(content, "<br>{}", html_escape(&card.dest));
      }
      content.push_str("</blockquote>");
      return;
   }
   let _ = write!(
      content,
      r#"<blockquote><b>📺 <a href="{}">{}</a></b>"#,
      html_escape(&card.url),
      html_escape(title),
   );
   if !card.text.is_empty() {
      let _ = write!(content, "<br>{}", html_escape(&card.text));
   }
   if !card.dest.is_empty() {
      let _ = write!(content, "<br>{}", html_escape(&card.dest));
   }
   content.push_str("</blockquote>");
}

/// Build rich Mastodon-status HTML for Discord's activity embed path.
fn build_mastodon_content(tweet: &Tweet, replied_to: Option<&Tweet>, config: &Config) -> String {
   let reply_context = reply_context(tweet, replied_to);
   let tweet_text = if reply_context.is_some() {
      activity_text(tweet)
   } else {
      activity_text_with_reply_mentions(tweet)
   };
   let mut content = tweet.translation.as_ref().map_or_else(
      || format!("{tweet_text}<br><br>"),
      |tl| {
         let translated = html_escape(&tl.text).replace('\n', "<br>");
         let translated = preserve_activity_spacing(&translated);
         format!(
            "<b>📑 Translated from {}</b><br><br>{}<br><br><blockquote><b>Original \
             text</b><br>{}</blockquote>",
            html_escape(&tl.source_lang_display),
            translated,
            tweet_text,
         )
      },
   );

   append_article_preview(&mut content, tweet, config);
   append_audio_space_preview(&mut content, tweet);
   append_broadcast_preview(&mut content, tweet);
   append_quote(&mut content, tweet);
   if let Some(original) = reply_context {
      append_reply_context(&mut content, original);
   }
   append_community_note(&mut content, tweet);

   if reply_context.is_none() && !tweet.reply.is_empty() {
      let username = html_escape(&tweet.reply[0]);
      content = format!(
         r#"<sub>↩ <a href="https://x.com/{username}" class="u-url mention"> (@{username})</a></sub><br>{content}"#
      );
   }

   append_poll(&mut content, tweet);
   append_media_overflow(&mut content, tweet, config);

   let engagement = engagement_html(tweet);
   if !engagement.is_empty() {
      content.push_str(&engagement);
   }

   content
}

/// Build Mastodon API v1-compatible status JSON for Discord.
pub fn build_activity_pub(tweet: &Tweet, config: &Config) -> ActivityPubNote {
   build_activity_pub_with_reply(tweet, None, config)
}

/// Build a Mastodon status with optional replied-to context for Discord.
pub fn build_activity_pub_with_reply(
   tweet: &Tweet,
   replied_to: Option<&Tweet>,
   config: &Config,
) -> ActivityPubNote {
   let url_prefix = config.url_prefix();

   let mut attachments = build_media_attachments(tweet, config, url_prefix);

   if !tweet.has_media()
      && let Some(ref quote) = tweet.quote
   {
      attachments.extend(build_media_attachments(quote, config, url_prefix));
   }

   let card = build_preview_card(tweet, config, url_prefix);

   // Discord ignores Mastodon's PreviewCard object, but does render media
   // attachments.
   if attachments.is_empty()
      && let Some(preview_card) = card.as_ref()
      && let Some(image) = preview_card.image.as_ref()
   {
      let (width, height) = dimensions_or(preview_card.width, preview_card.height, (800, 419));
      attachments.push(make_attachment(
         "image",
         image.clone(),
         None,
         Some(preview_card.title.clone()),
         width,
         height,
      ));
   }

   let created_at = tweet.time.map_or_else(
      || time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
      |ts| ts.format(&Rfc3339).unwrap(),
   );
   let account_created_at = tweet.user.join_date.map_or_else(
      || time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
      |ts| ts.format(&Rfc3339).unwrap(),
   );

   let status_url = format!("https://x.com/{}/status/{}", tweet.user.username, tweet.id);
   let avatar_url = formatters::get_pic_url(&tweet.user.user_pic, config.config.base64_media);
   let avatar_url = format!("{url_prefix}{avatar_url}");
   let header_url = (!tweet.user.banner.is_empty()).then(|| {
      format!(
         "{url_prefix}{}",
         formatters::get_pic_url(&tweet.user.banner, config.config.base64_media)
      )
   });

   ActivityPubNote {
      id: tweet.id.to_string(),
      url: status_url.clone(),
      uri: status_url.clone(),
      created_at,
      edited_at: None,
      reblog: None,
      in_reply_to_id: None,
      in_reply_to_account_id: None,
      language: if tweet.lang.is_empty() {
         "en".to_owned()
      } else {
         tweet.lang.clone()
      },
      content: build_mastodon_content(tweet, replied_to, config),
      spoiler_text: String::new(),
      sensitive: tweet.sensitive,
      visibility: "public".to_owned(),
      application: MastodonApplication {
         name:    application_name(&tweet.source),
         website: None,
      },
      media_attachments: attachments,
      account: MastodonAccount {
         id:               tweet.user.id.clone(),
         display_name:     tweet.user.fullname.clone(),
         username:         tweet.user.username.clone(),
         acct:             tweet.user.username.clone(),
         url:              status_url.clone(),
         uri:              status_url,
         created_at:       account_created_at,
         locked:           false,
         bot:              false,
         discoverable:     true,
         indexable:        false,
         group:            false,
         avatar:           avatar_url.clone(),
         avatar_static:    avatar_url,
         header:           header_url.clone(),
         header_static:    header_url,
         followers_count:  tweet.user.followers,
         following_count:  tweet.user.following,
         statuses_count:   tweet.user.tweets,
         hide_collections: false,
         noindex:          false,
         emojis:           vec![],
         roles:            vec![],
         fields:           vec![],
      },
      mentions: vec![],
      tags: vec![],
      emojis: vec![],
      card,
      poll: None,
   }
}

/// Render a full status page with OG meta tags, video embeds, and
/// `ActivityPub` discovery. Uses [`super::layout::PageLayout`] with custom
/// head content for media-specific OG tags, oEmbed, and `ActivityPub` links.
pub fn render_status_page(
   tweet: &Tweet,
   content: &Markup,
   prefs: &Prefs,
   config: &Config,
   username: &str,
   id: &str,
   discord_activity: bool,
) -> Markup {
   let url_prefix = config.url_prefix();
   let oembed_url = build_oembed_url(tweet, config);

   let title = format!(
      "{} (@{}): \"{}\"",
      tweet.user.fullname, tweet.user.username, tweet.text
   );
   let og_title = format!("{} (@{})", tweet.user.fullname, tweet.user.username);
   let description = if discord_activity {
      build_discord_embed_description(tweet)
   } else {
      build_embed_description(tweet)
   };
   let canonical = format!("https://x.com/{username}/status/{id}");
   let referer = format!("/{username}/status/{id}");

   let avatar_url = formatters::get_pic_url(&tweet.user.user_pic, config.config.base64_media);

   let head_extra = html! {
       link rel="canonical" href=(canonical);
       meta property="og:url" content=(canonical);
       meta property="twitter:site" content=(format!("@{}", tweet.user.username));
       meta property="twitter:creator" content=(format!("@{}", tweet.user.username));
       meta property="twitter:title" content=(og_title);
       link rel="apple-touch-icon" href=(format!("{url_prefix}{avatar_url}"));

       // Publish time for Discord footer timestamp
       @if !discord_activity && let Some(ts) = tweet.time {
           @let iso = ts.format(&Rfc3339).unwrap_or_default();
           meta property="article:published_time" content=(iso);
       }

       @if discord_activity {
           (render_discord_activity_meta_tags(tweet, config, url_prefix))
       } @else {
           (render_media_meta_tags(tweet, config, url_prefix))
       }

       meta property="og:title" content=(og_title);
       meta property="og:description" content=(description);
       meta property="og:site_name" content=(embed_provider_name(tweet, config));
       @if discord_activity {
           // Discord recognizes this as a Mastodon status, then requests
           // `/api/v1/statuses/{id}` from the same host.
            link rel="alternate"
                href=(format!("{url_prefix}/users/{username}/statuses/{id}"))
                type="application/activity+json";
       }

       // oEmbed link for engagement metrics
       link rel="alternate"
           href=(oembed_url)
           type="application/json+oembed"
           title=(tweet.user.fullname);
   };

   let rss_url = format!("/{username}/status/{id}/rss");

   super::layout::PageLayout::new(config, &title, content.clone())
      .description(&description)
      .prefs(prefs)
      .rss(&rss_url)
      .canonical(&canonical)
      .referer(&referer)
      .theme_color("#005ace")
      .head_extra(&head_extra)
      .custom_open_graph()
      .render()
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{
      api::schema::CommunityNote,
      config::{
         AppConfig,
         CacheConfig,
         GifTranscodingConfig,
         PreferencesConfig,
         ServerConfig,
      },
      types::{
         Card,
         Photo,
         Poll,
         User,
         VideoType,
         VideoVariant,
      },
   };

   fn test_config() -> Config {
      Config {
         server:          ServerConfig {
            hostname:             "teapot.test".to_owned(),
            title:                "teapot".to_owned(),
            address:              "127.0.0.1".to_owned(),
            port:                 443,
            public_port:          None,
            https:                true,
            http_max_connections: 100,
            static_dir:           "./public".to_owned(),
         },
         cache:           CacheConfig {
            list_minutes: 120,
            rss_minutes:  10,
            max_entries:  50_000,
         },
         config:          AppConfig {
            hmac_key:            "0123456789abcdef0123456789abcdef".to_owned(),
            base64_media:        true,
            enable_rss:          true,
            enable_debug:        false,
            debug_token:         String::new(),
            proxy:               String::new(),
            proxy_auth:          String::new(),
            api_proxy:           String::new(),
            disable_tid:         false,
            max_concurrent_reqs: 2,
            paid_emoji:          ":paid:".to_owned(),
            ai_emoji:            ":ai:".to_owned(),
            kagi_token:          String::new(),
            kagi_token_file:     String::new(),
         },
         preferences:     PreferencesConfig::default(),
         gif_transcoding: GifTranscodingConfig::default(),
         url_prefix:      "https://teapot.test".to_owned(),
      }
   }

   fn user(username: &str) -> User {
      User {
         id: username.to_owned(),
         username: username.to_owned(),
         fullname: username.to_owned(),
         user_pic: "https://pbs.twimg.com/profile_images/avatar.jpg".to_owned(),
         ..User::default()
      }
   }

   fn tweet(id: i64, username: &str, text: &str) -> Tweet {
      Tweet {
         id,
         user: user(username),
         text: text.to_owned(),
         available: true,
         ..Tweet::default()
      }
   }

   fn video() -> Video {
      Video {
         thumb: "https://pbs.twimg.com/ext_tw_video_thumb/thumb.jpg".to_owned(),
         variants: vec![VideoVariant {
            content_type: VideoType::Mp4,
            url:          "https://video.twimg.com/ext_tw_video/1/pu/vid/720x1280/video.mp4?tag=12"
               .to_owned(),
            bitrate:      2_176_000,
            resolution:   1280,
         }],
         ..Video::default()
      }
   }

   fn quoted_video_tweet() -> Tweet {
      let mut outer = tweet(100, "outer", "outer text");
      let mut quote = tweet(200, "quote", "quote text");
      quote.video = Some(video());
      outer.quote = Some(Box::new(quote));
      outer
   }

   #[test]
   fn quoted_video_player_uses_quote_tweet_id() {
      let html = render_tweet_embed(&quoted_video_tweet(), &test_config(), false).into_string();

      assert!(
         html.contains(r#"name="twitter:player" content="https://teapot.test/i/videos/tweet/200""#)
      );
      assert!(
         !html
            .contains(r#"name="twitter:player" content="https://teapot.test/i/videos/tweet/100""#)
      );
      assert!(html.contains(r#"name="twitter:player:stream" content="https://teapot.test/video/"#));
      assert!(html.contains("/enc/"));
   }

   #[test]
   fn video_embed_uses_signed_proxy_for_quoted_video() {
      let tweet = quoted_video_tweet();
      let html = render_video_embed(&tweet, &test_config()).into_string();

      assert!(has_playable_video(&tweet));
      assert!(html.contains(r#"poster="/pic/enc/"#));
      assert!(html.contains(r#"<source src="/video/"#));
      assert!(html.contains(r#"name="twitter:player:stream" content="https://teapot.test/video/"#));
      assert!(html.contains("/enc/"));
      assert!(!html.contains("https://teapot.test/video/https://video.twimg.com"));
   }

   #[test]
   fn activity_pub_quote_fallback_uses_signed_media_urls() {
      let mut tweet = quoted_video_tweet();
      tweet.reply = vec!["parent".to_owned()];
      tweet.reply_mentions_stripped = true;

      let activity = build_activity_pub(&tweet, &test_config());
      let attachment = activity.media_attachments.first().unwrap();

      assert_eq!(activity.media_attachments.len(), 1);
      assert_eq!(attachment.type_, "video");
      assert!(attachment.url.starts_with("https://teapot.test/video/"));
      assert!(attachment.url.contains("/enc/"));
      assert!(!attachment.url.contains("/video/https://video.twimg.com"));
      assert!(
         attachment
            .preview_url
            .as_ref()
            .is_some_and(|url| url.starts_with("https://teapot.test/pic/enc/"))
      );
      let dimensions = &attachment.meta.as_ref().unwrap().original;
      assert_eq!((dimensions.width, dimensions.height), (720, 1280));
      assert_eq!(dimensions.size, "720x1280");
      assert!(activity.content.contains(
         r#"<sub>↩ <a href="https://x.com/parent" class="u-url mention"> (@parent)</a>"#
      ));
      assert!(
         activity
            .content
            .contains(r#"</sub><br><a href="https://x.com/parent">@parent</a> outer text"#)
      );
   }

   #[test]
   fn activity_reply_context_links_and_truncates_original_post() {
      let mut reply = tweet(402, "replier", "reply text");
      reply.reply_id = 401;
      reply.reply = vec!["parent".to_owned()];
      reply.reply_mentions_stripped = true;
      let original_text = format!("{} final marker", "original context ".repeat(20));
      let original = tweet(401, "parent", &original_text);

      let activity = build_activity_pub_with_reply(&reply, Some(&original), &test_config());

      let context_start = activity
         .content
         .find(r#"<blockquote><b>↩ <a href="https://x.com/parent/status/401">Original post</a> · <a href="https://x.com/parent">@parent</a></b><br>"#)
         .unwrap();
      assert!(activity.content.find("reply text").unwrap() < context_start);
      assert!(!activity.content.contains("<sub>↩"));
      assert!(!activity.content.contains(r#">@parent</a> reply text"#));
      assert!(activity.content.contains("original context"));
      assert!(activity.content.contains('…'));
      assert!(!activity.content.contains("final marker"));
   }

   #[test]
   fn activity_payload_preserves_intentional_space_runs() {
      let status = tweet(403, "spaced", "How it started     How it's going");

      let activity = build_activity_pub(&status, &test_config());

      assert!(
         activity
            .content
            .contains("How it started&nbsp; &nbsp; &nbsp;How it's going")
      );
   }

   #[test]
   fn activity_payload_includes_community_note_with_sources() {
      let mut status = tweet(403, "noted", "A claim");
      let note_text = "Germany did so in 2002. Source";
      let source_start = note_text.find("Source").unwrap();
      status.note = Some(CommunityNote {
         text:  note_text.to_owned(),
         links: vec![(
            source_start,
            source_start + "Source".chars().count(),
            "https://example.com/source".to_owned(),
         )],
      });

      let activity = build_activity_pub(&status, &test_config());

      assert!(activity.content.contains(
         r#"<blockquote><b>📝 Community Note</b><br>Germany did so in 2002. <a href="https://example.com/source">Source</a></blockquote>"#,
      ));
   }

   #[test]
   fn activity_payload_marks_sensitive_media() {
      let mut status = tweet(403, "sensitive", "content warning");
      status.sensitive = true;
      status.photos.push(Photo {
         url: "https://pbs.twimg.com/media/sensitive.jpg".to_owned(),
         width: 1200,
         height: 800,
         ..Photo::default()
      });

      let activity = build_activity_pub(&status, &test_config());

      assert!(activity.sensitive);
      assert!(activity.spoiler_text.is_empty());
      assert_eq!(activity.media_attachments.len(), 1);
   }

   #[test]
   fn activity_payload_renders_article_title_and_excerpt() {
      let mut status = tweet(404, "writer", "");
      status.card = Some(Card {
         kind: CardKind::Article,
         url: "/writer/article/404".to_owned(),
         title: "Defining Taste".to_owned(),
         text: "Taste is the ability to make qualitative judgments.".to_owned(),
         dest: "Article · @writer".to_owned(),
         ..Card::default()
      });

      let activity = build_activity_pub(&status, &test_config());

      assert!(activity.content.contains(
         r#"<blockquote><b>📄 <a href="https://teapot.test/writer/article/404">Defining Taste</a></b><br>Taste is the ability to make qualitative judgments.</blockquote>"#,
      ));
      assert!(!activity.content.starts_with("<br>"));
   }

   #[test]
   fn activity_payload_renders_audio_space_preview() {
      let mut status = tweet(405, "host", "Join our Space");
      status.card = Some(Card {
         kind: CardKind::Audiospace,
         url: "https://x.com/i/spaces/1AxRnnrNvyDxl".to_owned(),
         title: "Building in public".to_owned(),
         text: "Scheduled · Jul 16, 2026 · 8:00 AM UTC".to_owned(),
         dest: "Hosted by Example (@host)".to_owned(),
         ..Card::default()
      });

      let activity = build_activity_pub(&status, &test_config());

      assert!(activity.content.contains(
         r#"<blockquote><b>🎙️ <a href="https://x.com/i/spaces/1AxRnnrNvyDxl">Building in public</a></b><br>Scheduled · Jul 16, 2026 · 8:00 AM UTC<br>Hosted by Example (@host)</blockquote>"#,
      ));
   }

   #[test]
   fn activity_payload_renders_live_broadcast_preview() {
      let mut status = tweet(406, "tbpn", "Going live");
      status.card = Some(Card {
         kind: CardKind::Broadcast,
         url: "https://x.com/i/broadcasts/1XxyggAaLzvGM".to_owned(),
         title: "Stripe x PayPal".to_owned(),
         text: "Live now · 26 watching".to_owned(),
         dest: "Hosted by TBPN (@tbpn)".to_owned(),
         image: "https://video.pscp.tv/latest.jpg".to_owned(),
         ..Card::default()
      });

      let activity = build_activity_pub(&status, &test_config());
      let html = render_status_page(
         &status,
         &html! {},
         &Prefs::default(),
         &test_config(),
         "tbpn",
         "406",
         true,
      )
      .into_string();
      let image_url = format!(
         "https://teapot.test{}",
         formatters::get_pic_url("https://video.pscp.tv/latest.jpg", true)
      );

      assert!(activity.content.contains("📺"));
      assert!(activity.content.contains("Stripe x PayPal"));
      assert!(activity.content.contains("Live now · 26 watching"));
      assert!(activity.content.contains("Hosted by TBPN (@tbpn)"));
      assert_eq!(activity.media_attachments.len(), 1);
      assert_eq!(
         activity.media_attachments[0]
            .meta
            .as_ref()
            .unwrap()
            .original
            .size,
         "1920x1080"
      );
      assert!(html.contains(r#"property="og:site_name" content="teapot""#));
      assert!(html.contains(&format!(r#"property="og:image" content="{image_url}""#)));
      assert!(html.contains(r#"property="og:image:width" content="1920""#));
      assert!(html.contains(r#"property="twitter:card" content="summary_large_image""#));
      assert!(!build_oembed_url(&status, &test_config()).contains("&provider="));

      status.text = "Stripe x PayPal https://t.co/broadcast".to_owned();
      assert_eq!(
         build_discord_embed_description(&status),
         "Stripe x PayPal https://t.co/broadcast"
      );
      let duplicate_activity = build_activity_pub(&status, &test_config());
      assert_eq!(
         duplicate_activity
            .content
            .matches("Stripe x PayPal")
            .count(),
         1
      );
      assert!(
         duplicate_activity.content.contains(
            "<blockquote>📺 Live now · 26 watching<br>Hosted by TBPN (@tbpn)</blockquote>"
         )
      );

      status.text = "Stripe &amp; PayPal https://t.co/broadcast".to_owned();
      status.card.as_mut().unwrap().title = "Stripe & PayPal".to_owned();
      let entity_activity = build_activity_pub(&status, &test_config());
      assert_eq!(
         entity_activity
            .content
            .matches("Stripe &amp; PayPal")
            .count(),
         1
      );
   }

   #[test]
   fn oembed_footer_renders_content_disclosures() {
      let mut status = tweet(405, "creator", "Generated campaign artwork");
      status.paid_promotion = true;
      status.ai_generated = true;

      let oembed_url = build_oembed_url(&status, &test_config());
      let html = render_tweet_embed(&status, &test_config(), true).into_string();
      let activity = build_activity_pub(&status, &test_config());

      assert!(oembed_url.contains(&format!(
         "&provider={}",
         formatters::url_encode("teapot · :paid: Paid partnership · :ai: Made with AI")
      )));
      assert!(html.contains(
         r#"property="og:site_name" content="teapot · :paid: Paid partnership · :ai: Made with AI""#
      ));
      assert!(!activity.content.contains("Paid partnership"));
      assert!(!activity.content.contains("Made with AI"));
   }

   #[test]
   fn activity_payload_includes_external_link_preview_card() {
      let mut status = tweet(403, "github", "Download the app");
      status.card = Some(Card {
         kind: CardKind::SummaryLarge,
         url: "https://github.com/mobile?utm_source=x".to_owned(),
         title: "GitHub Mobile".to_owned(),
         dest: "github.com".to_owned(),
         text: "The world's development platform, in your pocket".to_owned(),
         image:
            "https://pbs.twimg.com/card_img/2075083153316749312/qlKLI1ak?format=jpg&name=800x419"
               .to_owned(),
         ..Card::default()
      });

      let activity = build_activity_pub(&status, &test_config());
      let card = activity.card.unwrap();

      assert_eq!(card.url, "https://github.com/mobile?utm_source=x");
      assert_eq!(card.title, "GitHub Mobile");
      assert_eq!(
         card.description,
         "The world's development platform, in your pocket"
      );
      assert_eq!(card.type_, "link");
      assert_eq!((card.width, card.height), (800, 419));
      assert_eq!(card.provider_name, "github.com");
      assert!(
         card
            .image
            .is_some_and(|image| image.starts_with("https://teapot.test/pic/enc/"))
      );
      assert_eq!(activity.media_attachments.len(), 1);
      let attachment = &activity.media_attachments[0];
      assert_eq!(attachment.type_, "image");
      assert_eq!(attachment.description.as_deref(), Some("GitHub Mobile"));
      assert_eq!(
         attachment
            .meta
            .as_ref()
            .map(|meta| (meta.original.width, meta.original.height,)),
         Some((800, 419))
      );
   }

   #[test]
   fn activity_payload_renders_poll_and_quote_blocks() {
      let mut status = tweet(405, "poller", "pick one");
      status.poll = Some(Poll {
         options: vec!["first".to_owned(), "second".to_owned()],
         values: vec![3, 1],
         votes: 4,
         ..Poll::default()
      });
      let quote = tweet(406, "quoted", "quoted text");
      status.quote = Some(Box::new(quote));

      let activity = build_activity_pub(&status, &test_config());

      assert!(
         activity
            .content
            .contains("<blockquote>████████████████████████")
      );
      assert!(activity.content.contains("<b>first</b>&emsp;75%"));
      assert!(
         activity
            .content
            .contains("4 votes · Final results</blockquote>")
      );
      assert!(
         activity
            .content
            .contains(r#"<a href="https://x.com/quoted/status/406">Quoting</a> quoted"#)
      );
   }

   #[test]
   fn activity_payload_uses_image_attachment_for_transcoded_gif() {
      let mut tweet = tweet(407, "gif", "animated");
      tweet.gif = Some(Gif {
         url:      "https://video.twimg.com/tweet_video/animation.mp4".to_owned(),
         thumb:    "https://pbs.twimg.com/tweet_video_thumb/animation.jpg".to_owned(),
         alt_text: "animation alt text".to_owned(),
         width:    400,
         height:   400,
      });
      let mut config = test_config();
      config.gif_transcoding.mode = GifTranscodingMode::Local;

      let activity = build_activity_pub(&tweet, &config);
      let attachment = activity.media_attachments.first().unwrap();

      assert_eq!(attachment.type_, "image");
      assert!(attachment.url.starts_with("https://teapot.test/gif/"));
      assert_eq!(attachment.preview_url, None);
      assert_eq!(
         attachment.description.as_deref(),
         Some("animation alt text")
      );
   }

   #[test]
   fn player_oembed_duplicates_engagement_in_provider() {
      let mut tweet = quoted_video_tweet();
      tweet.stats.views = 1_200;
      let html = render_tweet_embed(&tweet, &test_config(), true).into_string();

      assert!(html.contains("&amp;provider="));
      assert!(html.contains(r#"property="twitter:card" content="player""#));
      assert!(!html.contains(r#"property="og:video""#));
   }

   #[test]
   fn activity_pub_photo_urls_are_encoded_and_keep_alt_text() {
      let mut tweet = tweet(300, "photos", "photo text");
      tweet.photos.push(Photo {
         url:      "https://pbs.twimg.com/media/photo.jpg?format=jpg&name=large".to_owned(),
         alt_text: "alt text".to_owned(),
         width:    2048,
         height:   1536,
      });

      let activity = build_activity_pub(&tweet, &test_config());
      let attachment = activity.media_attachments.first().unwrap();

      assert_eq!(attachment.type_, "image");
      assert!(
         attachment
            .url
            .starts_with("https://teapot.test/pic/orig/enc/")
      );
      assert_eq!(attachment.preview_url, None);
      assert_eq!(attachment.description.as_deref(), Some("alt text"));
      let dimensions = &attachment.meta.as_ref().unwrap().original;
      assert_eq!((dimensions.width, dimensions.height), (2048, 1536));
      assert_eq!(dimensions.size, "2048x1536");
      assert!((dimensions.aspect - 4.0 / 3.0).abs() < f64::EPSILON);
   }

   #[test]
   fn activity_mixed_media_prioritizes_video_with_photo_poster() {
      let mut status = tweet(301, "mixed", "mixed media");
      let photo_url = "https://pbs.twimg.com/media/photo.jpg";
      status.photos.push(Photo {
         url: photo_url.to_owned(),
         width: 1200,
         height: 1600,
         ..Photo::default()
      });
      status.video = Some(video());
      let config = test_config();

      let activity = build_activity_pub(&status, &config);
      let expected_poster = format!(
         "https://teapot.test{}",
         formatters::get_pic_url(photo_url, config.config.base64_media),
      );

      assert_eq!(activity.media_attachments.len(), 2);
      assert_eq!(activity.media_attachments[0].type_, "video");
      assert_eq!(activity.media_attachments[1].type_, "image");
      assert_eq!(
         activity.media_attachments[0].preview_url.as_deref(),
         Some(expected_poster.as_str()),
      );
   }

   #[test]
   fn activity_pub_links_to_additional_videos() {
      let mut status = tweet(302, "videos", "two videos");
      status.video = Some(video());
      let mut second = video();
      second.thumb = "https://pbs.twimg.com/ext_tw_video_thumb/second.jpg".to_owned();
      second.variants[0].url =
         "https://video.twimg.com/ext_tw_video/2/pu/vid/1280x720/video.mp4".to_owned();
      status.additional_videos.push(second);

      let activity = build_activity_pub(&status, &test_config());

      assert_eq!(activity.media_attachments.len(), 1);
      assert_eq!(activity.media_attachments[0].type_, "video");
      assert!(activity.content.contains(
         r#"<sub>🗂️ <a href="https://teapot.test/videos/status/302">View all 2 media</a></sub><br>"#
      ));
   }

   #[test]
   fn status_page_uses_author_first_embed_metadata() {
      let tweet = tweet(400, "G2CSGO", "Trying hard to win/lose/something");
      let html = render_status_page(
         &tweet,
         &html! {},
         &Prefs::default(),
         &test_config(),
         "G2CSGO",
         "400",
         false,
      )
      .into_string();

      assert!(!html.contains(
         r#"property="og:title" content="G2CSGO (@G2CSGO): &quot;Trying hard to win/lose/something&quot;""#
      ));
      assert!(html.contains(r#"property="twitter:title" content="G2CSGO (@G2CSGO)""#));
      assert!(html.contains(r#"property="og:title" content="G2CSGO (@G2CSGO)""#));
      assert!(
         html.contains(r#"property="og:description" content="Trying hard to win/lose/something""#)
      );
      assert!(html.contains(r#"property="twitter:site" content="@G2CSGO""#));
      assert!(html.contains(r#"property="og:url" content="https://x.com/G2CSGO/status/400""#));
      assert!(html.contains(r#"link rel="apple-touch-icon" href="https://teapot.test/pic/enc/"#));
      assert!(
         !html.contains(
            r#"link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png""#
         )
      );
   }

   #[test]
   fn discord_activity_page_defers_photo_media_to_mastodon_api() {
      let mut tweet = tweet(401, "photos", "photo text");
      tweet.time = time::OffsetDateTime::from_unix_timestamp(1).ok();
      tweet.photos.push(Photo {
         url: "https://pbs.twimg.com/media/photo.jpg".to_owned(),
         width: 2048,
         height: 1536,
         ..Photo::default()
      });

      let html = render_status_page(
         &tweet,
         &html! {},
         &Prefs::default(),
         &test_config(),
         "photos",
         "401",
         true,
      )
      .into_string();

      assert!(html.contains(r##"property="theme-color" content="#005ace""##));
      assert!(html.contains(r##"rel="mask-icon" href="/safari-pinned-tab.svg" color="#005ace""##));
      assert!(html.contains(
         r#"href="https://teapot.test/users/photos/statuses/401" type="application/activity+json""#
      ));
      assert!(html.contains(r#"property="twitter:card" content="summary_large_image""#));
      assert!(!html.contains(r#"property="og:image" content="https://teapot.test/pic/"#));
      assert!(!html.contains("article:published_time"));
   }

   #[test]
   fn discord_activity_page_uses_avatar_for_text_only_status() {
      let tweet = tweet(402, "plain", "plain text");
      let html = render_status_page(
         &tweet,
         &html! {},
         &Prefs::default(),
         &test_config(),
         "plain",
         "402",
         true,
      )
      .into_string();

      assert!(html.contains(r#"property="og:image" content="https://teapot.test/pic/enc/"#));
      assert!(html.contains(r#"property="twitter:image" content="0""#));
      assert!(html.contains(r#"property="twitter:card" content="summary""#));
   }

   #[test]
   fn generic_photo_metadata_includes_real_dimensions() {
      let mut tweet = tweet(403, "photos", "photo text");
      tweet.photos.push(Photo {
         url: "https://pbs.twimg.com/media/photo.jpg".to_owned(),
         width: 2048,
         height: 1536,
         ..Photo::default()
      });

      let html = render_status_page(
         &tweet,
         &html! {},
         &Prefs::default(),
         &test_config(),
         "photos",
         "403",
         false,
      )
      .into_string();

      assert!(html.contains(r#"property="og:image:width" content="2048""#));
      assert!(html.contains(r#"property="og:image:height" content="1536""#));
      assert!(html.contains(r#"property="twitter:image" content="https://teapot.test/pic/enc/"#));
      assert!(!html.contains("twitter:image:src"));
      assert!(!html.contains("application/activity+json"));
   }

   #[test]
   fn activity_payload_matches_discord_mastodon_shape() {
      let mut tweet = tweet(404, "photos", "hello\nworld");
      tweet.stats.replies = 2;
      tweet.stats.retweets = 3;
      tweet.stats.likes = 4;
      tweet.stats.views = 5;
      tweet.lang = "en".to_owned();
      tweet.photos.extend([
         Photo {
            url: "https://pbs.twimg.com/media/one.jpg".to_owned(),
            width: 2048,
            height: 1536,
            ..Photo::default()
         },
         Photo {
            url: "https://pbs.twimg.com/media/two.jpg".to_owned(),
            width: 1024,
            height: 2048,
            ..Photo::default()
         },
      ]);

      let activity = build_activity_pub(&tweet, &test_config());

      assert_eq!(activity.id, "404");
      assert_eq!(activity.url, "https://x.com/photos/status/404");
      assert_eq!(activity.language, "en");
      assert_eq!(activity.media_attachments.len(), 2);
      assert_eq!(activity.media_attachments[0].preview_url, None);
      assert_eq!(
         activity.media_attachments[1]
            .meta
            .as_ref()
            .map(|meta| (meta.original.width, meta.original.height)),
         Some((1024, 2048))
      );
      assert!(activity.content.contains("hello<br>︀︀world<br><br><b>"));
      assert!(activity.content.contains("in_reply_to=404"));
      assert!(activity.content.contains("tweet_id=404"));
      assert_eq!(activity.account.avatar, activity.account.avatar_static);
      assert!(activity.card.is_none());
      assert!(activity.poll.is_none());
   }

   #[test]
   fn best_dimensions_parse_non_widescreen_video_url() {
      assert_eq!(video().best_dimensions(), (720, 1280));
   }

   #[test]
   fn activity_application_uses_source_label() {
      assert_eq!(
         application_name(
            r#"<a href="https://mobile.twitter.com" rel="nofollow">Twitter for iPhone</a>"#
         ),
         "Twitter for iPhone"
      );
   }
}
