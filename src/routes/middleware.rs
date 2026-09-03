//! Request middleware.

use std::net::SocketAddr;

use axum::{
   extract::{
      ConnectInfo,
      Path,
      Request,
      State,
   },
   http::{
      HeaderValue,
      header,
   },
   middleware::Next,
   response::{
      IntoResponse as _,
      Redirect,
      Response,
   },
};
use axum_extra::extract::{
   CookieJar,
   cookie::{
      Cookie,
      SameSite,
   },
};
use time::Duration;

use crate::{
   AppState,
   api::budget,
   error::Error,
   types::Prefs,
};

/// Refuse any route whose `{id}` cannot be a snowflake before a handler spends
/// cookies, prefs or a session on it.
pub async fn snowflake_guard(
   params: Option<Path<Vec<(String, String)>>>,
   request: Request,
   next: Next,
) -> Response {
   let bad_id = params
      .as_ref()
      .and_then(|path| path.0.iter().find(|pair| pair.0 == "id"))
      .is_some_and(|pair| {
         let id = &pair.1;
         !(1..=19).contains(&id.len()) || !id.bytes().all(|byte| byte.is_ascii_digit())
      });
   if bad_id {
      return Error::InvalidUrl("Invalid id".into()).into_response();
   }
   next.run(request).await
}

/// Bind the caller to the request so the API client can bill upstream calls to
/// it without every route having to thread it through.
pub async fn client_middleware(
   State(state): State<AppState>,
   ConnectInfo(peer): ConnectInfo<SocketAddr>,
   request: Request,
   next: Next,
) -> Response {
   let client = budget::client_from(peer.ip(), request.headers(), &state.trusted_proxies);
   budget::CLIENT.scope(client, next.run(request)).await
}

/// Middleware that applies `?prefs=` URL parameter overrides.
pub async fn prefs_middleware(mut request: Request, next: Next) -> Response {
   let uri = request.uri().clone();
   let query_string = uri.query().unwrap_or("");

   // Check if ?prefs= parameter exists
   let prefs_param = form_urlencoded::parse(query_string.as_bytes())
      .find(|&(ref key, _)| key == "prefs")
      .map(|(_, val)| val.to_string());

   if let Some(prefs_value) = prefs_param {
      // Parse prefs in "key=val,key2=val2" form
      let mut jar = CookieJar::new();
      let pref_names = Prefs::URL_PREF_NAMES;

      for pair in prefs_value.split(',') {
         let (key, value) = match pair.split_once('=') {
            Some((pkey, pval)) => (pkey, pval),
            None if !pair.is_empty() => (pair, ""),
            _ => continue,
         };

         if pref_names.contains(&key) {
            let cookie = Cookie::build((key.to_owned(), value.to_owned()))
               .path("/")
               .max_age(Duration::days(365))
               .http_only(true)
               .same_site(SameSite::Lax)
               .build();
            jar = jar.add(cookie);
         }
      }

      // Rebuild URL without prefs param
      let path = uri.path();
      let clean_params = form_urlencoded::parse(query_string.as_bytes())
         .filter(|&(ref key, _)| key != "prefs")
         .map(|(key, val)| {
            if val.is_empty() {
               key.to_string()
            } else {
               format!("{key}={val}")
            }
         })
         .collect::<Vec<_>>();
      let redirect_url = if clean_params.is_empty() {
         path.to_owned()
      } else {
         format!("{}?{}", path, clean_params.join("&"))
      };

      return (jar, Redirect::to(&redirect_url)).into_response();
   }

   // Individual preference parameters are transient. Inject them into this
   // request's Cookie header so every existing CookieJar extractor sees the
   // override, without emitting Set-Cookie or changing the URL.
   let overrides = form_urlencoded::parse(query_string.as_bytes())
      .filter(|&(ref key, ref value)| {
         Prefs::URL_PREF_NAMES.contains(&key.as_ref()) && valid_cookie_value(value)
      })
      .map(|(key, value)| (key.into_owned(), value.into_owned()))
      .collect::<Vec<_>>();
   if !overrides.is_empty() {
      let mut cookies = request
         .headers()
         .get(header::COOKIE)
         .and_then(|value| value.to_str().ok())
         .unwrap_or_default()
         .split(';')
         .map(str::trim)
         .filter(|cookie| {
            cookie
               .split_once('=')
               .is_none_or(|(name, _)| !overrides.iter().any(|&(ref key, _)| key == name))
         })
         .filter(|cookie| !cookie.is_empty())
         .map(str::to_owned)
         .collect::<Vec<_>>();
      cookies.extend(
         overrides
            .into_iter()
            .map(|(key, value)| format!("{key}={value}")),
      );
      if let Ok(value) = HeaderValue::from_str(&cookies.join("; ")) {
         request.headers_mut().insert(header::COOKIE, value);
      }
   }

   next.run(request).await
}

fn valid_cookie_value(value: &str) -> bool {
   value.len() <= 512
      && value
         .bytes()
         .all(|byte| matches!(byte, 0x21 | 0x23..=0x2B | 0x2D..=0x3A | 0x3C..=0x5B | 0x5D..=0x7E))
}

#[cfg(test)]
mod tests {
   use axum::{
      Router,
      body::Body,
      http::{
         Request,
         StatusCode,
         header,
      },
      middleware,
      response::IntoResponse,
      routing::get,
   };
   use axum_extra::extract::CookieJar;
   use http_body_util::BodyExt as _;
   use tower::ServiceExt as _;

   use super::prefs_middleware;

   async fn preference_value(jar: CookieJar) -> impl IntoResponse {
      jar.get("mp4Playback")
         .map_or_else(String::new, |cookie| cookie.value().to_owned())
         .into_response()
   }

   #[tokio::test]
   async fn individual_preference_query_overrides_only_current_request() {
      let app = Router::new()
         .route("/", get(preference_value))
         .layer(middleware::from_fn(prefs_middleware));
      let response = app
         .oneshot(
            Request::builder()
               .uri("/?mp4Playback=on")
               .body(Body::empty())
               .unwrap(),
         )
         .await
         .unwrap();

      assert_eq!(response.status(), StatusCode::OK);
      assert!(response.headers().get(header::SET_COOKIE).is_none());
      let body = response.into_body().collect().await.unwrap().to_bytes();
      assert_eq!(body, "on");
   }
}
