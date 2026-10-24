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
   context::Cx,
   runtime::{
      Signal,
      script,
      signal,
   },
   view::{
      Child,
      View,
      class,
      component,
      view,
   },
};

use crate::{
   types::prefs::{
      Prefs,
      Toggle,
   },
   utils::formatters::replace_matches,
   views::renderutils::config,
};

pub const STYLE_CSS: &str = "/css/style.css";
pub const CLEAN_CSS: &str = "/css/clean.css";
pub const FONTELLO_CSS: &str = "/css/fontello.css";

const X_LOGO_PATH: &str = "M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-4.714-6.231-5.401 \
                           6.231H2.744l7.737-8.834L1.254 2.25H8.08l4.259 5.622L18.244 \
                           2.25zm-1.161 17.52h1.833L7.084 4.126H5.117L17.083 19.77z";

const SCRIPTS: [&str; 2] = ["/js/site.js", "/js/threadContext.js"];

/// Preferences as browser signals, so a change restyles the page before saving.
pub struct LivePrefs {
   theme:     Signal<String>,
   font_size: Signal<String>,
   toggles:   Vec<Signal<bool>>,
   themes:    Vec<String>,
}

impl LivePrefs {
   pub fn new(cx: &Cx, prefs: &Prefs, themes: Vec<String>) -> Self {
      let theme = themes
         .iter()
         .find(|name| theme_matches(name, &prefs.theme))
         .map_or_else(|| prefs.theme.clone(), Clone::clone);

      Self {
         theme: signal(cx, || theme),
         font_size: signal(cx, || prefs.font_size.clone()),
         toggles: Toggle::ALL
            .iter()
            .map(|&toggle| signal(&cx.keyed(toggle.name()), || toggle.field(prefs)))
            .collect(),
         themes,
      }
   }

   pub const fn theme(&self) -> &Signal<String> {
      &self.theme
   }

   pub const fn font_size(&self) -> &Signal<String> {
      &self.font_size
   }

