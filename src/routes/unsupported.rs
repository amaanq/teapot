#![expect(
   clippy::same_name_method,
   clippy::shadow_same,
   clippy::shadow_unrelated,
   reason = "topcoat's #[page] and view! expansions"
)]

use topcoat::{
   context::Cx,
   router::page,
   view::{
      View,
      component,
      view,
   },
};

use crate::{
   routes::helpers::request_prefs,
   views::document::document,
};

#[page("/login")]
pub async fn login(cx: &Cx) -> topcoat::Result<impl View> {
   Ok(login_view(cx))
}

#[page("/login/{*path}")]
pub async fn login_subpath(cx: &Cx) -> topcoat::Result<impl View> {
   Ok(login_view(cx))
}

fn login_view(cx: &Cx) -> impl View + '_ {
   let prefs = request_prefs(cx);
   view! { cx =>
      document(
         title: "Login Not Supported",
         prefs: &prefs,
         <div class="overlay-panel">
            <h1>"Login Not Supported"</h1>
            <p>"teapot does not support logging in to Twitter."</p>
            <p>"teapot is a privacy-focused frontend that allows you to browse Twitter without an account."</p>
            <p><a href="/">"Go to Homepage"</a></p>
         </div>
      )
   }
}

#[page("/about/feature")]
pub async fn about_feature(cx: &Cx) -> topcoat::Result<impl View> {
   Ok(unsupported(cx, "Feature"))
}

/// Catch-all for unsupported `/i/*` routes.
#[page("/i/{*path}")]
pub async fn i_unsupported(cx: &Cx) -> topcoat::Result<impl View> {
   Ok(unsupported(cx, "Feature"))
}

/// Catch-all for unsupported intent handlers.
#[page("/intent/{*path}")]
pub async fn intent_unsupported(cx: &Cx) -> topcoat::Result<impl View> {
   Ok(unsupported(cx, "Intent"))
}

fn unsupported<'a>(cx: &'a Cx, feature: &'a str) -> impl View + 'a {
   let prefs = request_prefs(cx);
   let title = format!("{feature} Not Available");
   view! { cx =>
      document(
         title: &title,
         prefs: &prefs,
         unsupported_panel()
      )
   }
}

#[component]
async fn unsupported_panel() -> topcoat::Result<impl View> {
   Ok(view! {
      <div class="overlay-panel">
         <h1>"Unsupported feature"</h1>
         <p>
            "teapot doesn't support this feature yet, but it might in the future. "
            "You can check for an issue and open one if needed here: "
            <a href="https://github.com/amaanq/teapot/issues" target="_blank">
               "https://github.com/amaanq/teapot/issues"
            </a>
         </p>
         <p>"To find out more about the teapot project, see the " <a href="/about">"About page"</a></p>
      </div>
   })
}
