use std::{
   borrow::Cow,
   sync::LazyLock,
};

use data_encoding::BASE64URL_NOPAD;
use percent_encoding::{
   NON_ALPHANUMERIC,
   utf8_percent_encode,
};
use regex::Regex;

use crate::config::Config;

// Pre-compiled regexes for URL replacement (twitter/x.com)
static X_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
   Regex::new(
      r#"<a href="https://(?:www\.|mobile\.)?x\.com([^"]+)">(?:www\.|mobile\.)?x\.com(\S+)</a>"#,
   )
   .unwrap()
});
static TW_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
   Regex::new(r#"<a href="https://(?:www\.|mobile\.)?twitter\.com([^"]+)">(?:www\.|mobile\.)?twitter\.com(\S+)</a>"#).unwrap()
});
static X_URL_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r"(://)(?:www\.|mobile\.)?x\.com\b").unwrap());
static TW_URL_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r"(://)(?:www\.|mobile\.)?twitter\.com\b").unwrap());
static X_BARE_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r"(^|\s)((?:www\.|mobile\.)?x\.com)\b").unwrap());
static TW_BARE_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r"(^|\s)((?:www\.|mobile\.)?twitter\.com)\b").unwrap());
// YouTube and Reddit
static YT_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r"(?i)([A-Za-z.]+\.)?youtu(be\.com|\.be)").unwrap());
static RD_RE: LazyLock<Regex> = LazyLock::new(|| {
   Regex::new(r"(?:^|(?:https://)|[\s])(?:((?:www|np|new|amp|old)\.)?reddit\.com)").unwrap()
});
static RD_SHORT_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r"(?:^|[\s])(redd\.it/)").unwrap());
// User profile pic
static USER_PIC_SIZE_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r"_(normal|bigger|mini|200x200|400x400)(\.[A-Za-z]+)$").unwrap());
static USER_PIC_EXT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\.[A-Za-z]+)$").unwrap());

/// Replace Twitter/X URLs with teapot instance URLs.
/// Handles both raw URLs (in href attributes) and HTML anchor tags (display
/// text). Avoids matching partial domains like `spacex.com` by requiring `://`
/// or word boundary before the domain.
pub fn replace_twitter_urls(text: &str, config: &Config) -> String {
   let replace_with = if config.preferences.replace_twitter.is_empty() {
      &config.server.hostname
   } else {
      &config.preferences.replace_twitter
   };
   let replace_with = replace_with.trim_end_matches('/');

   let mut result = text.to_owned();

   let prefix = config.url_prefix();

   // Replace t.co short links -> host/t.co
   if result.contains("https://t.co") {
      result = result.replace("https://t.co", &format!("{prefix}/t.co"));
   }

   // Replace cards.twitter.com/cards -> host/cards
   if result.contains("cards.twitter.com/cards") {
      result = result.replace("cards.twitter.com/cards", &format!("{replace_with}/cards"));
   }

   // Handle full <a> tags with x.com/twitter.com
   let link_replacement = format!(r#"<a href="{prefix}$1">{replace_with}$2</a>"#);
   let result = X_LINK_RE.replace_all(&result, link_replacement.as_str());
   let result = TW_LINK_RE.replace_all(&result, link_replacement.as_str());

   // Handle raw URLs: replace domain after "://" boundary.
   let url_replacement = format!("${{1}}{replace_with}");
   let result = X_URL_RE.replace_all(&result, url_replacement.as_str());
   let result = TW_URL_RE.replace_all(&result, url_replacement.as_str());

   // Handle bare domain at start of string or after whitespace (e.g., in display
   // text)
   let bare_replacement = format!("${{1}}{replace_with}");
   let result = X_BARE_RE.replace_all(&result, bare_replacement.as_str());
   let result = TW_BARE_RE.replace_all(&result, bare_replacement.as_str());

   result.to_string()
}

/// Replace `YouTube` URLs with configured replacement.
pub fn replace_youtube_urls<'a>(text: &'a str, config: &Config) -> Cow<'a, str> {
   if config.preferences.replace_youtube.is_empty() {
      return Cow::Borrowed(text);
   }

   let host = config.preferences.replace_youtube.trim_end_matches('/');
   match YT_RE.replace_all(text, host) {
      Cow::Borrowed(_) => Cow::Borrowed(text),
      Cow::Owned(owned) => Cow::Owned(owned),
   }
}

