#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[component] and view! expansions"
)]

use std::{
   fs,
   ops::Index,
   path::Path,
};

use percent_encoding::{
   NON_ALPHANUMERIC,
   utf8_percent_encode,
};
use topcoat::{
   Result,
   context::{
      Cx,
      app_context,
   },
   runtime::{
      Signal,
      script,
      signal,
   },
   view::{
      Child,
      View,
      component,
      view,
   },
};

use crate::{
   AppState,
   types::prefs::{
      Prefs,
      Toggle,
   },
   views::layout::{
      CLEAN_CSS,
      FONTELLO_CSS,
      STYLE_CSS,
   },
};

const X_LOGO_PATH: &str = "M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-4.714-6.231-5.401 \
                           6.231H2.744l7.737-8.834L1.254 2.25H8.08l4.259 5.622L18.244 \
                           2.25zm-1.161 17.52h1.833L7.084 4.126H5.117L17.083 19.77z";

/// Preferences as browser signals, so a change restyles the page before saving.
pub struct LivePrefs {
   theme:     Signal<String>,
   font_size: Signal<String>,
   toggles:   Vec<Signal<bool>>,
}

impl LivePrefs {
   pub fn new(cx: &Cx, prefs: &Prefs, themes: &[String]) -> Self {
      let theme = themes
         .iter()
         .find(|name| theme_matches(name, &prefs.theme))
         .map_or_else(|| prefs.theme.clone(), Clone::clone);

      Self {
         theme:     signal(cx, || theme),
         font_size: signal(cx, || prefs.font_size.clone()),
         toggles:   Toggle::ALL
            .iter()
            .map(|&toggle| signal(&cx.keyed(toggle.name()), || toggle.field(prefs)))
            .collect(),
      }
   }

   pub const fn theme(&self) -> &Signal<String> {
      &self.theme
   }

   pub const fn font_size(&self) -> &Signal<String> {
      &self.font_size
   }
}

impl Index<Toggle> for LivePrefs {
   type Output = Signal<bool>;

   fn index(&self, index: Toggle) -> &Signal<bool> {
      &self.toggles[index as usize]
   }
}

pub fn theme_matches(name: &str, selected: &str) -> bool {
   name.eq_ignore_ascii_case(selected)
      || name.to_lowercase().replace(' ', "_") == selected.to_lowercase()
}

/// Discover available themes from CSS files in the static directory.
#[expect(
   clippy::absolute_paths,
   reason = "crate's Result type shadows std::result::Result"
)]
pub fn themes(static_dir: &str) -> Vec<String> {
   let themes_dir = format!("{static_dir}/css/themes");

   fs::read_dir(&themes_dir).map_or_else(
      |_| {
         vec![
            "Auto".to_owned(),
            "Black".to_owned(),
            "Dracula".to_owned(),
            "Mastodon".to_owned(),
            "teapot".to_owned(),
            "Pleroma".to_owned(),
            "Twitter".to_owned(),
            "Twitter Dark".to_owned(),
         ]
      },
      |entries| {
         let mut themes = entries
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| {
               Path::new(name)
                  .extension()
                  .is_some_and(|ext| ext.eq_ignore_ascii_case("css"))
            })
            .map(|name| {
               name
                  .trim_end_matches(".css")
                  .replace('_', " ")
                  .split_whitespace()
                  .map(|word| {
                     let mut chars = word.chars();
                     chars.next().map_or_else(String::new, |first| {
                        first.to_uppercase().chain(chars).collect()
                     })
                  })
                  .collect::<Vec<_>>()
                  .join(" ")
            })
            .collect::<Vec<_>>();

         themes.sort();
         themes
      },
   )
}

