use std::str::FromStr;

use axum_extra::extract::cookie::CookieJar;
use flub::Error;
use serde::{
   Deserialize,
   Serialize,
};

use crate::config::Config;

/// User preferences stored in cookies.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[expect(
   clippy::struct_excessive_bools,
   reason = "each bool is an independent user preference"
)]
pub struct Prefs {
   pub theme:                String,
   pub infinite_scroll:      bool,
   pub sticky_profile:       bool,
   pub bidi_support:         bool,
   pub mp4_playback:         bool,
   pub autoplay_gifs:        bool,
   pub mute_videos:          bool,
   pub hide_tweet_stats:     bool,
   pub hide_banner:          bool,
   pub hide_pins:            bool,
   pub hide_replies:         bool,
   pub hide_community_notes: bool,
   pub square_avatars:       bool,
   pub use_twemoji:          bool,
   pub sticky_nav:           bool,
   pub font_size:            String,
   pub replace_twitter:      String,
   pub replace_youtube:      String,
   pub replace_reddit:       String,
   pub kagi_token:           String,
}

impl Prefs {
   /// Canonical defaults -- the ONE place all defaults are defined.
   pub fn with_defaults(config: &Config) -> Self {
      Self {
         theme:                config.preferences.theme.clone(),
         infinite_scroll:      config.preferences.infinite_scroll,
         sticky_profile:       true,
         bidi_support:         false,
         mp4_playback:         true,
         autoplay_gifs:        true,
         mute_videos:          false,
         hide_tweet_stats:     false,
         hide_banner:          false,
         hide_pins:            false,
         hide_replies:         false,
         hide_community_notes: false,
         square_avatars:       true,
         use_twemoji:          true,
         sticky_nav:           true,
         font_size:            String::new(),
         replace_twitter:      config.preferences.replace_twitter.clone(),
         replace_youtube:      config.preferences.replace_youtube.clone(),
         replace_reddit:       config.preferences.replace_reddit.clone(),
         kagi_token:           String::new(),
      }
   }

   /// Extract preferences from cookies, falling back to config defaults.
   pub fn from_cookies(jar: &CookieJar, config: &Config) -> Self {
      let mut prefs = Self::with_defaults(config);
      for toggle in Toggle::ALL {
         if let Some(cookie) = jar.get(toggle.name()) {
            *toggle.field_mut(&mut prefs) = cookie.value() == "on";
         }
      }
      for text in Text::ALL {
         if let Some(cookie) = jar.get(text.name()) {
            cookie.value().clone_into(text.field_mut(&mut prefs));
         }
      }
      prefs
   }

   /// Cookie names for all preference fields.
   pub fn cookie_names() -> impl Iterator<Item = &'static str> {
      Toggle::ALL
         .iter()
         .map(|toggle| toggle.name())
         .chain(Text::ALL.iter().map(|text| text.name()))
   }

   /// Preference names accepted from shareable URL preference imports.
   pub fn is_url_pref(name: &str) -> bool {
      Toggle::ALL.iter().any(|toggle| toggle.name() == name)
         || Text::ALL
            .iter()
            .any(|text| text.shareable() && text.name() == name)
   }
}

#[derive(Debug, Error)]
#[error("unknown preference {0:?}")]
pub struct UnknownPref(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
   Display,
   Media,
}

