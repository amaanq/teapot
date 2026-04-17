use std::{
   borrow::Cow,
   fmt::Write as _,
};

use maud::{
   Markup,
   PreEscaped,
   html,
};

use crate::{
   api::schema::CommunityNote,
   config::Config,
   types::{
      Prefs,
      Tweet,
      User,
      VerifiedType,
   },
   utils::{
      entity_expander::{
         expand_with_regex,
         short_url,
      },
      formatters,
      html_escape,
   },
};

const SMALL_WEBP: &str = "?name=small&format=webp";

/// Get the canonical link path for a tweet (`/{user}/status/{id}`).
pub fn tweet_link(tweet: &Tweet) -> String {
   if tweet.id == 0 {
      return String::new();
   }
   let username = if tweet.user.username.is_empty() {
      "i"
   } else {
      &tweet.user.username
   };
   format!("/{username}/status/{}", tweet.id)
}

/// Append small webp params and proxy the URL.
pub fn get_small_pic(url: &str, config: &Config) -> String {
   let url = if !url.contains('?') && !url.ends_with("placeholder.png") {
      format!("{url}{SMALL_WEBP}")
   } else {
      url.to_owned()
   };
   formatters::get_pic_url(&url, config.config.base64_media)
}

/// Render an icon inside a container div.
///
/// Produces: `<div class="icon-container"><span class="icon-{name} {class}"
/// title="{title}"></span>{text}</div>`.
///
/// When `href` is non-empty, uses an `<a>` instead of `<span>`.
pub fn icon(name: &str, text: &str, title: &str, class: &str, href: &str) -> Markup {
   let mut css_class = format!("icon-{name}");
   if !class.is_empty() {
      css_class = format!("{css_class} {class}");
   }

   html! {
       div class="icon-container" {
           @if !href.is_empty() {
               a class=(css_class) title=(title) href=(href) {}
           } @else {
               span class=(css_class) title=(title) {}
           }
           @if !text.is_empty() {
               " " (text)
           }
       }
   }
}

