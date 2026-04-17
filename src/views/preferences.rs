use maud::{
   Markup,
   PreEscaped,
   html,
};

use crate::types::Prefs;

/// Render a checkbox preference row with title tooltip.
fn gen_checkbox(name: &str, label: &str, checked: bool) -> Markup {
   let id = format!("pref-{name}");
   html! {
       label class="pref-group checkbox-container" title=(name) for=(id) {
           (label)
           input id=(id) type="checkbox" name=(name) checked[checked];
           span class="checkbox" {}
       }
   }
}

/// Render a text input preference row with title tooltip.
fn gen_input(name: &str, label: &str, value: &str, placeholder: &str) -> Markup {
   let id = format!("pref-{name}");
   html! {
       div class="pref-group pref-input" title=(name) {
           label for=(id) { (label) }
           input id=(id) type="text" name=(name) placeholder=(placeholder) value=(value);
       }
   }
}

/// Render the font size select with live preview on change.
fn gen_font_size_select(selected: &str) -> Markup {
   let options: &[(&str, &str, &str)] = &[
      ("", "Default", ""),
      ("Small", "Small (14px)", "14px"),
      ("Medium", "Medium (16px)", "16px"),
      ("Large", "Large (18px)", "18px"),
      ("X-Large", "X-Large (20px)", "20px"),
   ];
   html! {
       div class="pref-group pref-input" title="fontSize" {
           label for="pref-fontSize" { "Font size" }
           select id="pref-fontSize" name="fontSize"
               onchange="document.body.style.fontSize=this.selectedOptions[0].dataset.px||''" {
               @for &(value, display, px) in options {
                   @let is_selected = value.eq_ignore_ascii_case(selected)
                       || (value.is_empty() && selected.is_empty());
                   option value=(value) data-px=(px) selected[is_selected] { (display) }
               }
           }
       }
   }
}

/// Render a select preference row with title tooltip.
/// `options` is a list of `(value, display_label)` pairs.
fn gen_select(name: &str, label: &str, selected: &str, options: &[(&str, &str)]) -> Markup {
   let id = format!("pref-{name}");
   html! {
       div class="pref-group pref-input" title=(name) {
           label for=(id) { (label) }
           select id=(id) name=(name) {
               @for &(value, display) in options {
                   @let is_selected = value.eq_ignore_ascii_case(selected)
                       || value.to_lowercase().replace(' ', "_") == selected.to_lowercase();
                   option value=(value) selected[is_selected] { (display) }
               }
           }
       }
   }
}

/// Render the preferences/settings form.
pub fn render_preferences_form(
   prefs: &Prefs,
   themes: &[String],
   referer: &str,
   prefs_url: &str,
) -> Markup {
   html! {
       div class="overlay-panel preferences-panel" {
           fieldset class="preferences" {
               // Close button for animated panel
               a href=(referer) class="preferences-close" {
                   span class="icon-close" {}
               }

               // Header with animation
               div class="preferences-header" {
                   h2 { "Preferences" }
                   p { "Customize your teapot experience" }
               }

               form method="post" action="/saveprefs" autocomplete="off" class="preferences-form" id="saveprefs-form" {
                   input type="hidden" name="referer" value=(referer);

                   // Display section
                   div class="pref-section" data-section="display" {
                       legend { "Display" }
                       div class="pref-grid" {
                           (gen_select("theme", "Theme", &prefs.theme, &themes.iter().map(|t| (t.as_str(), t.as_str())).collect::<Vec<_>>()))
                           (gen_checkbox("infiniteScroll", "Infinite scrolling (experimental, requires JavaScript)", prefs.infinite_scroll))
                           (gen_checkbox("stickyProfile", "Make profile sidebar stick to top", prefs.sticky_profile))
                           (gen_checkbox("stickyNav", "Keep navbar fixed to top", prefs.sticky_nav))
                           (gen_checkbox("bidiSupport", "Support bidirectional text", prefs.bidi_support))
                           (gen_checkbox("hideTweetStats", "Hide tweet stats", prefs.hide_tweet_stats))
                           (gen_checkbox("hideBanner", "Hide profile banner", prefs.hide_banner))
                           (gen_checkbox("hidePins", "Hide pinned tweets", prefs.hide_pins))
                           (gen_checkbox("hideReplies", "Hide tweet replies", prefs.hide_replies))
                           (gen_checkbox("hideCommunityNotes", "Hide community notes", prefs.hide_community_notes))
                           (gen_checkbox("squareAvatars", "Square profile pictures", prefs.square_avatars))
                           (gen_checkbox("useTwemoji", "Use Twitter emoji (Twemoji)", prefs.use_twemoji))
                       }
                   }

                   // Media section
                   div class="pref-section" data-section="media" {
                       legend { "Media" }
                       div class="pref-grid" {
                           (gen_checkbox("mp4Playback", "Enable mp4 video playback", prefs.mp4_playback))
                           (gen_checkbox("muteVideos", "Mute videos by default", prefs.mute_videos))
                           (gen_checkbox("autoplayGifs", "Autoplay gifs", prefs.autoplay_gifs))
                       }
                   }

                   // Link replacements section
                   div class="pref-section" data-section="links" {
                       legend { "Link replacements (blank to disable)" }
                       (gen_input("replaceTwitter", "Twitter -> teapot", &prefs.replace_twitter, "teapot hostname"))
                       (gen_input("replaceYouTube", "YouTube -> Piped/Invidious", &prefs.replace_youtube, "Piped hostname"))
                       (gen_input("replaceReddit", "Reddit -> Teddit/Libreddit", &prefs.replace_reddit, "Teddit hostname"))
                   }

                   // Translation section
                   div class="pref-section" data-section="translation" {
                       legend { "Translation" }
                       (gen_input("kagiToken", "Kagi session token", &prefs.kagi_token, "Paste session token here"))
                       p class="bookmark-note" {
                           "Uses Kagi Translate instead of Twitter. "
                           a href="https://kagi.com/settings?p=user_details" target="_blank" { "Get your token here" }
                           " (Session Link → copy token)"
                       }
                   }

                   // Bookmark section
                   div class="pref-section pref-section-bookmark" data-section="bookmark" {
                       legend { "Bookmark" }
                       p class="bookmark-note" {
                           "Save this URL to restore your preferences (?prefs works on all pages)"
                       }
                       div class="prefs-code-wrapper" {
                           pre class="prefs-code" {
                               code { (prefs_url) }
                           }
                           button type="button" class="copy-prefs-btn" data-clipboard=(prefs_url) {
                               span class="icon-copy" {}
                           }
                       }
                       p class="bookmark-note" {
                           (PreEscaped("You can override preferences with query parameters (e.g. <code>?mp4Playback=on</code>). These overrides aren't saved to cookies."))
                       }
                   }

               }

               // Actions row — reset and save side by side
               div class="preferences-actions" {
                   form method="post" action="/resetprefs" class="pref-reset-form" {
                       input type="hidden" name="referer" value=(referer);
                       button type="submit" class="pref-reset" { "Reset preferences" }
                   }
                   button type="submit" form="saveprefs-form" class="pref-submit" { "Save preferences" }
               }

               h4 class="note preferences-footer-note" { "Preferences are stored client-side using cookies without any personal information." }
           }
       }
   }
}
