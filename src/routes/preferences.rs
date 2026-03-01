use std::{
   fs,
   path::Path,
};

use axum::{
   Form,
   Router,
   extract::{
      Query,
      State,
   },
   http::{
      HeaderMap,
      header::REFERER,
   },
   response::{
      Html,
      IntoResponse as _,
      Redirect,
      Response,
   },
   routing::{
      get,
      post,
   },
};
use axum_extra::extract::cookie::{
   Cookie,
   CookieJar,
};
use serde::Deserialize;
use time::Duration;

use crate::{
   AppState,
   config::Config,
   error::Result,
   types::Prefs,
   views::{
      layout,
      preferences as pref_view,
   },
};

/// Convert preferences to HTTP cookies for storage.
fn prefs_to_cookies(prefs: &Prefs) -> Vec<Cookie<'static>> {
   let max_age = Duration::days(365);
   let bool_cookie =
      |name: &str, val: bool| make_cookie(name, if val { "on" } else { "" }, max_age);
   let str_cookie = |name: &str, val: &str| make_cookie(name, val, max_age);

   vec![
      str_cookie("theme", &prefs.theme),
      bool_cookie("infiniteScroll", prefs.infinite_scroll),
      bool_cookie("stickyProfile", prefs.sticky_profile),
      bool_cookie("bidiSupport", prefs.bidi_support),
      bool_cookie("hlsPlayback", prefs.hls_playback),
      bool_cookie("mp4Playback", prefs.mp4_playback),
      bool_cookie("proxyVideos", prefs.proxy_videos),
      bool_cookie("autoplayGifs", prefs.autoplay_gifs),
      bool_cookie("muteVideos", prefs.mute_videos),
      bool_cookie("hideTweetStats", prefs.hide_tweet_stats),
      bool_cookie("hideBanner", prefs.hide_banner),
      bool_cookie("hidePins", prefs.hide_pins),
      bool_cookie("hideReplies", prefs.hide_replies),
      bool_cookie("hideCommunityNotes", prefs.hide_community_notes),
      bool_cookie("squareAvatars", prefs.square_avatars),
      bool_cookie("useTwemoji", prefs.use_twemoji),
      bool_cookie("stickyNav", prefs.sticky_nav),
      str_cookie("replaceTwitter", &prefs.replace_twitter),
      str_cookie("replaceYouTube", &prefs.replace_youtube),
      str_cookie("replaceReddit", &prefs.replace_reddit),
   ]
}

/// Encode non-default prefs as a comma-separated string for URL sharing.
#[expect(
   clippy::cognitive_complexity,
   reason = "macro-driven repetition, not actual complexity"
)]
fn encode_prefs(prefs: &Prefs, config: &Config) -> String {
   let defaults = Prefs::with_defaults(config);
   let mut pairs = Vec::new();

   macro_rules! enc_checkbox {
      ($name:expr, $field:ident) => {
         if prefs.$field != defaults.$field {
            if prefs.$field {
               pairs.push(format!("{}=on", $name));
            } else {
               pairs.push(format!("{}=", $name));
            }
         }
      };
   }
   macro_rules! enc_string {
      ($name:expr, $field:ident) => {
         if prefs.$field != defaults.$field {
            pairs.push(format!("{}={}", $name, prefs.$field));
         }
      };
   }

   enc_string!("theme", theme);
   enc_checkbox!("infiniteScroll", infinite_scroll);
   enc_checkbox!("stickyProfile", sticky_profile);
   enc_checkbox!("stickyNav", sticky_nav);
   enc_checkbox!("bidiSupport", bidi_support);
   enc_checkbox!("hideTweetStats", hide_tweet_stats);
   enc_checkbox!("hideBanner", hide_banner);
   enc_checkbox!("hidePins", hide_pins);
   enc_checkbox!("hideReplies", hide_replies);
   enc_checkbox!("hideCommunityNotes", hide_community_notes);
   enc_checkbox!("squareAvatars", square_avatars);
   enc_checkbox!("useTwemoji", use_twemoji);
   enc_checkbox!("mp4Playback", mp4_playback);
   enc_checkbox!("hlsPlayback", hls_playback);
   enc_checkbox!("proxyVideos", proxy_videos);
   enc_checkbox!("muteVideos", mute_videos);
   enc_checkbox!("autoplayGifs", autoplay_gifs);
   enc_string!("replaceTwitter", replace_twitter);
   enc_string!("replaceYouTube", replace_youtube);
   enc_string!("replaceReddit", replace_reddit);

   pairs.join(",")
}

