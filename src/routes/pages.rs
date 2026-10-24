//! Static pages served by teapot itself.

use axum::{
   Router,
   response::Redirect,
   routing::get,
};
use topcoat::{
   context::Cx,
   router::page,
   view::{
      View,
      view,
   },
};

use crate::{
   AppState,
   routes::helpers::request_prefs,
   views::{
      document::document,
      search::landing_search,
   },
};

/// The redirects teapot serves itself rather than fetching from X.
pub fn router() -> Router<AppState> {
   Router::new()
      .route("/explore", get(|| async { Redirect::to("/about") }))
      .route("/help", get(|| async { Redirect::to("/about") }))
}

#[page("/")]
pub async fn home(cx: &Cx) -> topcoat::Result<impl View> {
   let prefs = request_prefs(cx);
   Ok(view! {
      document(
         title: "Home",
         prefs: &prefs,
         description: "A privacy-focused Twitter/X frontend",
         landing_search()
      )
   })
}

#[page("/about")]
pub async fn about(cx: &Cx) -> topcoat::Result<impl View> {
   let prefs = request_prefs(cx);
   Ok(view! {
      document(
         title: "About",
         prefs: &prefs,
         description: "About teapot",
         <div class="overlay-panel">
            <h1>"About"</h1>
            <p>
               "teapot is a free and open source alternative Twitter front-end focused on privacy and performance. "
               "The source is available on GitHub at "
               <a href="https://github.com/amaanq/teapot">"https://github.com/amaanq/teapot"</a>
            </p>
            <ul>
               <li>"No third-party JavaScript or ads"</li>
               <li>"All requests go through the backend, client never talks to Twitter"</li>
               <li>"Prevents Twitter from tracking your IP or JavaScript fingerprint"</li>
               <li>"Uses Twitter's unofficial API (no developer account required)"</li>
               <li>"Lightweight"</li>
               <li>"RSS feeds"</li>
               <li>"Themes"</li>
               <li>"Mobile support (responsive design)"</li>
               <li>"AGPLv3 licensed, no proprietary instances permitted"</li>
            </ul>
            <p>
               "teapot's GitHub wiki contains "
               <a href="https://github.com/amaanq/teapot/wiki/Instances">"instances"</a>
               " and "
               <a href="https://github.com/amaanq/teapot/wiki/Extensions">"browser extensions"</a>
               " maintained by the community."
            </p>
            <h2>"Why use teapot?"</h2>
            <p>
               "It's impossible to use Twitter without JavaScript enabled, and as of 2024 you need to sign up. "
               "For privacy-minded folks, preventing JavaScript analytics and IP-based tracking is important, "
               "but apart from using a VPN and uBlock/uMatrix, it's impossible. Despite being behind a VPN and "
               "using heavy-duty adblockers, you can get accurately tracked with your "
               <a href="https://restoreprivacy.com/browser-fingerprinting/">"browser's fingerprint"</a>
               ", "
               <a href="https://noscriptfingerprint.com/">"no JavaScript required"</a>
               ". This all became particularly important after Twitter "
               <a href="https://www.eff.org/deeplinks/2020/04/twitter-removes-privacy-option-and-shows-why-we-need-strong-privacy-laws">
                  "removed the ability"
               </a>
               " for users to control whether their data gets sent to advertisers."
            </p>
            <p>
               "Using an instance of teapot (hosted on a VPS for example), you can browse Twitter without "
               "JavaScript while retaining your privacy. In addition to respecting your privacy, teapot is on "
               "average around 15 times lighter than Twitter, and in most cases serves pages faster "
               "(eg. timelines load 2-4x faster)."
            </p>
            <h2>"Instance info"</h2>
            <p>"Version: teapot " (env!("CARGO_PKG_VERSION"))</p>
         </div>
      )
   })
}
