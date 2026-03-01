use std::sync::LazyLock;

use maud::{
   DOCTYPE,
   Markup,
   html,
};
use regex::Regex;

use crate::{
   config::Config,
   types::Prefs,
   views::renderutils::icon,
};

/// CSS cache-busting paths (update the version when static assets change).
pub const STYLE_CSS: &str = "/css/style.css?v=28";
pub const FONTELLO_CSS: &str = "/css/fontello.css?v=4";

/// Builder for rendering a full page layout.
#[expect(
   clippy::module_name_repetitions,
   reason = "PageLayout is the canonical name"
)]
pub struct PageLayout<'a> {
   config:      &'a Config,
   title:       &'a str,
   body:        Markup,
   description: &'a str,
   prefs:       Option<&'a Prefs>,
   rss:         &'a str,
   canonical:   &'a str,
   referer:     &'a str,
   og_image:    &'a str,
   og_type:     &'a str,
   head_extra:  Option<&'a Markup>,
}

impl<'a> PageLayout<'a> {
   pub const fn new(config: &'a Config, title: &'a str, body: Markup) -> Self {
      Self {
         config,
         title,
         body,
         description: "",
         prefs: None,
         rss: "",
         canonical: "",
         referer: "",
         og_image: "",
         og_type: "article",
         head_extra: None,
      }
   }

   pub const fn description(mut self, description: &'a str) -> Self {
      self.description = description;
      self
   }

   pub const fn prefs(mut self, prefs: &'a Prefs) -> Self {
      self.prefs = Some(prefs);
      self
   }

   pub const fn rss(mut self, rss: &'a str) -> Self {
      self.rss = rss;
      self
   }

   pub const fn canonical(mut self, canonical: &'a str) -> Self {
      self.canonical = canonical;
      self
   }

   pub const fn referer(mut self, referer: &'a str) -> Self {
      self.referer = referer;
      self
   }

   pub const fn og_image(mut self, og_image: &'a str) -> Self {
      self.og_image = og_image;
      self
   }

   pub const fn og_type(mut self, og_type: &'a str) -> Self {
      self.og_type = og_type;
      self
   }

   /// Extra markup injected into `<head>` after the standard OG tags.
   /// Use for custom meta tags (media OG, oEmbed, `ActivityPub` links).
   pub const fn head_extra(mut self, head_extra: &'a Markup) -> Self {
      self.head_extra = Some(head_extra);
      self
   }

   pub fn render(self) -> Markup {
      let (hls_playback, infinite_scroll, theme, sticky_nav, use_twemoji) =
         self.prefs.map_or((false, false, None, true, true), |prefs| {
            (
               prefs.hls_playback,
               prefs.infinite_scroll,
               Some(prefs.theme.as_str()),
               prefs.sticky_nav,
               prefs.use_twemoji,
            )
         });

      let body_class = if sticky_nav { "fixed-nav" } else { "" };

      let theme_href = match theme {
         Some(theme_name) if !theme_name.is_empty() && theme_name.to_lowercase() != "teapot" => {
            Some(format!(
               "/css/themes/{}.css",
               theme_name.to_lowercase().replace(' ', "_")
            ))
         },
         _ => None,
      };

      html! {
          (DOCTYPE)
          html lang="en" {
              head {
                  meta charset="utf-8";
                  meta name="viewport" content="width=device-width, initial-scale=1.0";
                  meta name="theme-color" content="#1F1F1F";

                  // Favicon and manifest links
                  link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png";
                  link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png";
                  link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png";
                  link rel="manifest" href="/site.webmanifest";
                  link rel="mask-icon" href="/safari-pinned-tab.svg" color="#ff6c60";

                  // OpenSearch description (full absolute URL to /opensearch)
                  link rel="search" type="application/opensearchdescription+xml"
                      title=(self.config.server.title)
                      href=(format!("{}/opensearch", self.config.url_prefix()));

                  // RSS alternate link
                  @if self.config.config.enable_rss && !self.rss.is_empty() {
                      link rel="alternate" type="application/rss+xml"
                          title="RSS feed"
                          href=(self.rss);
                  }

                  // Main CSS + icon fonts (with cache-busting)
                  link rel="stylesheet" type="text/css" href=(STYLE_CSS);
                  link rel="stylesheet" type="text/css" href=(FONTELLO_CSS);
                  // Theme CSS if different from default
                  @if let Some(ref href) = theme_href {
                      link rel="stylesheet" type="text/css" href=(href);
                  }

                  // Twemoji font (opt-in via preferences, on by default)
                  @if use_twemoji {
                      link rel="stylesheet" type="text/css" href="/css/twemoji.css";
                  }

                  @if self.title.is_empty() {
                      title { (self.config.server.title) }
                  } @else {
                      title { (self.title) " | " (self.config.server.title) }
                  }

                  // OpenGraph meta tags
                  meta property="og:site_name" content=(self.config.server.title);
                  meta property="og:locale" content="en_US";
                  meta property="og:title" content=(self.title);
                  meta property="og:description" content=(strip_html(self.description));
                  meta property="og:type" content=(self.og_type);
                  @if !self.og_image.is_empty() {
                      meta property="og:image" content=(self.og_image);
                      meta property="twitter:image:src" content=(self.og_image);
                  }
                  @if !self.rss.is_empty() {
                      meta name="twitter:card" content="summary";
                  }

                  // Custom head content (OG overrides, oEmbed, ActivityPub)
                  @if let Some(extra) = self.head_extra {
                      (extra)
                  }

                  // Link to original x.com URL
                  @if !self.canonical.is_empty() {
                      link rel="alternate" href=(self.canonical) title="View on X";
                  }

                  // Scripts in <head> with defer
                  @if hls_playback {
                      script src="/js/hls.min.js" defer="" {}
                      script src="/js/hlsPlayback.js" defer="" {}
                  }
                  @if infinite_scroll {
                      script src="/js/infiniteScroll.js" defer="" {}
                  }

                  // Font preload
                  link rel="preload" type="font/woff2" as="font"
                      href="/fonts/fontello.woff2?61663884" crossorigin="anonymous";
              }
              body class=(body_class) {
                  (render_navbar_full(self.config, self.rss, self.canonical, self.referer))
                  div class="container" {
                      (self.body)
                  }
              }
          }
      }
   }
}