   pub fn themes(&self) -> &[String] {
      &self.themes
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

fn theme_href(name: &str) -> Option<String> {
   (!name.is_empty() && !name.eq_ignore_ascii_case("teapot"))
      .then(|| format!("/css/themes/{}.css", name.to_lowercase().replace(' ', "_")))
}

const fn font_size_style(size: &str) -> &'static str {
   match size.as_bytes() {
      b"Small" => "font-size: 14px",
      b"Medium" => "font-size: 16px",
      b"Large" => "font-size: 18px",
      b"X-Large" => "font-size: 20px",
      _ => "",
   }
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

/// Strip HTML tags from text, replacing `<a href="...">display</a>` with the
/// actual URL. Used for `og:description` where we want link targets, not
/// display text.
pub fn strip_html(text: &str) -> String {
   // Show each <a> tag's href URL instead of its display text
   let linked = replace_matches(
      text,
      |pos| anchor_at(text, pos),
      |out, (url, anchor)| out.push_str(if url.contains("http") { url } else { anchor }),
   );

   let mut result = String::new();
   let mut in_tag = false;
   for ch in linked.chars() {
      match ch {
         '<' => in_tag = true,
         '>' => in_tag = false,
         _ if !in_tag => result.push(ch),
         _ => {},
      }
   }
   result
}

fn anchor_at(text: &str, pos: usize) -> Option<(usize, (&str, &str))> {
   let after_tag = text[pos..].strip_prefix("<a")?;
   let space = after_tag
      .find(|ch: char| !ch.is_whitespace())
      .unwrap_or(after_tag.len());
   if space == 0 {
      return None;
   }
   let attrs = &after_tag[space..];
   let attrs_start = text.len() - attrs.len();
   let tag_end = attrs.find('>').unwrap_or(attrs.len());

   attrs[..tag_end]
      .rmatch_indices(r#"href=""#)
      .find_map(|(offset, _)| {
         let url_start = attrs_start + offset + r#"href=""#.len();
         let url_len = text[url_start..].find('"')?;
         let after_url = url_start + url_len + 1;
         let content = after_url + text[after_url..].find('>')? + 1;
         let close = content + text[content..].find('<')?;
         let end = text[close..]
            .starts_with("</a>")
            .then_some(close + "</a>".len())?;
         Some((
            end,
            (&text[url_start..url_start + url_len], &text[pos..end]),
         ))
      })
}

/// The page shell. `head` adds tags after the `OpenGraph` block, and
/// `custom_og` lets `head` supply that block itself. With `live`, the theme,
/// font size and toggles follow their signals.
#[component]
pub async fn document<'a>(
   cx: &Cx,
   title: &'a str,
   prefs: &'a Prefs,
   #[default] live: Option<&'a LivePrefs>,
   #[default] description: &'a str,
   #[default] rss: &'a str,
   #[default] canonical: &'a str,
   #[default] referer: &'a str,
   #[default] og_image: &'a str,
   #[default("article")] og_type: &'a str,
   #[default("#1F1F1F")] theme_color: &'a str,
   #[default] custom_og: bool,
   #[default] head: Option<Child<'a>>,
   #[default] child: Child<'a>,
) -> Result<impl View> {
   let config = config(cx);
   let server_title = config.server.title.as_str();
   let has_head = head.is_some();
   let rss = (config.config.enable_rss && !rss.is_empty()).then_some(rss);
   let canonical_or_x = if canonical.is_empty() {
      "https://x.com"
   } else {
      canonical
   };
   let settings_href = if referer.is_empty() {
      "/settings".to_owned()
   } else {
      format!(
         "/settings?referer={}",
         utf8_percent_encode(referer, NON_ALPHANUMERIC)
      )
   };
   let page_title = if title.is_empty() {
      server_title.to_owned()
   } else {
      format!("{title} | {server_title}")
   };
   let live_themes = live.map_or_default(|live| {
      live
         .themes()
         .iter()
         .filter_map(|name| theme_href(name).map(|href| (name.clone(), href)))
         .collect::<Vec<_>>()
   });
   let static_theme = theme_href(&prefs.theme).filter(|_| live.is_none());
   let signals = live.map(|live| {
      (
         live.theme(),
         live.font_size(),
         &live[Toggle::StickyNav],
         &live[Toggle::UseTwemoji],
      )
   });

   Ok(view! {
      <!DOCTYPE html>
      <html lang="en">
         <head>
            <meta charset="utf-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            if custom_og {
               <meta property="theme-color" content=(theme_color)>
            } else {
               <meta name="theme-color" content=(theme_color)>
               <link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png">
            }
            <link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png">
            <link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png">
            <link rel="manifest" href="/site.webmanifest">
            <link
               rel="mask-icon"
               href="/safari-pinned-tab.svg"
               color=(if custom_og { theme_color } else { "#ff6c60" })
            >
            // OpenSearch description (full absolute URL to /opensearch)
            <link
               rel="search"
               type="application/opensearchdescription+xml"
               title=(server_title)
               href=(format!("{}/opensearch", config.url_prefix()))
            >
            if let Some(rss) = rss {
               <link rel="alternate" type="application/rss+xml" title="RSS feed" href=(rss)>
            }
            // Main CSS + icon fonts (with cache-busting)
            <link rel="stylesheet" type="text/css" href=(STYLE_CSS)>
            // After the component styles, whose layout decisions it replaces.
            <link rel="stylesheet" type="text/css" href=(CLEAN_CSS)>
            <link rel="stylesheet" type="text/css" href=(FONTELLO_CSS)>
            if let Some(href) = static_theme {
               <link rel="stylesheet" type="text/css" href=(href)>
            }
            if let Some((theme, _, _, twemoji)) = signals {
               for (name, href) in live_themes {
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
            } else if prefs.use_twemoji {
               // Twemoji font (opt-in via preferences, on by default)
               <link rel="stylesheet" type="text/css" href="/css/twemoji.css">
            }
            <title>(page_title)</title>
            if !custom_og {
               <meta property="og:site_name" content=(server_title)>
               <meta property="og:locale" content="en_US">
               <meta property="og:title" content=(title)>
               <meta property="og:description" content=(strip_html(description))>
               // head provides its own og:type and twitter:card when present
               // (e.g. video/player for GIF embeds). Discord takes the first
               // occurrence, so suppress defaults.
               if !has_head {
                  <meta property="og:type" content=(og_type)>
               }
               if !og_image.is_empty() {
                  <meta property="og:image" content=(og_image)>
                  <meta property="twitter:image:src" content=(og_image)>
               }
               if !has_head && rss.is_some() {
                  <meta name="twitter:card" content="summary">
               }
            }
            // Custom head content (OG overrides, oEmbed, ActivityPub)
            if let Some(head) = head {
               (head)
            }
            // Link to original x.com URL
            if !canonical.is_empty() {
               <link rel="alternate" href=(canonical) title="View on X">
            }
            // Scripts in <head> with defer
            for src in SCRIPTS {
               <script src=(src) defer=""></script>
            }
            script()
            <link
               rel="preload"
               type="font/woff2"
               as="font"
               href="/fonts/fontello.woff2?61663884"
               crossorigin="anonymous"
            >
         </head>
         if let Some((_, font_size, sticky_nav, _)) = signals {
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
               navbar(rss: rss, canonical: canonical_or_x, settings_href: &settings_href)
               <main><div class="container">(child)</div></main>
            </body>
         } else {
            <body
               class=(class!("fixed-nav" if prefs.sticky_nav))
               style=(Some(font_size_style(&prefs.font_size)).filter(|style| !style.is_empty()))
            >
               navbar(rss: rss, canonical: canonical_or_x, settings_href: &settings_href)
               <main><div class="container">(child)</div></main>
            </body>
         }
      </html>
   })
}

#[component]
async fn navbar<'a>(
   cx: &Cx,
   rss: Option<&'a str>,
   canonical: &'a str,
   settings_href: &'a str,
) -> Result<impl View> {
   let server_title = config(cx).server.title.as_str();
   Ok(view! {
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
               if let Some(rss) = rss {
                  <a class="nav-icon" title="RSS Feed" href=(rss)>
                     <span class="icon-rss" aria-hidden="true"></span>
                  </a>
               }
               <a class="nav-icon" title="Open in X" href=(canonical)>
                  <svg
                     viewBox="0 0 24 24"
                     xmlns="http://www.w3.org/2000/svg"
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
               <a class="nav-icon" title="Preferences" href=(settings_href)>
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
                  <a href="/search"><span class="icon-search" aria-hidden="true"></span>"Search"</a>
                  if let Some(rss) = rss {
                     <a href=(rss)><span class="icon-rss" aria-hidden="true"></span>"RSS feed"</a>
                  }
                  <a href=(canonical)>
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
                  <a href="/about"><span class="icon-info" aria-hidden="true"></span>"About"</a>
                  <a href=(settings_href)><span class="icon-cog" aria-hidden="true"></span>"Preferences"</a>
               </div>
            </details>
         </div>
      </nav>
   })
}

/// Render an error page body.
#[component]
pub async fn error_panel(message: &str) -> Result<impl View> {
   Ok(view! {
      <div class="panel-container">
         <div class="error-panel"><span>(message)</span></div>
      </div>
   })
}