#[component]
pub async fn document(
   cx: &Cx,
   title: &str,
   description: &str,
   live: &LivePrefs,
   themes: &[String],
   #[default] referer: &str,
   #[default] child: Child<'_>,
) -> Result<impl View> {
   let config = &app_context::<AppState>(cx).config;
   let server_title = config.server.title.as_str();

   let theme = live.theme();
   let font_size = live.font_size();
   let sticky_nav = &live[Toggle::StickyNav];
   let twemoji = &live[Toggle::UseTwemoji];
   let theme_links = themes
      .iter()
      .filter(|name| !name.eq_ignore_ascii_case("teapot"))
      .map(|name| {
         let href = format!("/css/themes/{}.css", name.to_lowercase().replace(' ', "_"));
         (name.clone(), href)
      })
      .collect::<Vec<_>>();
   let settings_href = if referer.is_empty() {
      "/settings".to_owned()
   } else {
      format!(
         "/settings?referer={}",
         utf8_percent_encode(referer, NON_ALPHANUMERIC)
      )
   };
   let opensearch = format!("{}/opensearch", config.url_prefix());

   Ok(view! {
      <!DOCTYPE html>
      <html lang="en">
         <head>
            <meta charset="utf-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <meta name="theme-color" content="#1F1F1F">
            <link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png">
            <link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png">
            <link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png">
            <link rel="manifest" href="/site.webmanifest">
            <link rel="mask-icon" href="/safari-pinned-tab.svg" color="#ff6c60">
            <link
               rel="search"
               type="application/opensearchdescription+xml"
               title=(server_title)
               href=(opensearch)
            >
            <link rel="stylesheet" type="text/css" href=(STYLE_CSS)>
            <link rel="stylesheet" type="text/css" href=(CLEAN_CSS)>
            <link rel="stylesheet" type="text/css" href=(FONTELLO_CSS)>
            for (name, href) in theme_links {
               <link
                  rel="stylesheet"
                  type="text/css"
                  href=(href)
                  :disabled=$(theme.get() != name)
               >
            }
            <link
               rel="stylesheet"
               type="text/css"
               href="/css/twemoji.css"
               :disabled=$(!twemoji.get())
            >
            <title>(title) " | " (server_title)</title>
            <meta property="og:site_name" content=(server_title)>
            <meta property="og:locale" content="en_US">
            <meta property="og:title" content=(title)>
            <meta property="og:description" content=(description)>
            <meta property="og:type" content="article">
            script()
            <link
               rel="preload"
               type="font/woff2"
               as="font"
               href="/fonts/fontello.woff2?61663884"
               crossorigin="anonymous"
            >
         </head>
         <body
            :class=$(if sticky_nav.get() { "fixed-nav" } else { "" })
            :style=$({
               let size = font_size.get();
               if size == "Small" {
                  "font-size: 14px"
               } else if size == "Medium" {
                  "font-size: 16px"
               } else if size == "Large" {
                  "font-size: 18px"
               } else if size == "X-Large" {
                  "font-size: 20px"
               } else {
                  ""
               }
            })
         >
            <nav>
               <div class="inner-nav">
                  <div class="nav-item">
                     <a class="site-brand" href="/">
                        <img class="site-logo" src="/logo.svg" alt="">
                        <span class="site-name">(server_title)</span>
                     </a>
                  </div>
                  <div class="nav-item right">
                     <a class="nav-icon" title="Search" href="/search">
                        <span class="icon-search" aria-hidden="true"></span>
                     </a>
                     <a class="nav-icon" title="Open in X" href="https://x.com">
                        <svg
                           viewBox="0 0 24 24"
                           width="18"
                           height="18"
                           fill="currentColor"
                           aria-hidden="true"
                        >
                           <path d=(X_LOGO_PATH)></path>
                        </svg>
                     </a>
                     <a class="nav-icon" title="About" href="/about">
                        <span class="icon-info" aria-hidden="true"></span>
                     </a>
                     <a class="nav-icon" title="Preferences" href=(&settings_href)>
                        <span class="icon-cog" aria-hidden="true"></span>
                     </a>
                  </div>
                  <details class="mobile-nav">
                     <summary aria-label="Open navigation menu">
                        <svg
                           viewBox="0 0 24 24"
                           width="19"
                           height="19"
                           fill="none"
                           stroke="currentColor"
                           stroke-width="1.8"
                           stroke-linecap="round"
                           aria-hidden="true"
                        >
                           <path d="M4 7h16M4 12h16M4 17h16"></path>
                        </svg>
                     </summary>
                     <div class="mobile-nav-menu">
                        <a href="/search">
                           <span class="icon-search" aria-hidden="true"></span>
                           "Search"
                        </a>
                        <a href="https://x.com">
                           <svg
                              viewBox="0 0 24 24"
                              width="15"
                              height="15"
                              fill="currentColor"
                              aria-hidden="true"
                           >
                              <path d=(X_LOGO_PATH)></path>
                           </svg>
                           "Open in X"
                        </a>
                        <a href="/about">
                           <span class="icon-info" aria-hidden="true"></span>
                           "About"
                        </a>
                        <a href=(&settings_href)>
                           <span class="icon-cog" aria-hidden="true"></span>
                           "Preferences"
                        </a>
                     </div>
                  </details>
               </div>
            </nav>
            <main>
               <div class="container">(child)</div>
            </main>
         </body>
      </html>
   })
}
