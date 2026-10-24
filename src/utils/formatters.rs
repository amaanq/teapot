use std::{
   borrow::Cow,
   fmt::Write as _,
   iter,
};

use data_encoding::BASE64URL_NOPAD;
use jiff::{
   Timestamp,
   tz::TimeZone,
};
use percent_encoding::{
   NON_ALPHANUMERIC,
   utf8_percent_encode,
};
use unicode_ident::is_xid_continue;

use crate::config::Config;

const USER_PIC_SIZES: [&str; 5] = ["normal", "bigger", "mini", "200x200", "400x400"];
const REDDIT_SUBDOMAINS: [&str; 5] = ["www.", "np.", "new.", "amp.", "old."];
const KELVIN_SIGN: char = '\u{212A}';
const LONG_S: char = '\u{017F}';

/// Unicode `\w`, the regex crate's word class. `XID_Continue` covers the
/// marks, `Nd`, `Pc` and `Join_Control` except the enclosing marks, and adds
/// `Other_ID_Start` and `Other_ID_Continue` symbols that are not word
/// characters.
pub fn is_word_char(ch: char) -> bool {
   let enclosing_mark = matches!(
      ch,
      '\u{0488}' | '\u{0489}' | '\u{1ABE}' | '\u{20E2}' | '\u{20E3}' | '\u{20E4}' | '\u{A670}'
   ) || ('\u{20DD}'..='\u{20E0}').contains(&ch)
      || ('\u{A671}'..='\u{A672}').contains(&ch);
   let symbol = matches!(
      ch,
      '\u{00B7}' | '\u{0387}' | '\u{19DA}' | '\u{2118}' | '\u{212E}' | '\u{30FB}' | '\u{FF65}'
   ) || ('\u{1369}'..='\u{1371}').contains(&ch);
   enclosing_mark || (ch.is_alphabetic() || is_xid_continue(ch)) && !symbol
}

pub fn find_matches<'a, M, F>(
   text: &'a str,
   match_at: F,
) -> impl Iterator<Item = (usize, usize, M)> + 'a
where
   F: Fn(usize) -> Option<(usize, M)> + 'a,
{
   let mut pos = 0;
   iter::from_fn(move || {
      while let Some(ch) = text[pos..].chars().next() {
         if let Some((end, found)) = match_at(pos) {
            let start = pos;
            pos = end;
            return Some((start, end, found));
         }
         pos += ch.len_utf8();
      }
      None
   })
}

pub fn replace_matches<'a, M, F, R>(text: &'a str, match_at: F, mut render: R) -> Cow<'a, str>
where
   F: Fn(usize) -> Option<(usize, M)> + 'a,
   R: FnMut(&mut String, M),
{
   let mut replaced = None::<String>;
   let mut copied = 0;

   for (start, end, found) in find_matches(text, match_at) {
      let out = replaced.get_or_insert_with(|| String::with_capacity(text.len()));
      out.push_str(&text[copied..start]);
      render(out, found);
      copied = end;
   }

   replaced.map_or(Cow::Borrowed(text), |mut out| {
      out.push_str(&text[copied..]);
      Cow::Owned(out)
   })
}

pub fn match_leads<F>(text: &str, pos: usize, is_lead: F) -> impl Iterator<Item = (&str, usize)>
where
   F: Fn(char) -> bool,
{
   let start_of_text = (pos == 0).then_some(("", 0));
   let lead = text[pos..]
      .chars()
      .next()
      .filter(|&ch| is_lead(ch))
      .map(|ch| (&text[pos..pos + ch.len_utf8()], pos + ch.len_utf8()));
   start_of_text.into_iter().chain(lead)
}

fn subdomain_len(text: &str) -> usize {
   ["www.", "mobile."]
      .into_iter()
      .find(|sub| text.starts_with(sub))
      .map_or(0, str::len)
}

fn host_end(text: &str, pos: usize, domain: &str) -> Option<usize> {
   let host = pos + subdomain_len(&text[pos..]);
   let end = host + domain.len();
   let bounded =
      text[host..].starts_with(domain) && !text[end..].chars().next().is_some_and(is_word_char);
   bounded.then_some(end)
}