/// Replace Reddit URLs with configured replacement.
pub fn replace_reddit_urls<'a>(text: &'a str, config: &Config) -> Cow<'a, str> {
   if config.preferences.replace_reddit.is_empty() {
      return Cow::Borrowed(text);
   }

   let host = config.preferences.replace_reddit.trim_end_matches('/');
   let result = RD_SHORT_RE.replace_all(text, format!("{host}/comments/").as_str());
   let result = RD_RE.replace_all(&result, host);

   // Reddit gallery -> comments redirect
   if result.contains(host) && result.contains("/gallery/") {
      Cow::Owned(result.replace("/gallery/", "/comments/"))
   } else {
      match result {
         Cow::Borrowed(_) => Cow::Borrowed(text),
         Cow::Owned(owned) => Cow::Owned(owned),
      }
   }
}

/// Apply all URL replacements.
/// When `absolute` is non-empty, relative `href="/"` links are converted to
/// absolute `href="{absolute}/"` (used by RSS feeds).
pub fn replace_urls_abs(text: &str, config: &Config, absolute: &str) -> String {
   let text = replace_twitter_urls(text, config);
   let text = replace_youtube_urls(&text, config);
   let text = replace_reddit_urls(&text, config);

   let mut text = text.into_owned();

   // Convert relative hrefs to absolute for RSS
   if !absolute.is_empty() && text.contains("href") {
      text = text.replace("href=\"/", &format!("href=\"{absolute}/"));
   }

   text
}

/// Apply all URL replacements.
pub fn replace_urls(text: &str, config: &Config) -> String {
   replace_urls_abs(text, config, "")
}

/// Build a cursor URL with the cursor value percent-encoded.
pub fn cursor_url(base: &str, cursor: &str) -> String {
   let encoded: String = form_urlencoded::byte_serialize(cursor.as_bytes()).collect();
   format!("{base}?cursor={encoded}")
}

/// URL-encode a string.
pub fn url_encode(input: &str) -> String {
   utf8_percent_encode(input, NON_ALPHANUMERIC).to_string()
}

/// Transform a user profile pic URL to use a different size suffix.
///
/// Strips existing size suffixes (`_normal`, `_bigger`, `_mini`, `_200x200`,
/// `_400x400`) and inserts the new style before the file extension.
///
/// Example: `get_user_pic("...pic_400x400.jpg", "_mini")` ->
/// `"...pic_mini.jpg"`.
pub fn get_user_pic(user_pic: &str, style: &str) -> String {
   let without_size = USER_PIC_SIZE_RE.replace(user_pic, "$2").to_string();
   USER_PIC_EXT_RE
      .replace(&without_size, format!("{style}$1").as_str())
      .to_string()
}

/// Encode a URL for proxy path segments: base64 (with `/enc/` prefix) or
/// percent-encoded.
fn encode_media_url(prefix: &str, url: &str, base64: bool) -> String {
   if base64 {
      format!("{prefix}/enc/{}", base64_encode_url(url))
   } else {
      format!("{prefix}/{}", url_encode(url))
   }
}

/// Generate pic URL (proxied image).
pub fn get_pic_url(url: &str, base64_media: bool) -> String {
   encode_media_url("/pic", url, base64_media)
}

/// Generate original pic URL.
pub fn get_orig_pic_url(url: &str, base64_media: bool) -> String {
   encode_media_url("/pic/orig", url, base64_media)
}

/// Generate video URL with HMAC signature.
pub fn get_vid_url(url: &str, hmac_key: &str, base64_media: bool) -> String {
   let sig = super::sign(url, hmac_key);
   encode_media_url(&format!("/video/{sig}"), url, base64_media)
}

/// Generate video embed URL.
pub fn get_video_embed_url(config: &Config, tweet_id: i64) -> String {
   format!("{}/i/videos/tweet/{tweet_id}", config.url_prefix())
}

/// Generate GIF URL with HMAC signature (for local transcoding).
/// The `.gif` suffix is critical: Discord's image proxy uses the URL
/// extension to decide whether to preserve animation.
pub fn get_gif_url(mp4_url: &str, hmac_key: &str, base64_media: bool) -> String {
   let sig = super::sign(mp4_url, hmac_key);
   format!("{}.gif", encode_media_url(&format!("/gif/{sig}"), mp4_url, base64_media))
}

