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

/// Get images for `OpenGraph` meta tags (photos > video thumb > gif thumb > card image).
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
      .or_else(|| {
         tweet
            .card
            .as_ref()
            .map(|card| card.image.as_str())
      })
      .filter(|th| !th.is_empty());
   thumb.into_iter().collect()
}

/// Try `f` on the tweet, falling back to the quote tweet when this tweet has
/// no media.
fn with_quote_fallback<'a, T>(
   tweet: &'a Tweet,
   func: impl Fn(&'a Tweet) -> Option<T>,
) -> Option<T> {
   func(tweet).or_else(|| {
      if tweet.has_media() {
         return None;
      }
      func(tweet.quote.as_deref()?)
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

/// Get the video, falling back to quote tweet.
pub fn video_with_quote(tweet: &Tweet) -> Option<&Video> {
   with_quote_fallback(tweet, |tw| tw.video.as_ref())
}

/// Get the gif, falling back to quote tweet.
pub fn gif_with_quote(tweet: &Tweet) -> Option<&Gif> {
   with_quote_fallback(tweet, |tw| tw.gif.as_ref())
}

/// Build a rich embed description from a tweet, including quote tweets and
/// polls.
pub fn build_embed_description(tweet: &Tweet) -> String {
   let mut desc = strip_html(&tweet.text);

   // Append quote tweet text
   if let Some(ref quote) = tweet.quote {
      let _ = write!(
         desc,
         "\n\nQuoting @{}:\n{}",
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
   let has_video = video_with_quote(tweet).is_some();

   html! {
       @if has_video {
           meta property="og:type" content="video.other";
       } @else if !images.is_empty() {
           meta property="og:type" content="photo";
       } @else {
           meta property="og:type" content="article";
       }

       // Image meta tags
       @for image in &images {
           @let pic_url = formatters::get_pic_url(image, config.config.base64_media);
           @let full_pic_url = format!("{url_prefix}{pic_url}");
           meta property="og:image" content=(full_pic_url);
       }

       @if has_video {
           meta property="twitter:image" content="0";
       } @else {
           @for image in &images {
               @let pic_url = formatters::get_pic_url(image, config.config.base64_media);
               @let full_pic_url = format!("{url_prefix}{pic_url}");
               meta property="twitter:image:src" content=(full_pic_url);
           }
       }

       // Video meta tags for inline playback
       @if let Some(video) = video_with_quote(tweet) {
           @let (raw_w, raw_h) = video.best_dimensions();
           @let (width, height) = formatters::scale_dimensions_for_embed(raw_w, raw_h);
           @let embed_url = formatters::get_video_embed_url(config, tweet.id);

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
       } @else if let Some(gif) = gif_with_quote(tweet) {
           // GIF tweets: summary_large_image with thumbnail. Discord picks
           // up the animated GIF from the ActivityPub attachment instead.
           @let thumb_url = formatters::get_pic_url(&gif.thumb, config.config.base64_media);
           @let full_thumb_url = format!("{url_prefix}{thumb_url}");

           meta property="og:image" content=(full_thumb_url);
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

   let video = tweet.video.as_ref();
   let thumbnail = video
      .map(|vid| format!("{url_prefix}/pic/{}", vid.thumb))
      .unwrap_or_default();

   let (width, height) = video.map_or((1280, 720), Video::best_dimensions);

   let mp4_url = video.and_then(|vid| vid.best_mp4_url());

   html! {
       (DOCTYPE)
       html lang="en" {
           head {
               meta charset="utf-8";
               meta name="viewport" content="width=device-width, initial-scale=1.0";

               meta property="og:type" content="video";
               @if !thumbnail.is_empty() {
                   meta property="og:image" content=(thumbnail);
               }

               // Twitter player card
               meta name="twitter:card" content="player";
               meta name="twitter:player:width" content=(width);
               meta name="twitter:player:height" content=(height);

               @if let Some(url) = mp4_url {
                   meta name="twitter:player:stream" content=(format!("{url_prefix}/video/{url}"));
                   meta name="twitter:player:stream:content_type" content="video/mp4";
               }

               title { "Video" }
               link rel="stylesheet" type="text/css" href=(super::layout::STYLE_CSS);
               link rel="stylesheet" type="text/css" href=(super::layout::FONTELLO_CSS);
           }
           body {
               div class="embed-video" {
                   video controls="" preload="metadata" poster=(thumbnail) {
                       @if let Some(url) = mp4_url {
                           source src=(format!("{url_prefix}/video/{url}")) type="video/mp4";
                       }
                   }
               }
           }
       }
   }
}

/// Build `ActivityPub` JSON for Discord multi-image support.
#[derive(Debug, Serialize)]
pub struct ActivityPubNote {
   #[serde(rename = "@context")]
   pub context:       String,
   #[serde(rename = "type")]
   pub type_:         String,
   pub id:            String,
   #[serde(rename = "attributedTo")]
   pub attributed_to: String,
   pub content:       String,
   pub published:     String,
   pub attachment:    Vec<MediaAttachment>,
}

#[derive(Debug, Serialize)]
pub struct MediaAttachment {
   #[serde(rename = "type")]
   pub type_:       String,
   pub url:         String,
   pub preview_url: String,
   #[serde(rename = "mediaType")]
   pub media_type:  String,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub description: Option<String>,
   #[serde(skip_serializing_if = "Option::is_none")]
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

/// Build media attachments for a single tweet's photos/video/gif.
fn build_media_attachments(tweet: &Tweet, config: &Config, url_prefix: &str) -> Vec<MediaAttachment> {
   let mut attachments = Vec::new();

   for photo in &tweet.photos {
      attachments.push(MediaAttachment {
         type_:       "Image".to_owned(),
         url:         format!("{url_prefix}/pic/orig/{}", photo.url),
         preview_url: format!("{url_prefix}/pic/{}", photo.url),
         media_type:  "image/jpeg".to_owned(),
         description: None,
         meta:        Some(MediaMeta {
            original: MediaDimensions {
               width:  1200,
               height: 675,
               aspect: Some(1200.0 / 675.0),
            },
         }),
      });
   }

   if let Some(ref video) = tweet.video {
      let (width, height) = video.best_dimensions();
      if let Some(mp4_url) = video.best_mp4_url() {
         attachments.push(MediaAttachment {
            type_:       "Video".to_owned(),
            url:         format!("{url_prefix}/video/{mp4_url}"),
            preview_url: format!("{url_prefix}/pic/{}", video.thumb),
            media_type:  "video/mp4".to_owned(),
            description: None,
            meta:        Some(MediaMeta {
               original: MediaDimensions {
                  width,
                  height,
                  aspect: Some(aspect_ratio(width, height)),
               },
            }),
         });
      }
   }

   if let Some(ref gif) = tweet.gif {
      match config.gif_transcoding.mode {
         GifTranscodingMode::Local => {
            let gif_url = formatters::get_gif_url(
               &gif.url,
               &config.config.hmac_key,
               config.config.base64_media,
            );
            attachments.push(MediaAttachment {
               type_:       "Image".to_owned(),
               url:         format!("{url_prefix}{gif_url}"),
               preview_url: format!("{url_prefix}/pic/{}", gif.thumb),
               media_type:  "image/gif".to_owned(),
               description: None,
               meta:        Some(MediaMeta {
                  original: MediaDimensions {
                     width:  480,
                     height: 480,
                     aspect: Some(1.0),
                  },
               }),
            });
         },
         GifTranscodingMode::External => {
            let ext_url =
               formatters::get_external_gif_url(&gif.url, &config.gif_transcoding.external_domain);
            attachments.push(MediaAttachment {
               type_:       "Image".to_owned(),
               url:         ext_url,
               preview_url: format!("{url_prefix}/pic/{}", gif.thumb),
               media_type:  "image/gif".to_owned(),
               description: None,
               meta:        Some(MediaMeta {
                  original: MediaDimensions {
                     width:  480,
                     height: 480,
                     aspect: Some(1.0),
                  },
               }),
            });
         },
         GifTranscodingMode::Off => {
            attachments.push(MediaAttachment {
               type_:       "Video".to_owned(),
               url:         format!("{url_prefix}/pic/{}", gif.url),
               preview_url: format!("{url_prefix}/pic/{}", gif.thumb),
               media_type:  "video/mp4".to_owned(),
               description: None,
               meta:        Some(MediaMeta {
                  original: MediaDimensions {
                     width:  480,
                     height: 480,
                     aspect: Some(1.0),
                  },
               }),
            });
         },
      }
   }

   attachments
}

/// Build `ActivityPub` JSON from a tweet.
pub fn build_activity_pub(tweet: &Tweet, config: &Config) -> ActivityPubNote {
   let url_prefix = config.url_prefix();

   let mut attachments = build_media_attachments(tweet, config, url_prefix);

   // Inherit quote tweet media when the parent has no media
   if !tweet.has_media()
      && let Some(ref quote) = tweet.quote
   {
      attachments.extend(build_media_attachments(quote, config, url_prefix));
   }

   let published = tweet.time.map_or_else(
      || time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
      |ts| ts.format(&Rfc3339).unwrap(),
   );

   ActivityPubNote {
      context: "https://www.w3.org/ns/activitystreams".to_owned(),
      type_: "Note".to_owned(),
      id: format!("{url_prefix}/{}/status/{}", tweet.user.username, tweet.id),
      attributed_to: format!("{url_prefix}/users/{}", tweet.user.username),
      content: tweet.text.clone(),
      published,
      attachment: attachments,
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

   let head_extra = html! {
       // Override OG title/description with tweet-specific values
       meta property="og:title" content=(og_title);
       meta property="og:description" content=(description);

       // Media-specific OG/twitter tags
       (render_media_meta_tags(tweet, config, url_prefix))

       // ActivityPub discovery link
       link rel="alternate"
           href=(format!("{url_prefix}/users/{username}/statuses/{id}"))
           type="application/activity+json";

       // oEmbed link for engagement metrics
       link rel="alternate"
           href=(oembed_url)
           type="application/json+oembed";
   };

   let rss_url = format!("/{username}/status/{id}/rss");

   super::layout::PageLayout::new(config, &title, content.clone())
      .description(&description)
      .prefs(prefs)
      .rss(&rss_url)
      .canonical(&canonical)
      .referer(&referer)
      .head_extra(&head_extra)
      .render()
}
