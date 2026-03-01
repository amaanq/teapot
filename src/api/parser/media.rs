use std::cmp;

use crate::{
   api::schema::{
      MediaItem,
      MediaType,
      RawVideoContentType,
      TweetLegacy,
   },
   types::{
      Gif,
      Photo,
      User,
      Video,
      VideoType,
      VideoVariant,
   },
};

/// Parsed media result (no side effects on tweet text).
pub struct ParsedMedia {
   pub photos:      Vec<Photo>,
   pub video:       Option<Video>,
   pub gif:         Option<Gif>,
   pub attribution: Option<User>,
   /// t.co and expanded URLs that should be stripped from tweet text.
   pub strip_urls:  Vec<String>,
}

/// Parse media from tweet legacy data.
///
/// Returns parsed media plus a list of URLs to strip from the tweet text.
/// Callers apply the stripping -- this function has no side effects.
pub fn parse_media(legacy: &TweetLegacy) -> ParsedMedia {
   let mut photos = Vec::new();
   let mut video = None;
   let mut gif = None;
   let mut attribution = None;
   let mut strip_urls = Vec::new();

   let items = legacy.media_items();

   for media in items {
      match media.media_type.unwrap_or_default() {
         MediaType::Photo => {
            if let Some(url) = media.media_url_https.as_deref() {
               photos.push(Photo {
                  url:      url.to_owned(),
                  alt_text: media.ext_alt_text.clone().unwrap_or_default(),
               });
            }
         },
         MediaType::Video => {
            video = Some(parse_video(media));
            // Parse media attribution from additional_media_info.source_user
            if let Some(ref info) = media.additional_media_info
               && let Some(ref source_user) = info.source_user
            {
               attribution = User::try_from(source_user.as_ref()).ok();
            }
         },
         MediaType::AnimatedGif => {
            gif = Some(parse_gif(media));
         },
         _ => {},
      }

      // Collect URLs that should be stripped from tweet text
      let tco_url = media.url.as_deref().unwrap_or_default();
      let expanded_url = media.expanded_url.as_deref().unwrap_or(tco_url);
      if !tco_url.is_empty() {
         strip_urls.push(tco_url.to_owned());
      }
      if !expanded_url.is_empty() && expanded_url != tco_url {
         strip_urls.push(expanded_url.to_owned());
      }
   }

   ParsedMedia {
      photos,
      video,
      gif,
      attribution,
      strip_urls,
   }
}

/// Parse video from typed media item.
pub fn parse_video(media: &MediaItem) -> Video {
   let thumb = media
      .media_url_https
      .as_deref()
      .unwrap_or_default()
      .to_owned();

   let mut variants = Vec::new();
   let mut duration_ms = 0_i32;

   if let Some(ref video_info) = media.video_info {
      duration_ms = video_info.duration_millis;

      for variant in &video_info.variants {
         let bitrate = variant.bitrate;
         let url = variant.url.as_deref().unwrap_or_default().to_owned();

         let content_type = match variant.content_type.unwrap_or_default() {
            RawVideoContentType::Mp4 => VideoType::Mp4,
            RawVideoContentType::M3u8 | RawVideoContentType::Other => continue,
         };

         // Parse resolution from URL (format: /vid/WIDTHxHEIGHT/ or
         // /vid/avc1/WIDTHxHEIGHT/)
         let resolution = url
            .split("/vid/")
            .nth(1)
            .and_then(|after_vid| {
               after_vid
                  .split('/')
                  .find(|part| part.contains('x'))
                  .and_then(|dims| dims.split('x').nth(1))
                  .and_then(|height| height.parse().ok())
            })
            .unwrap_or(bitrate / 1000);

         variants.push(VideoVariant {
            content_type,
            url,
            bitrate,
            resolution,
         });
      }
   }

   // Sort variants by resolution (highest first)
   variants.sort_by_key(|var| cmp::Reverse(var.resolution));

   // Check media availability via ext_media_availability
   let (available, reason) = media.ext_media_availability.as_ref().map_or_else(
      || (true, String::new()),
      |avail| {
         let status = avail.status.as_deref().unwrap_or("");
         if !status.is_empty() && !status.eq_ignore_ascii_case("available") {
            (false, avail.reason.as_deref().unwrap_or(status).to_owned())
         } else {
            (true, String::new())
         }
      },
   );

   // Parse video title and description from additional_media_info
   let title = media
      .additional_media_info
      .as_ref()
      .and_then(|info| info.title.as_deref())
      .unwrap_or_default()
      .to_owned();
   let description = media
      .additional_media_info
      .as_ref()
      .and_then(|info| info.description.as_deref())
      .unwrap_or_default()
      .to_owned();

   Video {
      duration_ms,
      url: variants
         .first()
         .map_or_else(String::new, |var| var.url.clone()),
      thumb,
      available,
      reason,
      title,
      description,
      playback_type: VideoType::Mp4,
      variants,
   }
}

/// Parse GIF from typed media item.
pub fn parse_gif(media: &MediaItem) -> Gif {
   let thumb = media
      .media_url_https
      .as_deref()
      .unwrap_or_default()
      .to_owned();

   let url = media
      .video_info
      .as_ref()
      .and_then(|vi| vi.variants.first())
      .and_then(|var| var.url.as_deref())
      .unwrap_or_default()
      .to_owned();

   Gif { url, thumb }
}