fn domain_link_at<'a>(
   text: &'a str,
   pos: usize,
   domain: &str,
) -> Option<(usize, (&'a str, &'a str))> {
   let href = text[pos..].strip_prefix(r#"<a href="https://"#)?;
   let path_and_rest = href[subdomain_len(href)..].strip_prefix(domain)?;
   let path_len = path_and_rest.find('"').filter(|&len| len > 0)?;
   let display = path_and_rest[path_len..].strip_prefix(r#"">"#)?;
   let shown_and_rest = display[subdomain_len(display)..].strip_prefix(domain)?;
   let run = shown_and_rest
      .find(char::is_whitespace)
      .unwrap_or(shown_and_rest.len());
   let shown_len = shown_and_rest[..run].rfind("</a>").filter(|&len| len > 0)?;
   let end = text.len() - shown_and_rest.len() + shown_len + "</a>".len();
   Some((
      end,
      (&path_and_rest[..path_len], &shown_and_rest[..shown_len]),
   ))
}

fn youtube_at(text: &str, pos: usize) -> Option<usize> {
   let youtu_end = |at: usize| {
      let suffix_start = at + "youtu".len();
      text
         .get(at..suffix_start)
         .filter(|head| head.eq_ignore_ascii_case("youtu"))?;
      ["be.com", ".be"].into_iter().find_map(|suffix| {
         let end = suffix_start + suffix.len();
         text
            .get(suffix_start..end)
            .filter(|found| found.eq_ignore_ascii_case(suffix))
            .map(|_| end)
      })
   };
   let run = text[pos..]
      .find(|ch: char| !(ch.is_ascii_alphabetic() || matches!(ch, '.' | KELVIN_SIGN | LONG_S)))
      .map_or(text.len(), |len| pos + len);

   text[pos..run]
      .rmatch_indices('.')
      .map(|(offset, _)| pos + offset)
      .filter(|&dot| dot > pos)
      .find_map(|dot| youtu_end(dot + 1))
      .or_else(|| youtu_end(pos))
}

/// The end of a Reddit domain at `pos`, and whether it is the `redd.it`
/// short form. The domain must follow whitespace or a scheme, so the scheme
/// and the space before it stay in place.
fn reddit_at(text: &str, pos: usize) -> Option<(usize, bool)> {
   let head = &text[..pos];
   if head
      .chars()
      .next_back()
      .is_some_and(|ch| !ch.is_whitespace())
      && !head.ends_with("://")
   {
      return None;
   }
   let rest = &text[pos..];
   if let Some(after) = rest.strip_prefix("redd.it/") {
      return Some((text.len() - after.len(), true));
   }
   let domain = REDDIT_SUBDOMAINS
      .into_iter()
      .find_map(|sub| rest.strip_prefix(sub))
      .unwrap_or(rest);
   let after = domain
      .strip_prefix("reddit.com")
      .filter(|after| !after.starts_with(is_word_char))?;
   Some((text.len() - after.len(), false))
}

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

   if result.contains("https://t.co") {
      result = result.replace("https://t.co", &format!("{prefix}/t.co"));
   }

   if result.contains("cards.twitter.com/cards") {
      result = result.replace("cards.twitter.com/cards", &format!("{replace_with}/cards"));
   }

   // Handle full <a> tags with x.com/twitter.com
   let linked = ["x.com", "twitter.com"]
      .into_iter()
      .fold(result, |current, domain| {
         replace_matches(
            &current,
            |pos| domain_link_at(&current, pos, domain),
            |out, (path, shown)| {
               let _ = write!(out, r#"<a href="{prefix}{path}">{replace_with}{shown}</a>"#);
            },
         )
         .into_owned()
      });

   let schemed = ["x.com", "twitter.com"]
      .into_iter()
      .fold(linked, |current, domain| {
         replace_matches(
            &current,
            |pos| {
               current[pos..]
                  .starts_with("://")
                  .then(|| host_end(&current, pos + "://".len(), domain))
                  .flatten()
                  .map(|end| (end, ()))
            },
            |out, ()| {
               out.push_str("://");
               out.push_str(replace_with);
            },
         )
         .into_owned()
      });

   ["x.com", "twitter.com"]
      .into_iter()
      .fold(schemed, |current, domain| {
         replace_matches(
            &current,
            |pos| {
               match_leads(&current, pos, char::is_whitespace)
                  .find_map(|(lead, host)| Some((host_end(&current, host, domain)?, lead)))
            },
            |out, lead| {
               out.push_str(lead);
               out.push_str(replace_with);
            },
         )
         .into_owned()
      })
}

/// Replace `YouTube` URLs with configured replacement.
pub fn replace_youtube_urls<'a>(text: &'a str, config: &Config) -> Cow<'a, str> {
   if config.preferences.replace_youtube.is_empty() {
      return Cow::Borrowed(text);
   }

   let host = config.preferences.replace_youtube.trim_end_matches('/');
   replace_matches(
      text,
      |pos| youtube_at(text, pos).map(|end| (end, ())),
      |out, ()| out.push_str(host),
   )
}

