use std::{
   io,
   result,
   sync::{
      OnceLock,
      atomic::{
         AtomicUsize,
         Ordering,
      },
   },
};

use axum::{
   http::StatusCode,
   response::{
      Html,
      IntoResponse,
      Response,
   },
};
use thiserror::Error;
use toml::de::Error as TomlError;

use crate::{
   utils::html_escape,
   views::layout::{
      FONTELLO_CSS,
      STYLE_CSS,
   },
};

pub type Result<T> = result::Result<T, Error>;

/// Operator-facing wording, shared by the error page and by `?` propagation so
/// the two cannot drift.
#[derive(Debug, Default)]
pub struct Messages {
   pub client_budget:  Vec<String>,
   pub rate_limited:   Vec<String>,
   pub internal_error: Vec<String>,
}

static MESSAGES: OnceLock<Messages> = OnceLock::new();

/// Install the configured wording. Called once at startup.
pub fn set_messages(messages: Messages) {
   let _ = MESSAGES.set(messages);
}

fn configured() -> &'static Messages {
   static EMPTY: Messages = Messages {
      client_budget:  Vec::new(),
      rate_limited:   Vec::new(),
      internal_error: Vec::new(),
   };
   MESSAGES.get().unwrap_or(&EMPTY)
}

/// Rotate through the configured wordings, one per refusal.
fn pick(configured: &'static [String], fallback: &'static str) -> &'static str {
   static NEXT: AtomicUsize = AtomicUsize::new(0);
   match *configured {
      [] => fallback,
      [ref only] => only,
      ref many => &many[NEXT.fetch_add(1, Ordering::Relaxed) % many.len()],
   }
}

pub const CLIENT_BUDGET_MESSAGE: &str = "You are requesting uncached pages faster than this \
                                         instance can fetch them. Cached pages still work.";
pub const RATE_LIMITED_MESSAGE: &str =
   "This instance has reached its upstream rate limit. Please try again later.";
pub const INTERNAL_ERROR_MESSAGE: &str = "An internal service error occurred.";
pub const UNAVAILABLE_MESSAGE: &str = "The service has no available upstream sessions.";

#[must_use]
pub fn client_budget_message() -> &'static str {
   pick(&configured().client_budget, CLIENT_BUDGET_MESSAGE)
}

#[must_use]
pub fn rate_limited_message() -> &'static str {
   pick(&configured().rate_limited, RATE_LIMITED_MESSAGE)
}

#[must_use]
pub const fn unavailable_message() -> &'static str {
   UNAVAILABLE_MESSAGE
}

#[must_use]
pub fn internal_error_message() -> &'static str {
   pick(&configured().internal_error, INTERNAL_ERROR_MESSAGE)
}

