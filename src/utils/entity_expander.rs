//! Entity expansion for tweet and user bio text.
//!
//! Converts @mentions, #hashtags, and URLs into clickable HTML links.
//!
//! Twitter's API returns `full_text` with HTML entities already escaped
//! (`&amp;`, `&lt;`, `&gt;`, etc.). Text between entities is passed through
//! as-is since it's already HTML-safe. Only entity *attributes* (URLs,
//! display names) need escaping because they come from our own processing.

use std::{
   borrow::Cow,
   fmt::Write as _,
   sync::LazyLock,
};

use regex::Regex;

use crate::types::{
   Entity,
   EntityKind,
};

static MENTION_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new("(^|[^A-Za-z0-9_])@([A-Za-z0-9_]{1,15})").unwrap());
static HASHTAG_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r"(^|[^\w&;])([#$])(\w+)").unwrap());

/// Convert character index to byte index.
/// Twitter entities use character (code point) indices, not byte indices.
fn char_to_byte_index(text: &str, char_idx: usize) -> Option<usize> {
   text
      .char_indices()
      .nth(char_idx)
      .map(|(byte_idx, _)| byte_idx)
      .or_else(|| {
         // If char_idx equals the number of chars, return the byte length
         (char_idx == text.chars().count()).then_some(text.len())
      })
}

/// Expand entities in text to HTML links.
///
/// Takes the original text and a list of entities with their positions,
/// then reconstructs the text with HTML anchor tags. Text between entities
/// is passed through verbatim (Twitter already HTML-escapes it).
pub fn expand_entities(text: &str, entities: &[Entity]) -> String {
   if entities.is_empty() {
      return text.to_owned();
   }

   // Sort entities by start position
   let mut sorted_entities: Vec<&Entity> = entities.iter().collect();
   sorted_entities.sort_by_key(|ent| ent.indices.0);

   // Deduplicate entities at same position
   sorted_entities.dedup_by(|ent_a, ent_b| ent_a.indices.0 == ent_b.indices.0);

   let mut result = String::with_capacity(text.len() * 2);
   let mut last_end_char = 0_usize;

   let char_count = text.chars().count();

   for entity in sorted_entities {
      let (start_char, end_char) = entity.indices;

      // Validate character indices
      if start_char > char_count || end_char > char_count || start_char >= end_char {
         continue;
      }

      // Convert character indices to byte indices
      let Some(start_byte) = char_to_byte_index(text, start_char) else {
         continue;
      };
      let Some(end_byte) = char_to_byte_index(text, end_char) else {
         continue;
      };
      let Some(last_end_byte) = char_to_byte_index(text, last_end_char) else {
         continue;
      };

      // Add text before this entity (already HTML-escaped by Twitter)
      if start_char > last_end_char {
         result.push_str(&text[last_end_byte..start_byte]);
      }

      // Add the entity as an HTML link
      let entity_text = &text[start_byte..end_byte];
      match entity.kind {
         EntityKind::Mention => {
            let _ = write!(
               result,
               r#"<a href="{}" title="{}">{}</a>"#,
               html_escape(&entity.url),
               html_escape(&entity.display),
               entity_text
            );
         },
         EntityKind::Hashtag => {
            let tag_name = entity_text.trim_start_matches('#').trim_start_matches('$');
            let _ = write!(
               result,
               r#"<a href="/search?q=%23{}">{}</a>"#,
               url_encode(tag_name),
               entity_text
            );
         },
         EntityKind::Symbol => {
            let symbol_name = entity_text.trim_start_matches('$');
            let _ = write!(
               result,
               r#"<a href="/search?q=%24{}">{}</a>"#,
               url_encode(symbol_name),
               entity_text
            );
         },
         EntityKind::Url => {
            let display = if entity.display.is_empty() {
               short_url(&entity.url, 28)
            } else {
               entity.display.clone()
            };
            let _ = write!(
               result,
               r#"<a href="{}">{}</a>"#,
               html_escape(&entity.url),
               html_escape(&display)
            );
         },
      }

      last_end_char = end_char;
   }

   // Add remaining text (already HTML-escaped by Twitter)
   if last_end_char < char_count
      && let Some(last_end_byte) = char_to_byte_index(text, last_end_char)
   {
      result.push_str(&text[last_end_byte..]);
   }

   result
}