/// Replace Reddit URLs with configured replacement.
pub fn replace_reddit_urls<'a>(text: &'a str, config: &Config) -> Cow<'a, str> {
   if config.preferences.replace_reddit.is_empty() {
      return Cow::Borrowed(text);
   }

   let host = config.preferences.replace_reddit.trim_end_matches('/');
   let replaced = replace_matches(
      text,
      |pos| reddit_at(text, pos),
      |out, short| {
         out.push_str(host);
         if short {
            out.push_str("/comments/");
         }
      },
   );

   // Reddit gallery -> comments redirect
   if replaced.contains(host) && replaced.contains("/gallery/") {
      Cow::Owned(replaced.replace("/gallery/", "/comments/"))
   } else {
      replaced
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
   let encoded = form_urlencoded::byte_serialize(cursor.as_bytes()).collect::<String>();
   let separator = if base.contains('?') { '&' } else { '?' };
   format!("{base}{separator}cursor={encoded}")
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
   let Some(dot) = user_pic.rfind('.').filter(|&dot| {
      let ext = &user_pic[dot + 1..];
      !ext.is_empty() && ext.bytes().all(|byte| byte.is_ascii_alphabetic())
   }) else {
      return user_pic.to_owned();
   };
   let (stem, ext) = user_pic.split_at(dot);
   let base = USER_PIC_SIZES
      .into_iter()
      .find_map(|size| stem.strip_suffix(size)?.strip_suffix('_'))
      .unwrap_or(stem);
   format!("{base}{style}{ext}")
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
   let sig = super::hmac::sign(url, hmac_key);
   encode_media_url(&format!("/video/{sig}"), url, base64_media)
}

/// Generate video embed URL.
pub fn get_video_embed_url(config: &Config, tweet_id: i64) -> String {
   format!("{}/i/videos/tweet/{tweet_id}", config.url_prefix())
}

/// Generate GIF URL with HMAC signature (for local transcoding).
/// The `.gif` suffix is critical because Discord's image proxy uses the URL
/// extension to decide whether to preserve animation.
pub fn get_gif_url(mp4_url: &str, hmac_key: &str, base64_media: bool) -> String {
   let sig = super::hmac::sign(mp4_url, hmac_key);
   format!(
      "{}.gif",
      encode_media_url(&format!("/gif/{sig}"), mp4_url, base64_media)
   )
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

/// Abbreviate a number using `FxTwitter`'s fixed precision.
#[expect(
   clippy::cast_precision_loss,
   reason = "display-only, precision loss is fine"
)]
pub fn abbreviate_number(num: i64) -> String {
   let abs = num.unsigned_abs() as f64;
   if abs >= 1_000_000.0 {
      format!("{:.2}M", abs / 1_000_000.0)
   } else if abs >= 1_000.0 {
      format!("{:.1}K", abs / 1_000.0)
   } else {
      num.to_string()
   }
}

