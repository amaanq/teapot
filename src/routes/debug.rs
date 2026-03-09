use axum::{
   Json,
   Router,
   extract::State,
   http::StatusCode,
   response::{
      IntoResponse as _,
      Response,
   },
   routing::get,
};

use crate::AppState;

pub fn router() -> Router<AppState> {
   Router::new()
      .route("/.health", get(health_check))
      .route("/.sessions", get(sessions_debug))
}

/// Health check endpoint returning session pool statistics.
async fn health_check(State(state): State<AppState>) -> Response {
   if !state.config.config.enable_debug {
      return StatusCode::NOT_FOUND.into_response();
   }
   Json(state.api.get_session_health().await).into_response()
}

/// Detailed sessions debug endpoint.
async fn sessions_debug(State(state): State<AppState>) -> Response {
   if !state.config.config.enable_debug {
      return StatusCode::NOT_FOUND.into_response();
   }
   Json(state.api.get_session_debug().await).into_response()
}
