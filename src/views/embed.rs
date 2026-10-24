use std::fmt::Write as _;

use topcoat::{
   Result,
   context::Cx,
   view::{
      Child,
      View,
      component,
      view,
   },
};

use crate::{
   config::{
      Config,
      GifTranscodingMode,
   },
   types::{
      prefs::Prefs,
      tweet::{
         CardKind,
         Gif,
         Photo,
         Tweet,
         Video,
      },
   },
   utils::formatters,
   views::{
      document::{
         FONTELLO_CSS,
         STYLE_CSS,
         document,
         strip_html,
      },
      embed_activity::ActivityId,
      renderutils::config,
      tweet::tweet_item,
   },
};

// ── Helpers ──────────────────────────────────────────────────────────

/// Compute aspect ratio from integer dimensions.
pub(in crate::views) fn aspect_ratio(width: i32, height: i32) -> f64 {
   if width > 0 && height > 0 {
      f64::from(width) / f64::from(height)
   } else {
      1.0
   }
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
      .or_else(|| tweet.gifs.first().map(|gif| gif.thumb.as_str()))
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
   with_quote_media_source(tweet, |tw| tw.gifs.first())
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

   if let Some(ref quote) = tweet.quote {
      let _ = write!(
         desc,
         "\n\nQuoting {} (@{}):\n\u{201C}{}\u{201D}",
         quote.user.fullname,
         quote.user.username,
         strip_html(&quote.text)
      );
   }

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

pub(in crate::views) fn broadcast_title_repeats_tweet(tweet: &Tweet, title: &str) -> bool {
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

/// OG image, twitter:image, and video/gif meta tags for a tweet. Shared
/// between `tweet_embed` and `status_page`.
#[component]
async fn media_meta_tags(cx: &Cx, tweet: &Tweet) -> Result<impl View> {
   let config = config(cx);
   let url_prefix = config.url_prefix();
   let base64 = config.config.base64_media;
   let photos = photos_with_quote(tweet);
   let video_source = video_source_with_quote(tweet);
   let gif_source = gif_source_with_quote(tweet);
   let images = og_images_with_quote(tweet)
      .into_iter()
      .map(|image| {
         let size = photos
            .iter()
            .find(|photo| photo.url == image && photo.width > 0 && photo.height > 0)
            .map(|photo| (photo.width, photo.height));
         (
            format!("{url_prefix}{}", formatters::get_pic_url(image, base64)),
            size,
         )
      })
      .collect::<Vec<_>>();
   let og_type = if video_source.is_some() {
      "video.other"
   } else if images.is_empty() {
      "article"
   } else {
      "photo"
   };
   let player = video_source.and_then(|(media_tweet, video)| {
      let mp4_url = video.best_mp4_url()?;
      let (raw_width, raw_height) = video.best_dimensions();
      let (width, height) = formatters::scale_dimensions_for_embed(raw_width, raw_height);
      let video_url = format!(
         "{url_prefix}{}",
         formatters::get_vid_url(mp4_url, &config.config.hmac_key, base64)
      );
      Some((
         video_url,
         formatters::get_video_embed_url(config, media_tweet.id),
         width,
         height,
      ))
   });
   // Point og:image for GIF tweets directly at the transcoded GIF. Discord's
   // image proxy will fetch it, get image/gif content, and render it animated.
   let gif_image = gif_source.map(|(_, gif)| {
      match config.gif_transcoding.mode {
         GifTranscodingMode::Local => {
            format!(
               "{url_prefix}{}",
               formatters::get_gif_url(&gif.url, &config.config.hmac_key, base64)
            )
         },
         GifTranscodingMode::External => {
            formatters::get_external_gif_url(&gif.url, &config.gif_transcoding.external_domain)
         },
         GifTranscodingMode::Off => {
            format!(
               "{url_prefix}{}",
               formatters::get_pic_url(&gif.thumb, base64)
            )
         },
      }
   });
   let has_video = video_source.is_some();
   let has_gif = gif_image.is_some();

   Ok(view! {
      <meta property="og:type" content=(og_type)>
      // Skip image meta tags for GIF tweets because their branch provides og:image
      if !has_gif {
         for &(ref url, size) in &images {
            <meta property="og:image" content=(url)>
            if let Some((width, height)) = size {
               <meta property="og:image:width" content=(width)>
               <meta property="og:image:height" content=(height)>
            }
         }
      }
      if has_video {
         <meta property="twitter:image" content="0">
      } else if !has_gif {
         for &(ref url, size) in &images {
            <meta property="twitter:image" content=(url)>
            if let Some((width, height)) = size {
               <meta property="twitter:image:width" content=(width)>
               <meta property="twitter:image:height" content=(height)>
            }
         }
      }
      if has_video {
         if let Some((video_url, embed_url, width, height)) = player {
            <meta property="og:video" content=(&video_url)>
            <meta property="og:video:secure_url" content=(&video_url)>
            <meta property="og:video:type" content="video/mp4">
            <meta property="og:video:width" content=(width)>
            <meta property="og:video:height" content=(height)>
            <meta name="twitter:card" content="player">
            <meta name="twitter:player" content=(embed_url)>
            <meta name="twitter:player:width" content=(width)>
            <meta name="twitter:player:height" content=(height)>
            <meta name="twitter:player:stream" content=(&video_url)>
            <meta name="twitter:player:stream:content_type" content="video/mp4">
         }
      } else if let Some(gif_image) = gif_image {
         <meta property="og:image" content=(&gif_image)>
         <meta property="twitter:image" content=(&gif_image)>
         <meta name="twitter:card" content="summary_large_image">
      } else if !images.is_empty() {
         <meta name="twitter:card" content="summary_large_image">
      } else {
         <meta name="twitter:card" content="summary">
      }
   })
}

/// Discord normally gets tweet media from `/api/v1/statuses/{id}`. Broadcasts
/// keep an OG fallback because Discord sometimes skips that second request.
#[component]
async fn discord_activity_meta_tags(cx: &Cx, tweet: &Tweet) -> Result<impl View> {
   let config = config(cx);
   let url_prefix = config.url_prefix();
   let base64 = config.config.base64_media;
   let has_video = video_source_with_quote(tweet).is_some();
   let has_gif = gif_source_with_quote(tweet).is_some();
   let has_photo = !tweet.photos.is_empty()
      || (!tweet.has_media()
         && tweet
            .quote
            .as_deref()
            .is_some_and(|quote| !quote.photos.is_empty()));
   let broadcast_image = tweet
      .card
      .as_ref()
      .filter(|card| {
         matches!(card.kind, CardKind::Broadcast | CardKind::Periscope) && !card.image.is_empty()
      })
      .map(|card| {
         format!(
            "{url_prefix}{}",
            formatters::get_pic_url(&card.image, base64)
         )
      });
   let has_activity_media = has_video || has_gif || has_photo || broadcast_image.is_some();
   let avatar_url = format!(
      "{url_prefix}{}",
      formatters::get_pic_url(&tweet.user.user_pic, base64)
   );
   let card = if has_video {
      "player"
   } else if has_gif || has_photo || broadcast_image.is_some() {
      "summary_large_image"
   } else {
      "summary"
   };

   Ok(view! {
      if let Some(image_url) = broadcast_image {
         <meta property="og:image" content=(&image_url)>
         <meta property="og:image:width" content="1920">
         <meta property="og:image:height" content="1080">
         <meta property="twitter:image" content=(&image_url)>
         <meta property="twitter:image:width" content="1920">
         <meta property="twitter:image:height" content="1080">
      } else if !has_activity_media {
         <meta property="og:image" content=(avatar_url)>
         <meta property="twitter:image" content="0">
      }
      <meta property="twitter:card" content=(card)>
   })
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
#[component]
pub async fn tweet_embed(cx: &Cx, tweet: &Tweet, discord_activity: bool) -> Result<impl View> {
   let config = config(cx);
   let url_prefix = config.url_prefix();
   let description = if discord_activity {
      build_discord_embed_description(tweet)
   } else {
      build_embed_description(tweet)
   };
   let prefs = Prefs::with_defaults(config);

   Ok(view! {
      <!DOCTYPE html>
      <html lang="en">
         <head>
            <meta charset="utf-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <meta property="og:site_name" content=(embed_provider_name(tweet, config))>
            <meta property="og:title" content=(format!("@{}", tweet.user.username))>
            <meta property="og:description" content=(description)>
            if discord_activity {
               <meta property="theme-color" content="#005ace">
               discord_activity_meta_tags(tweet: tweet)
               <link
                  rel="alternate"
                  href=(format!("{url_prefix}/users/{}/statuses/{}", tweet.user.username, tweet.id))
                  type="application/activity+json"
               >
            } else {
               media_meta_tags(tweet: tweet)
            }
            // oEmbed link for engagement metrics in embed footer
            <link rel="alternate" href=(build_oembed_url(tweet, config)) type="application/json+oembed">
            <title>"@" (&tweet.user.username) " on teapot"</title>
            <link rel="stylesheet" type="text/css" href=(STYLE_CSS)>
            <link rel="stylesheet" type="text/css" href=(FONTELLO_CSS)>
         </head>
         <body>
            <div class="tweet-embed">tweet_item(tweet: tweet, prefs: &prefs, main: true)</div>
         </body>
      </html>
   })
}

/// Render video embed page.
#[component]
pub async fn video_embed(cx: &Cx, tweet: &Tweet) -> Result<impl View> {
   let config = config(cx);
   let url_prefix = config.url_prefix();
   let media = video_embed_media(tweet);
   let thumbnail_path = media.as_ref().map_or_default(|media| {
      formatters::get_pic_url(media.thumbnail_url, config.config.base64_media)
   });
   let thumbnail_url =
      (!thumbnail_path.is_empty()).then(|| format!("{url_prefix}{thumbnail_path}"));
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

   Ok(view! {
      <!DOCTYPE html>
      <html lang="en">
         <head>
            <meta charset="utf-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <meta property="og:type" content="video">
            if let Some(url) = thumbnail_url {
               <meta property="og:image" content=(url)>
            }
            <meta name="twitter:card" content="player">
            <meta name="twitter:player:width" content=(width)>
            <meta name="twitter:player:height" content=(height)>
            if let Some(url) = stream_url {
               <meta name="twitter:player:stream" content=(url)>
               <meta name="twitter:player:stream:content_type" content="video/mp4">
            }
            <title>"Video"</title>
            <link rel="stylesheet" type="text/css" href=(STYLE_CSS)>
            <link rel="stylesheet" type="text/css" href=(FONTELLO_CSS)>
         </head>
         <body>
            <div class="embed-video">
               <video
                  controls=""
                  preload="metadata"
                  poster=(thumbnail_path)
                  autoplay=(is_gif)
                  muted=(is_gif)
                  loop=(is_gif)
                  playsinline=(is_gif)
               >
                  if let Some(path) = stream_path {
                     <source src=(path) type="video/mp4">
                  }
               </video>
            </div>
         </body>
      </html>
   })
}

/// A full status page with media `OpenGraph` tags, oEmbed, and `ActivityPub`
/// discovery for Discord.
#[component]
pub async fn status_page<'a>(
   cx: &Cx,
   tweet: &'a Tweet,
   prefs: &'a Prefs,
   username: &'a str,
   id: &'a str,
   activity: Option<ActivityId>,
   #[default] child: Child<'a>,
) -> Result<impl View> {
   let config = config(cx);
   let url_prefix = config.url_prefix();
   let title = format!(
      "{} (@{}): \"{}\"",
      tweet.user.fullname, tweet.user.username, tweet.text
   );
   let full_title = format!("{} (@{})", tweet.user.fullname, tweet.user.username);
   let og_description = if activity.is_some() {
      build_discord_embed_description(tweet)
   } else {
      build_embed_description(tweet)
   };
   let canonical_url = format!("https://x.com/{username}/status/{id}");
   let avatar_url = formatters::get_pic_url(&tweet.user.user_pic, config.config.base64_media);
   let published = tweet
      .time
      .filter(|_| activity.is_none())
      .map(|time| time.to_string());
   let at_handle = format!("@{}", tweet.user.username);

   let rss = format!("/{username}/status/{id}/rss");
   let referer = format!("/{username}/status/{id}");

   Ok(view! {
      let canonical = &canonical_url;
      let description = &og_description;
      let og_title = &full_title;
      let handle = &at_handle;
      document(
         title: &title,
         prefs: prefs,
         description: description,
         rss: &rss,
         canonical: canonical,
         referer: &referer,
         theme_color: "#005ace",
         custom_og: true,
         head: Some(view! {
            <link rel="canonical" href=(canonical)>
            <meta property="og:url" content=(canonical)>
            <meta property="twitter:site" content=(handle)>
            <meta property="twitter:creator" content=(handle)>
            <meta property="twitter:title" content=(og_title)>
            <link rel="apple-touch-icon" href=(format!("{url_prefix}{avatar_url}"))>
            // Publish time for Discord footer timestamp
            if let Some(published) = published {
               <meta property="article:published_time" content=(published)>
            }
            if activity.is_some() {
               discord_activity_meta_tags(tweet: tweet)
            } else {
               media_meta_tags(tweet: tweet)
            }
            <meta property="og:title" content=(og_title)>
            <meta property="og:description" content=(description)>
            <meta property="og:site_name" content=(embed_provider_name(tweet, config))>
            if let Some(activity) = activity {
               // Discord recognizes this as a Mastodon status, then requests
               // `/api/v1/statuses/{id}` from the same host.
               <link
                  rel="alternate"
                  href=(format!("{url_prefix}/users/{username}/statuses/{activity}"))
                  type="application/activity+json"
               >
            }
            // oEmbed link for engagement metrics
            <link
               rel="alternate"
               href=(build_oembed_url(tweet, config))
               type="application/json+oembed"
               title=(&tweet.user.fullname)
            >
         }.into()),
         (child)
      )
   })
}

#[cfg(test)]
#[path = "embed_tests.rs"]
mod tests;