#[derive(Error, Debug)]
pub enum Error {
   #[error("Configuration error: {0}")]
   Config(#[from] TomlError),

   #[error("Configuration error: {0}")]
   InvalidConfig(String),

   #[error("IO error: {0}")]
   Io(#[from] io::Error),

   #[error("HTTP request error: {0}")]
   Http(String),

   #[error("JSON parsing error: {0}")]
   Json(#[from] serde_json::Error),

   #[error("Twitter API error: {0}")]
   TwitterApi(String),

   #[error("Rate limited")]
   RateLimited,

   #[error("Session rejected: {0}")]
   SessionRejected(String),

   #[error("Client budget exhausted")]
   ClientBudgetExhausted,

   #[error("Not found: {0}")]
   NotFound(String),

   #[error("Upstream returned nothing")]
   TransientUpstream,

   #[error("User suspended: {0}")]
   UserSuspended(String),

   #[error("User not found: {0}")]
   UserNotFound(String),

   #[error("Tweet not found: {0}")]
   TweetNotFound(String),

   #[error("Protected user: {0}")]
   ProtectedUser(String),

   #[error("No sessions available")]
   NoSessions,

   #[error("Invalid URL: {0}")]
   InvalidUrl(String),

   #[error("HMAC verification failed")]
   HmacVerification,

   #[error("Internal error: {0}")]
   Internal(String),
}

impl Error {
   /// Status, heading and reader-facing text for this failure.
   #[must_use]
   pub fn presentation(&self) -> (StatusCode, &'static str, &str) {
      match *self {
         Self::NotFound(ref message) => (StatusCode::NOT_FOUND, "Not found", message),
         Self::UserNotFound(_) => {
            (
               StatusCode::NOT_FOUND,
               "Account not found",
               "No account exists with that name, or X has removed it.",
            )
         },
         Self::TweetNotFound(_) => {
            (
               StatusCode::NOT_FOUND,
               "Post not found",
               "That post has been deleted, or the account that made it is gone.",
            )
         },
         Self::UserSuspended(_) => {
            (
               StatusCode::FORBIDDEN,
               "Account suspended",
               "X has suspended this account, so its posts are not available here.",
            )
         },
         Self::ProtectedUser(_) => {
            (
               StatusCode::FORBIDDEN,
               "Protected account",
               "This account's posts are only visible to followers it has approved.",
            )
         },
         Self::InvalidUrl(ref message) => (StatusCode::BAD_REQUEST, "Bad request", message),
         Self::HmacVerification => {
            (
               StatusCode::FORBIDDEN,
               "Forbidden",
               "The media URL signature is invalid.",
            )
         },
         Self::ClientBudgetExhausted => {
            (
               StatusCode::TOO_MANY_REQUESTS,
               "Slow down",
               client_budget_message(),
            )
         },
         Self::RateLimited => {
            (
               StatusCode::TOO_MANY_REQUESTS,
               "Rate limited",
               rate_limited_message(),
            )
         },
         Self::TransientUpstream => {
            (
               StatusCode::BAD_GATEWAY,
               "No answer from X",
               "X returned nothing for this request. Try again in a moment.",
            )
         },
         // Out of sessions is an operator problem, not an upstream quota one.
         Self::NoSessions | Self::SessionRejected(_) => {
            (
               StatusCode::SERVICE_UNAVAILABLE,
               "Unavailable",
               unavailable_message(),
            )
         },
         _ => {
            (
               StatusCode::INTERNAL_SERVER_ERROR,
               "Error",
               internal_error_message(),
            )
         },
      }
   }
}

impl IntoResponse for Error {
   fn into_response(self) -> Response {
      let (status, _, msg) = self.presentation();

      if status.is_server_error() {
         tracing::error!(error = ?self, "request failed");
      }

      let html = format!(
         r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="stylesheet" type="text/css" href="{STYLE_CSS}">
<link rel="stylesheet" type="text/css" href="{FONTELLO_CSS}">
<title>Error</title>
</head>
<body>
<nav><div class="inner-nav"><a class="site-name" href="/">teapot</a></div></nav>
<div class="container"><div class="panel-container"><div class="error-panel"><span>{}</span></div></div></div>
</body>
</html>"#,
         html_escape(msg)
      );

      (status, Html(html)).into_response()
   }
}

// Twitter API error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwitterError {
   NoUserMatches       = 17,
   ProtectedUser       = 22,
   UserNotFound        = 50,
   UserSuspended       = 63,
   RateLimited         = 88,
   InvalidToken        = 89,
   TweetNotFound       = 144,
   TweetUnavailable    = 179,
   NoStatusFound       = 220,
   BadToken            = 239,
   Locked              = 326,
   TweetUnavailable421 = 421,
   TweetCensored       = 422,
}

impl TwitterError {
   pub const fn from_code(code: i64) -> Option<Self> {
      match code {
         17 => Some(Self::NoUserMatches),
         22 => Some(Self::ProtectedUser),
         50 => Some(Self::UserNotFound),
         63 => Some(Self::UserSuspended),
         88 => Some(Self::RateLimited),
         89 => Some(Self::InvalidToken),
         144 => Some(Self::TweetNotFound),
         179 => Some(Self::TweetUnavailable),
         220 => Some(Self::NoStatusFound),
         239 => Some(Self::BadToken),
         326 => Some(Self::Locked),
         421 => Some(Self::TweetUnavailable421),
         422 => Some(Self::TweetCensored),
         _ => None,
      }
   }
}
