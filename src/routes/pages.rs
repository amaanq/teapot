//! Static pages served by teapot itself.

use axum::{
   Router,
   extract::State,
   response::{
      Html,
      IntoResponse,
      Redirect,
   },
   routing::get,
};
use axum_extra::extract::CookieJar;
use maud::html;

use crate::{
   AppState,
   types::prefs::Prefs,
   views::{
      layout::PageLayout,
      search as search_view,
   },
};

/// The pages teapot serves itself rather than fetching from X.
pub fn router() -> Router<AppState> {
   Router::new()
      .route("/", get(home))
      .route("/about", get(about))
      .route("/explore", get(|| async { Redirect::to("/about") }))
      .route("/help", get(|| async { Redirect::to("/about") }))
}

async fn home(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
   let prefs = Prefs::from_cookies(&jar, &state.config);
   let content = search_view::render_search_page(&state.config);

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
               li { "No third-party JavaScript or ads" }
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
               "Version: teapot " (env!("CARGO_PKG_VERSION"))
           }
       }
   };

   let markup = PageLayout::new(&state.config, "About", content)
      .description("About teapot")
      .prefs(&prefs)
      .render();
   Html(markup.into_string())
}