fn make_cookie(name: &str, value: &str, max_age: Duration) -> Cookie<'static> {
   Cookie::build((name.to_owned(), value.to_owned()))
      .path("/")
      .max_age(max_age)
      .http_only(true)
      .build()
}

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/settings", get(settings))
      .route("/saveprefs", post(save_prefs))
      .route("/resetprefs", post(reset_prefs))
      .route("/enablehls", post(enable_hls))
      .route("/enablemp4", post(enable_mp4))
}

#[derive(Debug, Deserialize)]
pub struct PrefsForm {
   pub referer:              Option<String>,
   pub theme:                Option<String>,
   #[serde(rename = "infiniteScroll")]
   pub infinite_scroll:      Option<String>,
   #[serde(rename = "stickyProfile")]
   pub sticky_profile:       Option<String>,
   #[serde(rename = "bidiSupport")]
   pub bidi_support:         Option<String>,
   #[serde(rename = "hlsPlayback")]
   pub hls_playback:         Option<String>,
   #[serde(rename = "mp4Playback")]
   pub mp4_playback:         Option<String>,
   #[serde(rename = "proxyVideos")]
   pub proxy_videos:         Option<String>,
   #[serde(rename = "autoplayGifs")]
   pub autoplay_gifs:        Option<String>,
   #[serde(rename = "muteVideos")]
   pub mute_videos:          Option<String>,
   #[serde(rename = "hideTweetStats")]
   pub hide_tweet_stats:     Option<String>,
   #[serde(rename = "hideBanner")]
   pub hide_banner:          Option<String>,
   #[serde(rename = "hidePins")]
   pub hide_pins:            Option<String>,
   #[serde(rename = "hideReplies")]
   pub hide_replies:         Option<String>,
   #[serde(rename = "hideCommunityNotes")]
   pub hide_community_notes: Option<String>,
   #[serde(rename = "squareAvatars")]
   pub square_avatars:       Option<String>,
   #[serde(rename = "useTwemoji")]
   pub use_twemoji:          Option<String>,
   #[serde(rename = "stickyNav")]
   pub sticky_nav:           Option<String>,
   #[serde(rename = "replaceTwitter")]
   pub replace_twitter:      Option<String>,
   #[serde(rename = "replaceYouTube")]
   pub replace_youtube:      Option<String>,
   #[serde(rename = "replaceReddit")]
   pub replace_reddit:       Option<String>,
}

impl PrefsForm {
   fn to_prefs(&self, config: &Config) -> Prefs {
      let defaults = Prefs::with_defaults(config);
      let parse_bool =
         |field: &Option<String>, default: bool| field.as_ref().map_or(default, |val| val == "on");
      let parse_str = |field: &Option<String>, default: String| field.clone().unwrap_or(default);

      Prefs {
         theme:                parse_str(&self.theme, defaults.theme),
         infinite_scroll:      parse_bool(&self.infinite_scroll, defaults.infinite_scroll),
         sticky_profile:       parse_bool(&self.sticky_profile, defaults.sticky_profile),
         bidi_support:         parse_bool(&self.bidi_support, defaults.bidi_support),
         hls_playback:         parse_bool(&self.hls_playback, defaults.hls_playback),
         mp4_playback:         parse_bool(&self.mp4_playback, defaults.mp4_playback),
         proxy_videos:         parse_bool(&self.proxy_videos, defaults.proxy_videos),
         autoplay_gifs:        parse_bool(&self.autoplay_gifs, defaults.autoplay_gifs),
         mute_videos:          parse_bool(&self.mute_videos, defaults.mute_videos),
         hide_tweet_stats:     parse_bool(&self.hide_tweet_stats, defaults.hide_tweet_stats),
         hide_banner:          parse_bool(&self.hide_banner, defaults.hide_banner),
         hide_pins:            parse_bool(&self.hide_pins, defaults.hide_pins),
         hide_replies:         parse_bool(&self.hide_replies, defaults.hide_replies),
         hide_community_notes: parse_bool(
            &self.hide_community_notes,
            defaults.hide_community_notes,
         ),
         square_avatars:       parse_bool(&self.square_avatars, defaults.square_avatars),
         use_twemoji:          parse_bool(&self.use_twemoji, defaults.use_twemoji),
         sticky_nav:           parse_bool(&self.sticky_nav, defaults.sticky_nav),
         replace_twitter:      parse_str(&self.replace_twitter, defaults.replace_twitter),
         replace_youtube:      parse_str(&self.replace_youtube, defaults.replace_youtube),
         replace_reddit:       parse_str(&self.replace_reddit, defaults.replace_reddit),
      }
   }
}

