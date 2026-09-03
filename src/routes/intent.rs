use axum::{
   Router,
   extract::{
      Query,
      State,
   },
   response::{
      Html,
      IntoResponse as _,
      Redirect,
      Response,
   },
   routing::get,
};
use maud::html;
use serde::Deserialize;

use super::unsupported::render_unsupported_feature;
use crate::{
   AppState,
   error::Result,
   views::layout,
};

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/intent/user", get(intent_user))
      .route("/intent/follow", get(intent_follow))
      .route("/intent/{*path}", get(intent_unsupported))
}

/// Catch-all for unsupported intent handlers.
async fn intent_unsupported(State(state): State<AppState>) -> Response {
   render_unsupported_feature(&state, "Intent").into_response()
}

#[derive(Debug, Deserialize)]
pub struct IntentUserQuery {
   pub user_id:     Option<String>,
   pub screen_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IntentFollowQuery {
   pub screen_name: Option<String>,
}

/// Handle `/intent/user` - redirect to user profile.
async fn intent_user(
   State(state): State<AppState>,
   Query(query): Query<IntentUserQuery>,
) -> Result<Response> {
   if let Some(ref screen_name) = query.screen_name
      && !screen_name.is_empty()
   {
      return Ok(Redirect::to(&format!("/{screen_name}")).into_response());
   }

   if let Some(ref user_id) = query.user_id
      && !user_id.is_empty()
   {
      // For now, redirect to /i/user/{id} which should handle the lookup
      // In a full implementation, we'd look up the user by ID
      return Ok(Redirect::to(&format!("/i/user/{user_id}")).into_response());
   }

   let content = html! {
       div class="error-page" {
           h1 { "Invalid Intent" }
           p { "Please provide either a screen_name or user_id parameter." }
       }
   };

   let markup = layout::PageLayout::new(&state.config, "Invalid Intent", content).render();
   Ok(Html(markup.into_string()).into_response())
}

/// Handle `/intent/follow` - redirect to user profile (following requires login
/// on Twitter).
async fn intent_follow(
   State(state): State<AppState>,
   Query(query): Query<IntentFollowQuery>,
) -> Result<Response> {
   if let Some(ref screen_name) = query.screen_name
      && !screen_name.is_empty()
   {
      return Ok(Redirect::to(&format!("/{screen_name}")).into_response());
   }

   let content = html! {
       div class="error-page" {
           h1 { "Invalid Intent" }
           p { "Please provide a screen_name parameter." }
       }
   };

   let markup = layout::PageLayout::new(&state.config, "Invalid Intent", content).render();
   Ok(Html(markup.into_string()).into_response())
}
