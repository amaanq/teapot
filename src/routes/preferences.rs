#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[page] and view! expansions"
)]

use axum::{
   Form,
   Router,
   extract::State,
   http::{
      HeaderMap,
      header::REFERER,
   },
   middleware,
   response::{
      IntoResponse as _,
      Redirect,
      Response,
   },
   routing::post,
};
use axum_extra::extract::cookie::{
   Cookie,
   CookieJar,
   SameSite,
};
use cookie::time::Duration;
use percent_encoding::{
   NON_ALPHANUMERIC,
   utf8_percent_encode,
};
use serde::Deserialize;
use topcoat::{
   context::Cx,
   cookie::{
      Cookies as _,
      cookies,
   },
   router::{
      error::RouterErrorExt as _,
      page,
      query_params,
   },
   runtime::procedure,
   view::{
      View,
      view,
   },
};

use crate::{
   AppState,
   config::Config,
   error::Result,
   routes::{
      helpers::{
         request_prefs,
         state,
      },
      middleware::same_origin_guard,
   },
   types::prefs::{
      Prefs,
      Text,
      Toggle,
   },
   views::{
      document::{
         LivePrefs,
         document,
         themes,
      },
      preferences::preferences_form,
   },
};

/// Convert preferences to HTTP cookies for storage.
fn prefs_to_cookies<'a>(
   prefs: &'a Prefs,
   config: &Config,
) -> impl Iterator<Item = Cookie<'static>> + 'a {
   let secure = config.server.https;
   let toggles = Toggle::ALL
      .iter()
      .map(move |&toggle| toggle_cookie(toggle, toggle.field(prefs), secure));
   let texts = Text::ALL
      .iter()
      .map(move |&text| text_cookie(text, text.field(prefs), secure));
   toggles.chain(texts)
}

fn toggle_cookie(toggle: Toggle, on: bool, secure: bool) -> Cookie<'static> {
   make_cookie(toggle.name(), if on { "on" } else { "" }, secure, true)
}

fn text_cookie(text: Text, value: &str, secure: bool) -> Cookie<'static> {
   make_cookie(text.name(), value, secure, text != Text::Theme)
}

/// Encode non-default prefs as a comma-separated string for URL sharing.
fn encode_prefs(prefs: &Prefs, config: &Config) -> String {
   let defaults = Prefs::with_defaults(config);
   let toggles = Toggle::ALL
      .iter()
      .filter(|toggle| toggle.field(prefs) != toggle.field(&defaults))
      .map(|toggle| {
         let value = if toggle.field(prefs) { "on" } else { "" };
         format!("{}={value}", toggle.name())
      });
   let texts = Text::ALL
      .iter()
      .filter(|text| text.shareable() && text.field(prefs) != text.field(&defaults))
      .map(|text| format!("{}={}", text.name(), text.field(prefs)));
   toggles.chain(texts).collect::<Vec<_>>().join(",")
}

fn make_cookie(name: &str, value: &str, secure: bool, http_only: bool) -> Cookie<'static> {
   Cookie::build((name.to_owned(), value.to_owned()))
      .path("/")
      .max_age(Duration::days(365))
      .http_only(http_only)
      .same_site(SameSite::Lax)
      .secure(secure)
      .build()
}

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/saveprefs", post(save_prefs))
      .route("/resetprefs", post(reset_prefs))
      .route("/enablemp4", post(enable_mp4))
      .route_layer(middleware::from_fn(same_origin_guard))
}

#[query_params(error = bad_request)]
struct SettingsQuery {
   referer: Option<String>,
}

#[page("/settings")]
pub async fn settings(cx: &Cx) -> topcoat::Result<impl View> {
   let config = &state(cx).config;
   let prefs = request_prefs(cx);
   let live = LivePrefs::new(cx, &prefs, themes(&config.server.static_dir));

   let query = query_params::<SettingsQuery>(cx)?;
   let referer = local_redirect_target(query.referer.as_deref(), "/");
   let prefs_code = encode_prefs(&prefs, config);
   let encoded_prefs = utf8_percent_encode(&prefs_code, NON_ALPHANUMERIC).to_string();
   let prefs_url = format!("{}/?prefs={encoded_prefs}", config.url_prefix());

   Ok(view! {
      document(
         title: "Settings",
         prefs: &prefs,
         live: Some(&live),
         description: "Customize your teapot experience",
         preferences_form(
            prefs: &prefs,
            live: &live,
            referer: &referer,
            prefs_url: &prefs_url,
         )
      )
   })
}

#[procedure("/_topcoat/settings/toggle")]
pub async fn save_toggle(cx: &Cx, name: String, on: bool) -> topcoat::Result<()> {
   let toggle = name
      .parse::<Toggle>()
      .ok_or_bad_request("unknown preference")?;
   let secure = state(cx).config.server.https;
   cookies(cx).add(toggle_cookie(toggle, on, secure));
   Ok(())
}

#[procedure("/_topcoat/settings/text")]
pub async fn save_text(cx: &Cx, name: String, value: String) -> topcoat::Result<()> {
   let text = name
      .parse::<Text>()
      .ok_or_bad_request("unknown preference")?;
   Text::accepts(&value)
      .then_some(())
      .ok_or_bad_request("invalid preference value")?;
   let secure = state(cx).config.server.https;
   cookies(cx).add(text_cookie(text, &value, secure));
   Ok(())
}

async fn save_prefs(
   State(state): State<AppState>,
   jar: CookieJar,
   Form(fields): Form<Vec<(String, String)>>,
) -> Result<Response> {
   let field = |name: &str| {
      fields
         .iter()
         .find(|pair| pair.0 == name)
         .map(|pair| pair.1.as_str())
   };

   let mut prefs = Prefs::from_cookies(&jar, &state.config);
   // Unchecked checkboxes are absent from form data and therefore false.
   for &toggle in Toggle::ALL {
      *toggle.field_mut(&mut prefs) = field(toggle.name()) == Some("on");
   }
   for text in Text::ALL {
      if let Some(value) = field(text.name()).filter(|value| Text::accepts(value)) {
         value.clone_into(text.field_mut(&mut prefs));
      }
   }

   let updated_jar = prefs_to_cookies(&prefs, &state.config).fold(jar, CookieJar::add);
   let redirect_to = local_redirect_target(field("referer"), "/settings");
   Ok((updated_jar, Redirect::to(&redirect_to)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct ResetPrefsForm {
   pub referer: Option<String>,
}

async fn reset_prefs(jar: CookieJar, Form(form): Form<ResetPrefsForm>) -> Result<Response> {
   let mut updated_jar = jar;
   for name in Prefs::cookie_names() {
      let removal = Cookie::build(name)
         .path("/")
         .max_age(Duration::seconds(0))
         .build();
      updated_jar = updated_jar.remove(removal);
   }

   let redirect_to = local_redirect_target(form.referer.as_deref(), "/settings");
   Ok((updated_jar, Redirect::to(&redirect_to)).into_response())
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
      .filter(|value| is_local_path(value))
      .unwrap_or("/settings");

   Ok((updated_jar, Redirect::to(referer)).into_response())
}

fn local_redirect_target(value: Option<&str>, fallback: &str) -> String {
   value
      .filter(|target| is_local_path(target))
      .unwrap_or(fallback)
      .to_owned()
}

fn is_local_path(value: &str) -> bool {
   value.starts_with('/')
      && !value.starts_with("//")
      && !value.chars().any(|ch| matches!(ch, '\r' | '\n'))
}
