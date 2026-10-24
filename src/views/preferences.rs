#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[component] and view! expansions"
)]

use topcoat::{
   Result,
   runtime::{
      Event,
      Signal,
   },
   view::{
      View,
      component,
      view,
   },
};

use crate::{
   routes::preferences::{
      save_text,
      save_toggle,
   },
   types::prefs::{
      Prefs,
      Section,
      Text,
      Toggle,
   },
   views::document::LivePrefs,
};

const LINK_REPLACEMENTS: [(Text, &str, &str); 3] = [
   (Text::ReplaceTwitter, "Twitter -> teapot", "teapot hostname"),
   (
      Text::ReplaceYouTube,
      "YouTube -> Piped/Invidious",
      "Piped hostname",
   ),
   (
      Text::ReplaceReddit,
      "Reddit -> Teddit/Libreddit",
      "Teddit hostname",
   ),
];

const FONT_SIZES: [(&str, &str); 5] = [
   ("", "Default"),
   ("Small", "Small (14px)"),
   ("Medium", "Medium (16px)"),
   ("Large", "Large (18px)"),
   ("X-Large", "X-Large (20px)"),
];

/// Render a checkbox preference row with title tooltip.
#[component]
async fn checkbox(toggle: Toggle, state: &Signal<bool>) -> Result<impl View> {
   let name = toggle.name().to_owned();
   let id = format!("pref-{name}");
   Ok(view! {
      <label class="pref-group checkbox-container" title=(&name) for=(&id)>
         (toggle.label())
         <input
            id=(&id)
            type="checkbox"
            name=(&name)
            checked=(state.get_untracked())
            @change=$(async |event: Event| {
               state.set(event.target.checked);
               save_toggle(name, event.target.checked).await;
            })
         >
         <span class="checkbox"></span>
      </label>
   })
}

/// Render a text input preference row with title tooltip.
#[component]
async fn text_input(text: Text, label: &str, value: &str, placeholder: &str) -> Result<impl View> {
   let name = text.name().to_owned();
   let id = format!("pref-{name}");
   Ok(view! {
      <div class="pref-group pref-input" title=(&name)>
         <label for=(&id)>(label)</label>
         <input
            id=(&id)
            type="text"
            name=(&name)
            placeholder=(placeholder)
            value=(value)
            @change=$(async |event: Event| save_text(name, event.target.value).await)
         >
      </div>
   })
}

/// Render a select preference row with title tooltip.
#[component]
async fn theme_select(theme: &Signal<String>, themes: &[String]) -> Result<impl View> {
   let selected = theme.get_untracked();
   let name = Text::Theme.name().to_owned();
   Ok(view! {
      <div class="pref-group pref-input" title=(&name)>
         <label for="pref-theme">"Theme"</label>
         <select
            id="pref-theme"
            name=(&name)
            @change=$(async |event: Event| {
               theme.set(event.target.value);
               save_text(name, theme.get()).await;
            })
         >
            for option in themes {
               <option value=(option) selected=(*option == selected)>(option)</option>
            }
         </select>
      </div>
   })
}

/// Render the font size select with live preview on change.
#[component]
async fn font_size_select(font_size: &Signal<String>) -> Result<impl View> {
   let selected = font_size.get_untracked();
   let name = Text::FontSize.name().to_owned();
   Ok(view! {
      <div class="pref-group pref-input" title=(&name)>
         <label for="pref-fontSize">"Font size"</label>
         <select
            id="pref-fontSize"
            name=(&name)
            @change=$(async |event: Event| {
               font_size.set(event.target.value);
               save_text(name, font_size.get()).await;
            })
         >
            for (value, display) in FONT_SIZES {
               <option value=(value) selected=(value.eq_ignore_ascii_case(&selected))>
                  (display)
               </option>
            }
         </select>
      </div>
   })
}

/// Render the preferences/settings form.
#[component]
pub async fn preferences_form(
   prefs: &Prefs,
   live: &LivePrefs,
   referer: &str,
   prefs_url: &str,
) -> Result<impl View> {
   Ok(view! {
      <div class="overlay-panel preferences-panel">
         <fieldset class="preferences">
            <a href=(referer) class="preferences-close">
               <span class="icon-close"></span>
            </a>
            <div class="preferences-header">
               <h2>"Preferences"</h2>
               <p>"Customize your teapot experience"</p>
            </div>
            <form
               method="post"
               action="/saveprefs"
               autocomplete="off"
               class="preferences-form"
               id="saveprefs-form"
            >
               <input type="hidden" name="referer" value=(referer)>
               <div class="pref-section" data-section="display">
                  <legend>"Display"</legend>
                  <div class="pref-grid">
                     theme_select(theme: live.theme(), themes: live.themes())
                     font_size_select(font_size: live.font_size())
                     for toggle in Toggle::in_section(Section::Display) {
                        checkbox(toggle: toggle, state: &live[toggle])
                     }
                  </div>
               </div>
               <div class="pref-section" data-section="media">
                  <legend>"Media"</legend>
                  <div class="pref-grid">
                     for toggle in Toggle::in_section(Section::Media) {
                        checkbox(toggle: toggle, state: &live[toggle])
                     }
                  </div>
               </div>
               <div class="pref-section" data-section="links">
                  <legend>"Link replacements (blank to disable)"</legend>
                  for (text, label, placeholder) in LINK_REPLACEMENTS {
                     text_input(
                        text: text,
                        label: label,
                        value: text.field(prefs),
                        placeholder: placeholder,
                     )
                  }
               </div>
               <div class="pref-section" data-section="translation">
                  <legend>"Translation"</legend>
                  text_input(
                     text: Text::KagiToken,
                     label: "Kagi session token",
                     value: Text::KagiToken.field(prefs),
                     placeholder: "Paste session token here",
                  )
                  <p class="bookmark-note">
                     "Uses Kagi Translate instead of Twitter. "
                     <a href="https://kagi.com/settings?p=user_details" target="_blank">
                        "Get your token here"
                     </a>
                     " (Session Link → copy token)"
                  </p>
               </div>
               <div class="pref-section pref-section-bookmark" data-section="bookmark">
                  <legend>"Bookmark"</legend>
                  <p class="bookmark-note">
                     "Save this URL to restore your preferences (?prefs works on all pages)"
                  </p>
                  <div class="prefs-code-wrapper">
                     <pre class="prefs-code"><code>(prefs_url)</code></pre>
                     <button type="button" class="copy-prefs-btn" data-clipboard=(prefs_url)>
                        <span class="icon-copy"></span>
                     </button>
                  </div>
                  <p class="bookmark-note">
                     "You can override preferences with query parameters (e.g. "
                     <code>"?mp4Playback=on"</code>
                     "). These overrides aren't saved to cookies."
                  </p>
               </div>
            </form>
            // Actions row — reset and save side by side
            <div class="preferences-actions">
               <form method="post" action="/resetprefs" class="pref-reset-form">
                  <input type="hidden" name="referer" value=(referer)>
                  <button type="submit" class="pref-reset">"Reset preferences"</button>
               </form>
               <button type="submit" form="saveprefs-form" class="pref-submit">
                  "Save preferences"
               </button>
            </div>
            <h4 class="note preferences-footer-note">
               "Preferences are stored client-side using cookies without any personal information."
            </h4>
         </fieldset>
      </div>
   })
}