macro_rules! toggles {
   ($($variant:ident => $field:ident, $name:literal, $section:ident, $label:literal;)+) => {
      #[derive(Debug, Clone, Copy, PartialEq, Eq)]
      pub enum Toggle {
         $($variant,)+
      }

      impl Toggle {
         pub const ALL: &[Self] = &[$(Self::$variant,)+];

         pub fn in_section(section: Section) -> impl Iterator<Item = Self> {
            Self::ALL
               .iter()
               .copied()
               .filter(move |toggle| toggle.section() == section)
         }

         pub const fn name(self) -> &'static str {
            match self {
               $(Self::$variant => $name,)+
            }
         }

         pub const fn label(self) -> &'static str {
            match self {
               $(Self::$variant => $label,)+
            }
         }

         pub const fn section(self) -> Section {
            match self {
               $(Self::$variant => Section::$section,)+
            }
         }

         pub const fn field(self, prefs: &Prefs) -> bool {
            match self {
               $(Self::$variant => prefs.$field,)+
            }
         }

         pub const fn field_mut(self, prefs: &mut Prefs) -> &mut bool {
            match self {
               $(Self::$variant => &mut prefs.$field,)+
            }
         }
      }

      impl FromStr for Toggle {
         type Err = UnknownPref;

         fn from_str(name: &str) -> Result<Self, Self::Err> {
            match name {
               $($name => Ok(Self::$variant),)+
               _ => Err(UnknownPref(name.to_owned())),
            }
         }
      }
   };
}

macro_rules! texts {
   ($($variant:ident => $field:ident, $name:literal, $shareable:literal;)+) => {
      #[derive(Debug, Clone, Copy, PartialEq, Eq)]
      pub enum Text {
         $($variant,)+
      }

      impl Text {
         pub const ALL: &[Self] = &[$(Self::$variant,)+];

         pub const fn name(self) -> &'static str {
            match self {
               $(Self::$variant => $name,)+
            }
         }

         pub fn accepts(value: &str) -> bool {
            value.len() <= 512
               && value
                  .chars()
                  .all(|ch| (' '..='~').contains(&ch) && !matches!(ch, '"' | ',' | ';' | '\\'))
         }

         pub const fn shareable(self) -> bool {
            match self {
               $(Self::$variant => $shareable,)+
            }
         }

         pub fn field(self, prefs: &Prefs) -> &str {
            match self {
               $(Self::$variant => &prefs.$field,)+
            }
         }

         pub const fn field_mut(self, prefs: &mut Prefs) -> &mut String {
            match self {
               $(Self::$variant => &mut prefs.$field,)+
            }
         }
      }

      impl FromStr for Text {
         type Err = UnknownPref;

         fn from_str(name: &str) -> Result<Self, Self::Err> {
            match name {
               $($name => Ok(Self::$variant),)+
               _ => Err(UnknownPref(name.to_owned())),
            }
         }
      }
   };
}

toggles! {
   InfiniteScroll => infinite_scroll, "infiniteScroll", Display,
      "Infinite scrolling (experimental, requires JavaScript)";
   StickyProfile => sticky_profile, "stickyProfile", Display, "Make profile sidebar stick to top";
   StickyNav => sticky_nav, "stickyNav", Display, "Keep navbar fixed to top";
   BidiSupport => bidi_support, "bidiSupport", Display, "Support bidirectional text";
   HideTweetStats => hide_tweet_stats, "hideTweetStats", Display, "Hide tweet stats";
   HideBanner => hide_banner, "hideBanner", Display, "Hide profile banner";
   HidePins => hide_pins, "hidePins", Display, "Hide pinned tweets";
   HideReplies => hide_replies, "hideReplies", Display, "Hide tweet replies";
   HideCommunityNotes => hide_community_notes, "hideCommunityNotes", Display,
      "Hide community notes";
   SquareAvatars => square_avatars, "squareAvatars", Display, "Square profile pictures";
   UseTwemoji => use_twemoji, "useTwemoji", Display, "Use Twitter emoji (Twemoji)";
   Mp4Playback => mp4_playback, "mp4Playback", Media, "Enable mp4 video playback";
   MuteVideos => mute_videos, "muteVideos", Media, "Mute videos by default";
   AutoplayGifs => autoplay_gifs, "autoplayGifs", Media, "Autoplay gifs";
}

texts! {
   Theme => theme, "theme", true;
   FontSize => font_size, "fontSize", true;
   ReplaceTwitter => replace_twitter, "replaceTwitter", true;
   ReplaceYouTube => replace_youtube, "replaceYouTube", true;
   ReplaceReddit => replace_reddit, "replaceReddit", true;
   KagiToken => kagi_token, "kagiToken", false;
}
