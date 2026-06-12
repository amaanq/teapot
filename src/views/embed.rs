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
   tweet::TweetRenderer,
};
use crate::{
   config::{
      Config,
      GifTranscodingMode,
   },
   types::{
      Gif,
      Prefs,
      Tweet,
      Video,
   },
   utils::formatters,
};

// ── Helpers ──────────────────────────────────────────────────────────

/// Compute aspect ratio from integer dimensions.
#[expect(
   clippy::cast_precision_loss,
   reason = "video dimensions are small enough for f32"
)]
fn aspect_ratio(width: i32, height: i32) -> f32 {
   width as f32 / height as f32
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

/// Render OG image, twitter:image, and video/gif meta tags for a tweet.
/// Shared between `render_tweet_embed` and `render_status_page`.
fn render_media_meta_tags(tweet: &Tweet, config: &Config, url_prefix: &str) -> Markup {
   let images = og_images_with_quote(tweet);
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
           }
       }

       @if has_video {
           meta property="twitter:image" content="0";
       } @else if !has_gif {
           @for image in &images {
               @let pic_url = formatters::get_pic_url(image, config.config.base64_media);
               @let full_pic_url = format!("{url_prefix}{pic_url}");
               meta property="twitter:image:src" content=(full_pic_url);
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

/// Build the oEmbed URL for a tweet's engagement metrics.
fn build_oembed_url(tweet: &Tweet, url_prefix: &str) -> String {
   let engagement_text = formatters::format_engagement_text(
      tweet.stats.likes,
      tweet.stats.retweets,
      tweet.stats.replies,
      tweet.stats.views,
   );
   format!(
      "{url_prefix}/owoembed?text={}&author={}&status={}",
      formatters::url_encode(&engagement_text),
      formatters::url_encode(&tweet.user.username),
      tweet.id,
   )
}

/// Render a tweet embed with proper OG meta tags for Discord.
#[expect(
   clippy::module_name_repetitions,
   reason = "public API name is clear and conventional"
)]
pub fn render_tweet_embed(tweet: &Tweet, config: &Config) -> Markup {
   let url_prefix = config.url_prefix();
   let oembed_url = build_oembed_url(tweet, url_prefix);

   html! {
       (DOCTYPE)
       html lang="en" {
           head {
               meta charset="utf-8";
               meta name="viewport" content="width=device-width, initial-scale=1.0";

               meta property="og:site_name" content="teapot";
               meta property="og:title" content=(format!("@{}", tweet.user.username));
               meta property="og:description" content=(build_embed_description(tweet));

               (render_media_meta_tags(tweet, config, url_prefix))

               // ActivityPub discovery link for Discord multi-image
               link rel="alternate"
                   href=(format!("{url_prefix}/users/{}/statuses/{}", tweet.user.username, tweet.id))
                   type="application/activity+json";

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
   pub id:                String,
   pub url:               String,
   pub uri:               String,
   pub created_at:        String,
   pub content:           String,
   pub visibility:        String,
   pub media_attachments: Vec<MediaAttachment>,
   pub account:           MastodonAccount,
   pub emojis:            Vec<()>,
}

#[derive(Debug, Serialize)]
pub struct MastodonAccount {
   pub id:           String,
   pub display_name: String,
   pub username:     String,
   pub acct:         String,
   pub url:          String,
   pub avatar:       String,
}

#[derive(Debug, Serialize)]
pub struct MediaAttachment {
   pub id:          String,
   #[serde(rename = "type")]
   pub type_:       String,
   pub url:         String,
   pub preview_url: Option<String>,
   pub remote_url:  Option<String>,
   pub description: Option<String>,
   pub meta:        Option<MediaMeta>,
}

#[derive(Debug, Serialize)]
pub struct MediaMeta {
   pub original: MediaDimensions,
}

#[derive(Debug, Serialize)]
pub struct MediaDimensions {
   pub width:  i32,
   pub height: i32,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub aspect: Option<f32>,
}

/// Create a Mastodon-compatible media attachment.
fn make_attachment(
   type_: &str,
   url: String,
   preview: Option<String>,
   width: i32,
   height: i32,
) -> MediaAttachment {
   MediaAttachment {
      id: "0".to_owned(),
      type_: type_.to_owned(),
      url,
      preview_url: preview,
      remote_url: None,
      description: None,
      meta: Some(MediaMeta {
         original: MediaDimensions {
            width,
            height,
            aspect: Some(aspect_ratio(width, height)),
         },
      }),
   }
}

fn full_media_url(url_prefix: &str, path: &str) -> String {
   format!("{url_prefix}{path}")
}

/// Build media attachments for a single tweet's photos/video/gif.
fn build_media_attachments(
   tweet: &Tweet,
   config: &Config,
   url_prefix: &str,
) -> Vec<MediaAttachment> {
   let mut attachments = Vec::new();

   for photo in &tweet.photos {
      attachments.push(make_attachment(
         "image",
         full_media_url(
            url_prefix,
            &formatters::get_orig_pic_url(&photo.url, config.config.base64_media),
         ),
         Some(full_media_url(
            url_prefix,
            &formatters::get_pic_url(&photo.url, config.config.base64_media),
         )),
         1200,
         675,
      ));
      if let Some(last) = attachments.last_mut()
         && !photo.alt_text.is_empty()
      {
         last.description = Some(photo.alt_text.clone());
      }
   }

   if let Some(ref video) = tweet.video {
      let (width, height) = video.best_dimensions();
      if let Some(mp4_url) = video.best_mp4_url() {
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
               &formatters::get_pic_url(&video.thumb, config.config.base64_media),
            )),
            width,
            height,
         ));
      }
   }

   if let Some(ref gif) = tweet.gif
      && !gif.url.is_empty()
   {
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
         480,
         480,
      ));
   }

   attachments
}

