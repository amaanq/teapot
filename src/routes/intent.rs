use topcoat::{
   context::Cx,
   router::{
      error::see_other,
      page,
      query_params,
   },
   view::{
      View,
      view,
   },
};

use crate::{
   routes::helpers::request_prefs,
   views::document::document,
};

#[query_params(error = bad_request)]
struct IntentQuery {
   user_id:     Option<String>,
   screen_name: Option<String>,
}

/// Handle `/intent/user` - redirect to user profile.
#[page("/intent/user")]
pub async fn intent_user(cx: &Cx) -> topcoat::Result<impl View> {
   let query = query_params::<IntentQuery>(cx)?;
   if let Some(screen_name) = query.screen_name.as_deref() {
      return Err(see_other(format!("/{screen_name}")).into());
   }
   if let Some(user_id) = query.user_id.as_deref() {
      // For now, redirect to /i/user/{id} which should handle the lookup
      // In a full implementation, we'd look up the user by ID
      return Err(see_other(format!("/i/user/{user_id}")).into());
   }
   Ok(invalid_intent(
      cx,
      "Please provide either a screen_name or user_id parameter.",
   ))
}

/// Handle `/intent/follow` - redirect to user profile (following requires login
/// on Twitter).
#[page("/intent/follow")]
pub async fn intent_follow(cx: &Cx) -> topcoat::Result<impl View> {
   if let Some(screen_name) = query_params::<IntentQuery>(cx)?.screen_name.as_deref() {
      return Err(see_other(format!("/{screen_name}")).into());
   }
   Ok(invalid_intent(
      cx,
      "Please provide a screen_name parameter.",
   ))
}

fn invalid_intent<'a>(cx: &'a Cx, message: &'static str) -> impl View + 'a {
   let prefs = request_prefs(cx);
   view! { cx =>
      document(
         title: "Invalid Intent",
         prefs: &prefs,
         <div class="error-page">
            <h1>"Invalid Intent"</h1>
            <p>(message)</p>
         </div>
      )
   }
}