/// Parse tweet time from Twitter format.
/// Twitter uses: "Wed Jun 01 12:00:00 +0000 2009" or "Wed Jun  1 12:00:00 +0000
/// 2009".
pub fn parse_twitter_time(time_str: &str) -> Option<Timestamp> {
   Timestamp::strptime("%a %b %d %H:%M:%S %z %Y", time_str).ok()
}

/// Format tweet time for display: "Feb 7, 2026 · 4:48 AM UTC".
pub fn format_tweet_time(time: Timestamp) -> String {
   time.strftime("%b %-d, %Y · %-I:%M %p UTC").to_string()
}

/// Format time in RFC822 format for RSS feeds.
pub fn format_rfc822_time(time: Timestamp) -> String {
   time.strftime("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Format relative time for compact display:
/// - < 1 min: "now"
/// - < 60 min: "Xm"
/// - < 24 hours: "Xh"
/// - >= 1 day, same year: "MMM d" (e.g. "Feb 3")
/// - different year: "d MMM yyyy" (e.g. "3 Feb 2024")
pub fn format_relative_time(time: Timestamp) -> String {
   let now = Timestamp::now();
   let duration = now.duration_since(time);

   if TimeZone::UTC.to_datetime(now).year() != TimeZone::UTC.to_datetime(time).year() {
      time.strftime("%-d %b %Y").to_string()
   } else if duration.as_hours() >= 24 {
      time.strftime("%b %-d").to_string()
   } else if duration.as_hours() >= 1 {
      format!("{}h", duration.as_hours())
   } else if duration.as_mins() >= 1 {
      format!("{}m", duration.as_mins())
   } else if duration.as_secs() >= 1 {
      format!("{}s", duration.as_secs())
   } else {
      "now".to_owned()
   }
}

/// Format join date for profile display (e.g., "Joined March 2020").
pub fn format_join_date(time: Timestamp) -> String {
   time.strftime("Joined %B %Y").to_string()
}

/// Scale video dimensions for Discord embed compatibility.
/// Discord won't render videos >1920p and renders <400p too small.
pub const fn scale_dimensions_for_embed(width: i32, height: i32) -> (i32, i32) {
   if width > 1920 || height > 1920 {
      (width / 2, height / 2)
   } else if width < 400 || height < 400 {
      (width * 2, height * 2)
   } else {
      (width, height)
   }
}

/// Format engagement metrics for oEmbed `author_name` field.
pub fn format_engagement_text(likes: i64, retweets: i64, replies: i64, views: i64) -> String {
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
   fn parses_twitter_time_with_zero_padded_day() {
      // Elon Musk's created_at uses a zero-padded day
      let result = parse_twitter_time("Tue Jun 02 20:12:29 +0000 2009");
      assert!(result.is_some(), "Failed to parse zero-padded day");
      let dt = result.unwrap();
      assert_eq!(dt.strftime("%B %Y").to_string(), "June 2009");
   }

   #[test]
   fn parses_twitter_time_with_space_padded_day() {
      let result = parse_twitter_time("Tue Jun  2 20:12:29 +0000 2009");
      assert!(result.is_some(), "Failed to parse space-padded day");
      let dt = result.unwrap();
      assert_eq!(dt.strftime("%B %Y").to_string(), "June 2009");
   }

   #[test]
   fn formats_join_date() {
      let dt = parse_twitter_time("Tue Jun 02 20:12:29 +0000 2009").unwrap();
      assert_eq!(format_join_date(dt), "Joined June 2009");
   }

   #[test]
   fn abbreviate_number_keeps_fxtwitter_precision() {
      assert_eq!(abbreviate_number(496_000), "496.0K");
      assert_eq!(abbreviate_number(1_000_000), "1.00M");
   }

   #[test]
   fn narrow_video_dimensions_match_fxtwitter_scaling() {
      assert_eq!(scale_dimensions_for_embed(320, 480), (640, 960));
      assert_eq!(scale_dimensions_for_embed(3840, 2160), (1920, 1080));
   }
}