/// Render navbar with all context.
pub fn render_navbar_full(config: &Config, rss: &str, canonical: &str, referer: &str) -> Markup {
   let canonical = if canonical.is_empty() {
      "https://x.com".to_owned()
   } else {
      canonical.to_owned()
   };

   let settings_href = if referer.is_empty() {
      "/settings".to_owned()
   } else {
      format!(
         "/settings?referer={}",
         percent_encoding::utf8_percent_encode(referer, percent_encoding::NON_ALPHANUMERIC)
      )
   };

   html! {
       nav {
           div class="inner-nav" {
               div class="nav-item" {
                   a class="site-name" href="/" { (config.server.title) }
               }
               a href="/" {
                   img class="site-logo" src="/logo.svg" alt="Logo";
               }
               div class="nav-item right" {
                   (icon("search", "", "Search", "", "/search"))
                   @if config.config.enable_rss && !rss.is_empty() {
                       (icon("rss", "", "RSS Feed", "", rss))
                   }
                   (icon("bird", "", "Open in X", "", &canonical))
                   (icon("info", "", "About", "", "/about"))
                   (icon("cog", "", "Preferences", "", &settings_href))
               }
           }
       }
   }
}

/// Render error page.
pub fn render_error(config: &Config, title: &str, message: &str) -> Markup {
   PageLayout::new(config, title, html! {
       div class="panel-container" {
           div class="error-panel" {
               span { (message) }
           }
       }
   })
   .description(message)
   .render()
}

static A_RE: LazyLock<Regex> =
   LazyLock::new(|| Regex::new(r#"<a\s+[^>]*href="([^"]*)"[^>]*>[^<]*</a>"#).unwrap());

/// Strip HTML tags from text, replacing `<a href="...">display</a>` with the
/// actual URL. Used for `og:description` where we want link targets, not
/// display text.
pub fn strip_html(text: &str) -> String {
   // Replace <a> tags: show the href URL instead of display text
   let text = A_RE.replace_all(text, |caps: &regex::Captures| {
      let url = &caps[1];
      if url.contains("http") {
         url.to_owned()
      } else {
         caps[0].to_owned()
      }
   });

   // Strip remaining HTML tags
   let mut result = String::new();
   let mut in_tag = false;
   for ch in text.chars() {
      match ch {
         '<' => in_tag = true,
         '>' => in_tag = false,
         _ if !in_tag => result.push(ch),
         _ => {},
      }
   }
   result
}
