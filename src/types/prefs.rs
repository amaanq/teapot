use axum_extra::extract::cookie::CookieJar;
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
   pub proxy_media:          bool,
   pub font_size:            String,
   pub replace_twitter:      String,
   pub replace_youtube:      String,
   pub replace_reddit:       String,
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
         square_avatars:       false,
         use_twemoji:          true,
         sticky_nav:           true,
         proxy_media:          true,
         font_size:            String::new(),
         replace_twitter:      config.preferences.replace_twitter.clone(),
         replace_youtube:      config.preferences.replace_youtube.clone(),
         replace_reddit:       config.preferences.replace_reddit.clone(),
      }
   }

   /// Extract preferences from cookies, falling back to config defaults.
   pub fn from_cookies(jar: &CookieJar, config: &Config) -> Self {
      let defaults = Self::with_defaults(config);
      let bool_pref = |name: &str, default: bool| {
         jar.get(name)
            .map_or(default, |cookie| cookie.value() == "on")
      };
      let str_pref = |name: &str, default: String| {
         jar.get(name)
            .map_or(default, |cookie| cookie.value().to_owned())
      };

      Self {
         theme:                str_pref("theme", defaults.theme),
         infinite_scroll:      bool_pref("infiniteScroll", defaults.infinite_scroll),
         sticky_profile:       bool_pref("stickyProfile", defaults.sticky_profile),
         bidi_support:         bool_pref("bidiSupport", defaults.bidi_support),
         mp4_playback:         bool_pref("mp4Playback", defaults.mp4_playback),
         autoplay_gifs:        bool_pref("autoplayGifs", defaults.autoplay_gifs),
         mute_videos:          bool_pref("muteVideos", defaults.mute_videos),
         hide_tweet_stats:     bool_pref("hideTweetStats", defaults.hide_tweet_stats),
         hide_banner:          bool_pref("hideBanner", defaults.hide_banner),
         hide_pins:            bool_pref("hidePins", defaults.hide_pins),
         hide_replies:         bool_pref("hideReplies", defaults.hide_replies),
         hide_community_notes: bool_pref("hideCommunityNotes", defaults.hide_community_notes),
         square_avatars:       bool_pref("squareAvatars", defaults.square_avatars),
         use_twemoji:          bool_pref("useTwemoji", defaults.use_twemoji),
         sticky_nav:           bool_pref("stickyNav", defaults.sticky_nav),
         proxy_media:          bool_pref("proxyMedia", defaults.proxy_media),
         font_size:            str_pref("fontSize", defaults.font_size),
         replace_twitter:      str_pref("replaceTwitter", defaults.replace_twitter),
         replace_youtube:      str_pref("replaceYouTube", defaults.replace_youtube),
         replace_reddit:       str_pref("replaceReddit", defaults.replace_reddit),
      }
   }

   /// Cookie names for all preference fields.
   pub const COOKIE_NAMES: &[&str] = &[
      "theme",
      "infiniteScroll",
      "stickyProfile",
      "bidiSupport",
      "mp4Playback",
      "autoplayGifs",
      "muteVideos",
      "hideTweetStats",
      "hideBanner",
      "hidePins",
      "hideReplies",
      "hideCommunityNotes",
      "squareAvatars",
      "useTwemoji",
      "stickyNav",
      "proxyMedia",
      "fontSize",
      "replaceTwitter",
      "replaceYouTube",
      "replaceReddit",
   ];
}