/// Generate external GIF URL by rewriting the domain.
pub fn get_external_gif_url(mp4_url: &str, external_domain: &str) -> String {
   mp4_url.replace("video.twimg.com", external_domain)
}

/// Base64 encode a URL (URL-safe variant).
pub fn base64_encode_url(url: &str) -> String {
   BASE64URL_NOPAD.encode(url.as_bytes())
}

/// Base64 decode a URL.
pub fn base64_decode_url(encoded: &str) -> Option<String> {
   let bytes = BASE64URL_NOPAD.decode(encoded.as_bytes()).ok()?;
   String::from_utf8(bytes).ok()
}

/// Insert comma separators into a number (e.g., 1234567 -> "1,234,567").
pub fn format_with_commas(num: i64) -> String {
   let (prefix, digits) = if num < 0 {
      ("-", num.unsigned_abs().to_string())
   } else {
      ("", num.to_string())
   };
   let len = digits.len();
   let mut result = String::with_capacity(prefix.len() + len + len / 3);
   result.push_str(prefix);
   for (idx, ch) in digits.chars().enumerate() {
      if idx > 0 && (len - idx) % 3 == 0 {
         result.push(',');
      }
      result.push(ch);
   }
   result
}

/// Abbreviate a number: 1234 → "1.2K", 1234567 → "1.23M".
#[expect(
   clippy::cast_precision_loss,
   reason = "display-only, precision loss is fine"
)]
pub fn abbreviate_number(num: i64) -> String {
   let abs = num.unsigned_abs() as f64;
   if abs >= 1_000_000.0 {
      format!("{:.2}M", abs / 1_000_000.0)
         .trim_end_matches('0')
         .trim_end_matches('.')
         .to_owned()
   } else if abs >= 1_000.0 {
      format!("{:.1}K", abs / 1_000.0)
         .trim_end_matches('0')
         .trim_end_matches('.')
         .to_owned()
   } else {
      num.to_string()
   }
}

/// Parse tweet time from Twitter format.
/// Twitter uses: "Wed Jun 01 12:00:00 +0000 2009" or "Wed Jun  1 12:00:00 +0000
/// 2009".
pub fn parse_twitter_time(time_str: &str) -> Option<time::OffsetDateTime> {
   use time::macros::format_description;

   time::OffsetDateTime::parse(
      time_str,
      format_description!(
         "[weekday repr:short] [month repr:short] [day padding:zero] [hour]:[minute]:[second] \
          [offset_hour sign:mandatory][offset_minute] [year]"
      ),
   )
   .or_else(|_| {
      time::OffsetDateTime::parse(
         time_str,
         format_description!(
            "[weekday repr:short] [month repr:short] [day padding:space] [hour]:[minute]:[second] \
             [offset_hour sign:mandatory][offset_minute] [year]"
         ),
      )
   })
   .ok()
}

/// Format tweet time for display: "Feb 7, 2026 · 4:48 AM UTC".
pub fn format_tweet_time(time: time::OffsetDateTime) -> String {
   use time::macros::format_description;

   let month_day_year = time
      .format(format_description!(
         "[month repr:short] [day padding:none], [year]"
      ))
      .unwrap_or_default();
   let hour_24 = time.hour();
   let (hour_12, period) = match hour_24 {
      0 => (12, "AM"),
      1..=11 => (hour_24, "AM"),
      12 => (12, "PM"),
      _ => (hour_24 - 12, "PM"),
   };
   format!(
      "{month_day_year} · {hour_12}:{:02} {period} UTC",
      time.minute()
   )
}

/// Format time in RFC822 format for RSS feeds.
pub fn format_rfc822_time(time: time::OffsetDateTime) -> String {
   use time::macros::format_description;

   time
      .format(format_description!(
         "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"
      ))
      .unwrap_or_default()
}

/// Format relative time for compact display:
/// - < 1 min: "now"
/// - < 60 min: "Xm"
/// - < 24 hours: "Xh"
/// - >= 1 day, same year: "MMM d" (e.g. "Feb 3")
/// - different year: "d MMM yyyy" (e.g. "3 Feb 2024")
pub fn format_relative_time(time: time::OffsetDateTime) -> String {
   use time::macros::format_description;

   let now = time::OffsetDateTime::now_utc();
   let duration = now - time;

   if now.year() != time.year() {
      time
         .format(format_description!(
            "[day padding:none] [month repr:short] [year]"
         ))
         .unwrap_or_default()
   } else if duration.whole_hours() >= 24 {
      time
         .format(format_description!("[month repr:short] [day padding:none]"))
         .unwrap_or_default()
   } else if duration.whole_hours() >= 1 {
      format!("{}h", duration.whole_hours())
   } else if duration.whole_minutes() >= 1 {
      format!("{}m", duration.whole_minutes())
   } else if duration.whole_seconds() >= 1 {
      format!("{}s", duration.whole_seconds())
   } else {
      "now".to_owned()
   }
}