/// Build rich HTML content with quote tweet formatting.
fn build_mastodon_content(tweet: &Tweet) -> String {
   let mut content = tweet.translation.as_ref().map_or_else(
      || tweet.text.replace('\n', "<br>"),
      |tl| {
         format!(
            "{}<br><br><i>[{}]</i><br>{}",
            tl.text.replace('\n', "<br>"),
            tl.source_lang_display,
            tweet.text.replace('\n', "<br>")
         )
      },
   );

   if let Some(ref quote) = tweet.quote {
      let _ = write!(
         content,
         "<br><br><blockquote><b>Quoting {} (@{})</b><br>{}</blockquote>",
         quote.user.fullname,
         quote.user.username,
         quote.text.replace('\n', "<br>")
      );
   }

   if !tweet.reply.is_empty() {
      let replies = tweet
         .reply
         .iter()
         .map(|username| format!("@{username}"))
         .collect::<Vec<_>>()
         .join(" ");
      content = format!("<sub>↩ Replying to {replies}</sub><br>{content}");
   }

   content
}

/// Build Mastodon API v1-compatible status JSON for Discord.
pub fn build_activity_pub(tweet: &Tweet, config: &Config) -> ActivityPubNote {
   let url_prefix = config.url_prefix();

   let mut attachments = build_media_attachments(tweet, config, url_prefix);

   if !tweet.has_media()
      && let Some(ref quote) = tweet.quote
   {
      attachments.extend(build_media_attachments(quote, config, url_prefix));
   }

   let created_at = tweet.time.map_or_else(
      || time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
      |ts| ts.format(&Rfc3339).unwrap(),
   );

   let status_url = format!("{url_prefix}/{}/status/{}", tweet.user.username, tweet.id);
   let avatar_url = formatters::get_pic_url(&tweet.user.user_pic, config.config.base64_media);

   ActivityPubNote {
      id: status_url.clone(),
      url: status_url.clone(),
      uri: status_url,
      created_at,
      content: build_mastodon_content(tweet),
      visibility: "public".to_owned(),
      media_attachments: attachments,
      account: MastodonAccount {
         id:           tweet.user.id.clone(),
         display_name: tweet.user.fullname.clone(),
         username:     tweet.user.username.clone(),
         acct:         tweet.user.username.clone(),
         url:          format!("{url_prefix}/{}", tweet.user.username),
         avatar:       format!("{url_prefix}{avatar_url}"),
      },
      emojis: vec![],
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
) -> Markup {
   let url_prefix = config.url_prefix();
   let oembed_url = build_oembed_url(tweet, url_prefix);

   let title = format!(
      "{} (@{}): \"{}\"",
      tweet.user.fullname, tweet.user.username, tweet.text
   );
   let og_title = format!("{} (@{})", tweet.user.fullname, tweet.user.username);
   let description = build_embed_description(tweet);
   let canonical = format!("https://x.com/{username}/status/{id}");
   let referer = format!("/{username}/status/{id}");

   let avatar_url = formatters::get_pic_url(&tweet.user.user_pic, config.config.base64_media);

   let head_extra = html! {
       link rel="canonical" href=(canonical);
       meta property="og:url" content=(canonical);
       meta property="twitter:site" content=(format!("@{}", tweet.user.username));
       meta property="twitter:creator" content=(format!("@{}", tweet.user.username));
       meta property="theme-color" content="#1F1F1F";
       meta property="twitter:title" content=(og_title);
       link rel="apple-touch-icon" href=(format!("{url_prefix}{avatar_url}"));

       // Publish time for Discord footer timestamp
       @if let Some(ts) = tweet.time {
           @let iso = ts.format(&Rfc3339).unwrap_or_default();
           meta property="article:published_time" content=(iso);
       }

       // Media-specific OG/twitter tags
       (render_media_meta_tags(tweet, config, url_prefix))

       meta property="og:title" content=(og_title);
       meta property="og:description" content=(description);
       meta property="og:site_name" content=(config.server.title);
       meta property="og:locale" content="en_US";

       // ActivityPub discovery link
       link rel="alternate"
           href=(format!("{url_prefix}/users/{username}/statuses/{id}"))
           type="application/activity+json";

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
      .head_extra(&head_extra)
      .custom_open_graph()
      .render()
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{
      config::{
         AppConfig,
         CacheConfig,
         GifTranscodingConfig,
         PreferencesConfig,
         ServerConfig,
      },
      types::{
         Photo,
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
      let html = render_tweet_embed(&quoted_video_tweet(), &test_config()).into_string();

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
      assert!(activity.content.contains("Replying to @parent"));
   }

   #[test]
   fn activity_pub_photo_urls_are_encoded_and_keep_alt_text() {
      let mut tweet = tweet(300, "photos", "photo text");
      tweet.photos.push(Photo {
         url:      "https://pbs.twimg.com/media/photo.jpg?format=jpg&name=large".to_owned(),
         alt_text: "alt text".to_owned(),
      });

      let activity = build_activity_pub(&tweet, &test_config());
      let attachment = activity.media_attachments.first().unwrap();

      assert_eq!(attachment.type_, "image");
      assert!(
         attachment
            .url
            .starts_with("https://teapot.test/pic/orig/enc/")
      );
      assert!(
         attachment
            .preview_url
            .as_ref()
            .is_some_and(|url| url.starts_with("https://teapot.test/pic/enc/"))
      );
      assert_eq!(attachment.description.as_deref(), Some("alt text"));
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
   fn best_dimensions_parse_non_widescreen_video_url() {
      assert_eq!(video().best_dimensions(), (720, 1280));
   }
}