/// Regex-based expansion fallback.
///
/// Operates directly on Twitter's pre-escaped text. The `HASHTAG_RE` pattern
/// uses `[^\w&;]` to avoid matching HTML entities like `&#x27;`.
pub fn expand_with_regex(text: &str) -> String {
   let result = MENTION_RE.replace_all(text, r#"$1<a href="/$2">@$2</a>"#);
   let result = HASHTAG_RE.replace_all(&result, r#"$1<a href="/search?q=%23$3">$2$3</a>"#);
   result.into_owned()
}

/// HTML escape special characters.
pub fn html_escape(text: &str) -> Cow<'_, str> {
   // Fast path: check if any escaping is needed
   if !text
      .bytes()
      .any(|byte| matches!(byte, b'&' | b'<' | b'>' | b'"' | b'\''))
   {
      return Cow::Borrowed(text);
   }
   // Slow path: build escaped string
   let mut result = String::with_capacity(text.len() + text.len() / 8);
   for ch in text.chars() {
      match ch {
         '&' => result.push_str("&amp;"),
         '<' => result.push_str("&lt;"),
         '>' => result.push_str("&gt;"),
         '"' => result.push_str("&quot;"),
         '\'' => result.push_str("&#x27;"),
         _ => result.push(ch),
      }
   }
   Cow::Owned(result)
}

/// URL encode a string (delegates to formatters).
fn url_encode(text: &str) -> String {
   super::formatters::url_encode(text)
}

/// Strip protocol and `www.` from a URL, optionally truncating to `max_len`
/// characters with a Unicode ellipsis.
///
/// - `max_len == 0` → no truncation (profile website links)
/// - `max_len == 28` → standard short-link behaviour (entity display text)
pub fn short_url(url: &str, max_len: usize) -> String {
   let stripped = url
      .trim_start_matches("https://")
      .trim_start_matches("http://")
      .trim_start_matches("www.");

   if max_len == 0 {
      return stripped.to_owned();
   }

   let truncated: String = stripped.chars().take(max_len).collect();
   if stripped.chars().count() > max_len {
      format!("{truncated}\u{2026}")
   } else {
      truncated
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_expand_mention() {
      let entities = vec![Entity {
         indices: (0, 5),
         kind:    EntityKind::Mention,
         url:     "/test".to_string(),
         display: "Test User".to_string(),
      }];
      let result = expand_entities("@test hello", &entities);
      assert!(result.contains(r#"<a href="/test""#));
      assert!(result.contains("@test"));
   }

   #[test]
   fn test_expand_hashtag() {
      let entities = vec![Entity {
         indices: (0, 5),
         kind:    EntityKind::Hashtag,
         url:     String::new(),
         display: String::new(),
      }];
      let result = expand_entities("#rust is great", &entities);
      assert!(result.contains(r#"href="/search?q=%23rust""#));
   }

   #[test]
   fn test_expand_url() {
      let entities = vec![Entity {
         indices: (6, 26),
         kind: EntityKind::Url,
         url: "https://example.com/long/path".to_string(),
         ..Default::default()
      }];
      let result = expand_entities("Check https://t.co/xyz out!", &entities);
      assert!(result.contains(r#"href="https://example.com/long/path""#));
      assert!(result.contains("example.com/long/path"));
   }

   #[test]
   fn test_html_escape() {
      let result = html_escape("<script>alert('xss')</script>");
      assert!(!result.contains('<'));
      assert!(!result.contains('>'));
      assert!(result.contains("&lt;"));
      assert!(result.contains("&gt;"));
   }

   #[test]
   fn test_regex_fallback() {
      let result = expand_with_regex("Hello @user and #topic!");
      assert!(result.contains(r#"<a href="/user">@user</a>"#));
      assert!(result.contains(r#"<a href="/search?q=%23topic">#topic</a>"#));
   }

   #[test]
   fn test_html_entity_not_treated_as_hashtag() {
      // &#x27; is the HTML entity for single quote — the #x27 must NOT become
      // a hashtag link. Twitter sends this pre-escaped.
      let result = expand_with_regex("It&#x27;s a test");
      assert!(
         result.contains("&#x27;"),
         "HTML entity &#x27; should be preserved"
      );
      assert!(
         !result.contains(r#"<a href="/search?q=%23x27">"#),
         "&#x27; should not be treated as hashtag #x27"
      );
   }

   #[test]
   fn test_html_entity_ampersand_not_treated_as_hashtag() {
      let result = expand_with_regex("Test &amp; &#x27;end");
      assert!(
         !result.contains(r#"<a href="/search?q=%23x27">"#),
         "&amp;#x27; should not produce hashtag link"
      );
   }

   #[test]
   fn test_no_double_escape() {
      // Twitter returns &lt; in full_text — should stay as &lt; not become &amp;lt;
      let result = expand_entities("x &lt; y", &[]);
      assert_eq!(result, "x &lt; y");
      assert!(!result.contains("&amp;"), "should not double-escape");
   }

   #[test]
   fn test_passthrough_preserves_twitter_escaping() {
      // Twitter pre-escapes: &amp; &lt; &gt; &quot;
      let result = expand_entities("A &amp; B &lt; C &gt; D", &[]);
      assert_eq!(result, "A &amp; B &lt; C &gt; D");
   }
}