/// Format join date for profile display (e.g., "Joined March 2020").
pub fn format_join_date(time: time::OffsetDateTime) -> String {
   use time::macros::format_description;

   format!(
      "Joined {}",
      time
         .format(format_description!("[month repr:long] [year]"))
         .unwrap_or_default()
   )
}

/// Scale video dimensions for Discord embed compatibility.
/// Discord won't render videos >1920p and renders <400p too small.
pub const fn scale_dimensions_for_embed(width: i32, height: i32) -> (i32, i32) {
   if width > 1920 || height > 1920 {
      (width / 2, height / 2)
   } else if width < 400 && height < 400 {
      (width * 2, height * 2)
   } else {
      (width, height)
   }
}

/// Format engagement metrics for oEmbed `author_name` field.
pub fn format_engagement_text(
   likes: i64,
   retweets: i64,
   replies: i64,
   views: i64,
) -> String {
   let mut parts = Vec::new();
   if replies > 0 {
      parts.push(format!("\u{1F4AC} {}", abbreviate_number(replies)));
   }
   if retweets > 0 {
      parts.push(format!("\u{1F501} {}", abbreviate_number(retweets)));
   }
   if likes > 0 {
      parts.push(format!("\u{2764}\u{FE0F} {}", abbreviate_number(likes)));
   }
   if views > 0 {
      parts.push(format!("\u{1F441}\u{FE0F} {}", abbreviate_number(views)));
   }
   if parts.is_empty() {
      String::new()
   } else {
      parts.join("   ")
   }
}

/// Format video duration from milliseconds to "MM:SS" or "HH:MM:SS".
#[expect(
   clippy::modulo_arithmetic,
   reason = "inputs are always non-negative milliseconds"
)]
pub fn format_duration(ms: i32) -> String {
   let total_seconds = ms / 1000;
   let hours = total_seconds / 3600;
   let minutes = (total_seconds % 3600) / 60;
   let seconds = total_seconds % 60;

   if hours > 0 {
      format!("{hours}:{minutes:02}:{seconds:02}")
   } else {
      format!("{minutes}:{seconds:02}")
   }
}

/// Strip illegal XML 1.0 characters. Valid XML 1.0 chars: #x9 | #xA | #xD |
/// [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF].
pub fn sanitize_xml(text: &str) -> String {
   text
      .chars()
      .filter(|&ch| {
         matches!(ch,
            '\x09' | '\x0A' | '\x0D' |
            '\u{0020}'..='\u{D7FF}' |
            '\u{E000}'..='\u{FFFD}' |
            '\u{10000}'..='\u{10FFFF}'
         )
      })
      .collect()
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_parse_twitter_time_zero_padded_day() {
      use time::macros::format_description;

      // Elon Musk's created_at: zero-padded day
      let result = parse_twitter_time("Tue Jun 02 20:12:29 +0000 2009");
      assert!(result.is_some(), "Failed to parse zero-padded day");
      let dt = result.unwrap();
      assert_eq!(
         dt.format(format_description!("[month repr:long] [year]"))
            .unwrap(),
         "June 2009"
      );
   }

   #[test]
   fn test_parse_twitter_time_space_padded_day() {
      use time::macros::format_description;

      // Space-padded day (single digit)
      let result = parse_twitter_time("Tue Jun  2 20:12:29 +0000 2009");
      assert!(result.is_some(), "Failed to parse space-padded day");
      let dt = result.unwrap();
      assert_eq!(
         dt.format(format_description!("[month repr:long] [year]"))
            .unwrap(),
         "June 2009"
      );
   }

   #[test]
   fn test_format_join_date() {
      let dt = parse_twitter_time("Tue Jun 02 20:12:29 +0000 2009").unwrap();
      assert_eq!(format_join_date(dt), "Joined June 2009");
   }
}