#[derive(Debug, Deserialize)]
pub struct SettingsQuery {
   pub referer: Option<String>,
}

async fn settings(
   State(state): State<AppState>,
   jar: CookieJar,
   Query(query): Query<SettingsQuery>,
) -> Result<Response> {
   let prefs = Prefs::from_cookies(&jar, &state.config);
   let themes = find_themes(&state.config.server.static_dir);

   let referer = query.referer.as_deref().unwrap_or("/");
   let prefs_code = encode_prefs(&prefs, &state.config);
   let prefs_url = format!("{}/?prefs={}", state.config.url_prefix(), prefs_code);
   let content = pref_view::render_preferences_form(&prefs, &themes, referer, &prefs_url);
   let markup = layout::PageLayout::new(&state.config, "Settings", content)
      .description("Customize your teapot experience")
      .prefs(&prefs)
      .render();

   Ok(Html(markup.into_string()).into_response())
}

async fn save_prefs(
   State(state): State<AppState>,
   jar: CookieJar,
   Form(form): Form<PrefsForm>,
) -> Result<Response> {
   let prefs = form.to_prefs(&state.config);
   let cookies = prefs_to_cookies(&prefs);

   // Add all cookies to the jar
   let mut updated_jar = jar;
   for cookie in cookies {
      updated_jar = updated_jar.add(cookie);
   }

   // Redirect back to referer or settings page
   let redirect_to = form.referer.as_deref().unwrap_or("/settings");

   Ok((updated_jar, Redirect::to(redirect_to)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct ResetPrefsForm {
   pub referer: Option<String>,
}

async fn reset_prefs(jar: CookieJar, Form(form): Form<ResetPrefsForm>) -> Result<Response> {
   // Remove all preference cookies
   let mut updated_jar = jar;
   for name in Prefs::COOKIE_NAMES {
      let removal = Cookie::build(name.to_string())
         .path("/")
         .max_age(Duration::seconds(0))
         .build();
      updated_jar = updated_jar.remove(removal);
   }

   // Redirect to referer or settings page
   let redirect_to = form.referer.as_deref().unwrap_or("/settings");
   Ok((updated_jar, Redirect::to(redirect_to)).into_response())
}

async fn enable_hls(jar: CookieJar, headers: HeaderMap) -> Result<Response> {
   let cookie = Cookie::build(("hlsPlayback".to_owned(), "on".to_owned()))
      .path("/")
      .max_age(Duration::days(365))
      .http_only(true)
      .build();

   let updated_jar = jar.add(cookie);
   let referer = headers
      .get(REFERER)
      .and_then(|hv| hv.to_str().ok())
      .unwrap_or("/settings");

   Ok((updated_jar, Redirect::to(referer)).into_response())
}

async fn enable_mp4(jar: CookieJar, headers: HeaderMap) -> Result<Response> {
   let cookie = Cookie::build(("mp4Playback".to_owned(), "on".to_owned()))
      .path("/")
      .max_age(Duration::days(365))
      .http_only(true)
      .build();

   let updated_jar = jar.add(cookie);
   let referer = headers
      .get(REFERER)
      .and_then(|hv| hv.to_str().ok())
      .unwrap_or("/settings");

   Ok((updated_jar, Redirect::to(referer)).into_response())
}

/// Discover available themes from CSS files in the static directory.
#[expect(
   clippy::absolute_paths,
   reason = "crate's Result type shadows std::result::Result"
)]
fn find_themes(static_dir: &str) -> Vec<String> {
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
