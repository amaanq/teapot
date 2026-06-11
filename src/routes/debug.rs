use axum::{
   Json,
   Router,
   extract::State,
   http::{
      HeaderMap,
      StatusCode,
      header,
   },
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
async fn health_check(State(state): State<AppState>, headers: HeaderMap) -> Response {
   if !debug_authorized(&state, &headers) {
      return StatusCode::NOT_FOUND.into_response();
   }
   Json(state.api.get_session_health().await).into_response()
}

/// Detailed sessions debug endpoint.
async fn sessions_debug(State(state): State<AppState>, headers: HeaderMap) -> Response {
   if !debug_authorized(&state, &headers) {
      return StatusCode::NOT_FOUND.into_response();
   }
   Json(state.api.get_session_debug().await).into_response()
}

fn debug_authorized(state: &AppState, headers: &HeaderMap) -> bool {
   if !state.config.config.enable_debug || state.config.config.debug_token.is_empty() {
      return false;
   }

   let Some(auth) = headers
      .get(header::AUTHORIZATION)
      .and_then(|value| value.to_str().ok())
   else {
      return false;
   };
   let Some(token) = auth.strip_prefix("Bearer ") else {
      return false;
   };

   constant_time_eq(token.as_bytes(), state.config.config.debug_token.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
   if left.len() != right.len() {
      return false;
   }

   let mut diff = 0_u8;
   for (&lhs, &rhs) in left.iter().zip(right) {
      diff |= lhs ^ rhs;
   }
   diff == 0
}
