mod debug;
mod embed;
pub mod helpers;
mod intent;
mod list;
mod media;
mod preferences;
mod redirect;
mod rss;
mod search;
mod status;
mod timeline;

mod unsupported;

use axum::{
   Router,
   extract::{
      Request,
      State,
   },
   middleware::Next,
   response::{
      Html,
      IntoResponse,
      Redirect,
      Response,
   },
   routing::get,
};
use axum_extra::extract::{
   CookieJar,
   cookie::Cookie,
};
use maud::html;
use time::Duration;

use crate::{
   AppState,
   types::Prefs,
   views::{
      layout::PageLayout,
      search as search_view,
   },
};

pub fn router() -> Router<AppState> {
   // ROUTE ORDERING RULES:
   //
   // Axum matches routes in merge order. The following constraints MUST hold:
   //
   // 1. embed::router() BEFORE status::router()
   //    - /users/{username}/statuses/{id} (ActivityPub) must not fall through to
   //      status handler
   //
   // 2. status::router() BEFORE timeline::router()
   //    - /{username}/status/{id} must match before the /{username} catch-all
   //
   // 3. All specific /i/* routes BEFORE unsupported::i_catchall_router()
   //    - /i/status/{id}, /i/user/{id} are handled by status::router()
   //    - /i/redirect is handled by redirect::router()
   //    - Everything else falls through to the /i/{*path} catch-all
   //
   // 4. timeline::router() MUST BE LAST
   //    - /{username} is a greedy catch-all that matches any single-segment path
   //    - timeline.rs validates against RESERVED_PATHS to reject non-usernames
   //
   // 5. media::router() BEFORE timeline::router()
   //    - /pic/* and /video/* must not match as usernames
   //
   // 6. list::router() BEFORE timeline::router()
   //    - /{username}/lists must match before /{username} catch-all sub-routes

   Router::new()
        // ── Fixed routes (no ordering constraints) ──
        .route("/", get(home))
        .route("/about", get(about))
        .route("/explore", get(|| async { Redirect::to("/about") }))
        .route("/help", get(|| async { Redirect::to("/about") }))
        .merge(unsupported::router())
        .merge(debug::router())
        .merge(redirect::router())
        .merge(intent::router())
        .merge(search::router())
        .merge(preferences::router())
        // ── Order-sensitive routes (see rules above) ──
        .merge(embed::router())                    // Rule 1: before status
        .merge(status::router())                   // Rule 2: before timeline
        .merge(media::router())                    // Rule 5: before timeline
        .merge(rss::router())                      // Has /{username}/rss, before timeline
        .merge(list::router())                     // Rule 6: before timeline
        .merge(unsupported::i_catchall_router())   // Rule 3: after specific /i/* routes
        .merge(timeline::router()) // Rule 4: MUST BE LAST
}

/// Middleware: apply ?prefs= URL parameter overrides.
/// Parses `?prefs=key1=val1,key2=val2` and individual `?key=val`
/// params, sets them as cookies, then redirects to the clean URL.
pub async fn prefs_middleware(request: Request, next: Next) -> Response {
   let uri = request.uri().clone();
   let query_string = uri.query().unwrap_or("");

   // Check if ?prefs= parameter exists
   let prefs_param = form_urlencoded::parse(query_string.as_bytes())
      .find(|&(ref key, _)| key == "prefs")
      .map(|(_, val)| val.to_string());

   if let Some(prefs_value) = prefs_param {
      // Parse prefs: "key=val,key2=val2"
      let mut jar = CookieJar::new();
      let pref_names = Prefs::COOKIE_NAMES;

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

   // Also handle individual pref params (e.g., ?mp4Playback=on) for cookieless
   // overrides These don't redirect — they just override the cookie jar for
   // this request
   next.run(request).await
}

async fn home(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
   let prefs = Prefs::from_cookies(&jar, &state.config);
   let content = search_view::render_search_page();

   let markup = PageLayout::new(&state.config, "Home", content)
      .description("A privacy-focused Twitter/X frontend")
      .prefs(&prefs)
      .render();
   Html(markup.into_string())
}

async fn about(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
   let prefs = Prefs::from_cookies(&jar, &state.config);
   let content = html! {
       div class="overlay-panel" {
           h1 { "About" }

           p {
               "teapot is a free and open source alternative Twitter front-end focused on privacy and performance. "
               "The source is available on GitHub at "
               a href="https://github.com/amaanq/teapot" { "https://github.com/amaanq/teapot" }
           }

           ul {
               li { "No JavaScript or ads" }
               li { "All requests go through the backend, client never talks to Twitter" }
               li { "Prevents Twitter from tracking your IP or JavaScript fingerprint" }
               li { "Uses Twitter's unofficial API (no developer account required)" }
               li { "Lightweight" }
               li { "RSS feeds" }
               li { "Themes" }
               li { "Mobile support (responsive design)" }
               li { "AGPLv3 licensed, no proprietary instances permitted" }
           }

           p {
               "teapot's GitHub wiki contains "
               a href="https://github.com/amaanq/teapot/wiki/Instances" { "instances" }
               " and "
               a href="https://github.com/amaanq/teapot/wiki/Extensions" { "browser extensions" }
               " maintained by the community."
           }

           h2 { "Why use teapot?" }

           p {
               "It's impossible to use Twitter without JavaScript enabled, and as of 2024 you need to sign up. "
               "For privacy-minded folks, preventing JavaScript analytics and IP-based tracking is important, "
               "but apart from using a VPN and uBlock/uMatrix, it's impossible. Despite being behind a VPN and "
               "using heavy-duty adblockers, you can get accurately tracked with your "
               a href="https://restoreprivacy.com/browser-fingerprinting/" { "browser's fingerprint" }
               ", "
               a href="https://noscriptfingerprint.com/" { "no JavaScript required" }
               ". This all became particularly important after Twitter "
               a href="https://www.eff.org/deeplinks/2020/04/twitter-removes-privacy-option-and-shows-why-we-need-strong-privacy-laws" { "removed the ability" }
               " for users to control whether their data gets sent to advertisers."
           }

           p {
               "Using an instance of teapot (hosted on a VPS for example), you can browse Twitter without "
               "JavaScript while retaining your privacy. In addition to respecting your privacy, teapot is on "
               "average around 15 times lighter than Twitter, and in most cases serves pages faster "
               "(eg. timelines load 2-4x faster)."
           }

           h2 { "Instance info" }
           p {
               "Version: teapot"
           }
       }
   };

   let markup = PageLayout::new(&state.config, "About", content)
      .description("About teapot")
      .prefs(&prefs)
      .render();
   Html(markup.into_string())
}