/// Render verified icon badge.
///
/// Produces:
/// ```html
/// <div class="verified-icon {lower}">
///   <div class="icon-container"><span class="icon-circle verified-icon-circle" title="Verified {lower} account"></span></div>
///   <div class="icon-container"><span class="icon-ok verified-icon-check" title="Verified {lower} account"></span></div>
/// </div>
/// ```
pub fn verified_icon(user: &User) -> Markup {
   if user.verified_type == VerifiedType::None {
      return html! {};
   }

   let lower = match user.verified_type {
      VerifiedType::Blue => "blue",
      VerifiedType::Business => "business",
      VerifiedType::Government => "government",
      VerifiedType::None => unreachable!(),
   };
   let title = format!("Verified {lower} account");

   html! {
       div class=(format!("verified-icon {lower}")) title=(title) {
           (PreEscaped(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 22 22" width="18" height="18" aria-hidden="true"><path class="verified-badge-bg" d="M 20.396 11 c -0.018 -0.646 -0.215 -1.275 -0.57 -1.816 c -0.354 -0.54 -0.852 -0.972 -1.438 -1.246 c 0.223 -0.607 0.27 -1.264 0.14 -1.897 c -0.131 -0.634 -0.437 -1.218 -0.882 -1.687 c -0.47 -0.445 -1.053 -0.75 -1.687 -0.882 c -0.633 -0.13 -1.29 -0.083 -1.897 0.14 c -0.273 -0.587 -0.704 -1.086 -1.245 -1.44 S 11.647 1.62 11 1.604 c -0.646 0.017 -1.273 0.213 -1.813 0.568 s -0.969 0.854 -1.24 1.44 c -0.608 -0.223 -1.267 -0.272 -1.902 -0.14 c -0.635 0.13 -1.22 0.436 -1.69 0.882 c -0.445 0.47 -0.749 1.055 -0.878 1.688 c -0.13 0.633 -0.08 1.29 0.144 1.896 c -0.587 0.274 -1.087 0.705 -1.443 1.245 c -0.356 0.54 -0.555 1.17 -0.574 1.817 c 0.02 0.647 0.218 1.276 0.574 1.817 c 0.356 0.54 0.856 0.972 1.443 1.245 c -0.224 0.606 -0.274 1.263 -0.144 1.896 c 0.13 0.634 0.433 1.218 0.877 1.688 c 0.47 0.443 1.054 0.747 1.687 0.878 c 0.633 0.132 1.29 0.084 1.897 -0.136 c 0.274 0.586 0.705 1.084 1.246 1.439 c 0.54 0.354 1.17 0.551 1.816 0.569 c 0.647 -0.016 1.276 -0.213 1.817 -0.567 s 0.972 -0.854 1.245 -1.44 c 0.604 0.239 1.266 0.296 1.903 0.164 c 0.636 -0.132 1.22 -0.447 1.68 -0.907 c 0.46 -0.46 0.776 -1.044 0.908 -1.681 s 0.075 -1.299 -0.165 -1.903 c 0.586 -0.274 1.084 -0.705 1.439 -1.246 c 0.354 -0.54 0.551 -1.17 0.569 -1.816 Z"/><path class="verified-badge-check" d="M 9.662 14.85 l -3.429 -3.428 l 1.293 -1.302 l 2.072 2.072 l 4.4 -4.794 l 1.347 1.246 Z"/></svg>"#))
       }
   }
}

/// Render a linked user.
///
/// Renders as fullname (when class doesn't contain "username") or "@username".
/// The verified icon is NOT included — callers must add `verified_icon()` as a
/// sibling after this element.
pub fn link_user(user: &User, class: &str) -> Markup {
   let is_name = !class.contains("username");
   let href = format!("/{}", user.username);
   let name_text = if is_name {
      Cow::Borrowed(user.fullname.as_str())
   } else {
      Cow::Owned(format!("@{}", user.username))
   };

   html! {
       a href=(href) class=(class) title=(&name_text) {
           (&name_text)
           @if is_name && user.protected {
               " "
               (icon("lock", "", "Protected account", "", ""))
           }
       }
   }
}

/// Render a hidden input field.
pub fn hidden_field(name: &str, value: &str) -> Markup {
   html! {
       input name=(name) style="display: none" value=(value);
   }
}

/// Render a hidden referer field.
pub fn referer_field(path: &str) -> Markup {
   hidden_field("referer", path)
}

/// Render a form button with referer.
pub fn button_referer(action: &str, text: &str, path: &str, class: &str, method: &str) -> Markup {
   let method = if method.is_empty() { "post" } else { method };

   html! {
       form method=(method) action=(action) class=(class) {
           (referer_field(path))
           button type="submit" { (text) }
       }
   }
}

/// Render a proxied image.
///
/// Produces: `<img src="{pic_url}" class="{class}" alt="" loading="lazy">`.
pub fn gen_img(url: &str, class: &str, config: &Config) -> Markup {
   let pic_url = formatters::get_pic_url(url, config.config.base64_media);
   html! {
       img src=(pic_url) class=(class) alt="" loading="lazy";
   }
}

/// Get the avatar CSS class based on user preferences.
///
/// Returns "avatar" for square avatars, "avatar round" for round (default).
pub fn get_avatar_class(prefs: Option<&Prefs>) -> &'static str {
   if prefs.is_some_and(|pref| pref.square_avatars) {
      "avatar"
   } else {
      "avatar round"
   }
}

/// Parse location string into (`place_name`, `search_url`).
/// Format: "`PlaceName`" or "`PlaceName`:`PlaceID`".
/// Used by both tweet.rs and profile.rs.
pub fn parse_location(location: &str) -> (String, String) {
   if location.contains("://") {
      return (location.to_owned(), String::new());
   }

   let parts = location.split(':').collect::<Vec<_>>();
   let place = parts[0].to_owned();
   let url = if parts.len() > 1 {
      format!("/search?f=tweets&q=place:{}", parts[1])
   } else {
      String::new()
   };
   (place, url)
}

/// Render a community note (Birdwatch).
/// Used by both `render_tweet_complete` and `render_quote` in tweet.rs.
pub fn render_community_note(note: Option<&CommunityNote>, hide_notes: bool) -> Markup {
   let Some(note) = note else {
      return html! {};
   };
   if hide_notes {
      return html! {};
   }

   // Render text with structured links into HTML
   let note_html = community_note_to_html(note);

   html! {
       div class="community-note" {
           div class="community-note-header" {
               span { "Readers added context they thought people might want to know" }
           }
           div class="community-note-text" dir="auto" {
               (maud::PreEscaped(&note_html))
           }
       }
   }
}

/// Convert a [`CommunityNote`]'s structured links into HTML. Replaces character
/// ranges with `<a>` tags, preserving the rest as plain text.
pub fn community_note_to_html(note: &CommunityNote) -> String {
   if note.links.is_empty() {
      return note.text.clone();
   }

   let mut result = String::new();
   let chars = note.text.chars().collect::<Vec<_>>();
   let mut sorted_links = note.links.clone();
   sorted_links.sort_by_key(|&(from, ..)| from);

   let mut pos = 0;
   for &(from, to, ref url) in &sorted_links {
      let to = to.min(chars.len());
      if from > pos {
         result.extend(&chars[pos..from]);
      }
      let display = chars[from..to].iter().collect::<String>();
      let url = html_escape(url);
      let display = html_escape(&display);
      let _ = write!(result, r#"<a href="{url}">{display}</a>"#);
      pos = to;
   }
   if pos < chars.len() {
      result.extend(&chars[pos..]);
   }
   result
}

/// Expand a user's bio entities and replace URLs for display.
/// Used by profile.rs and `user_list.rs`.
pub fn render_bio_html(bio: &str, config: &Config) -> String {
   if bio.is_empty() {
      return String::new();
   }
   let expanded = expand_with_regex(bio);
   formatters::replace_urls(&expanded, config)
}

/// Shorten a URL for display (remove protocol + www, no truncation).
pub fn short_link(url: &str) -> String {
   short_url(url, 0)
}
