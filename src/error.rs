use std::{
   io,
   result,
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

   #[error("Not found: {0}")]
   NotFound(String),

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

impl IntoResponse for Error {
   fn into_response(self) -> Response {
      let status = match &self {
         &Self::NotFound(_) | &Self::UserNotFound(_) | &Self::TweetNotFound(_) => {
            StatusCode::NOT_FOUND
         },
         &Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
         &Self::UserSuspended(_) | &Self::ProtectedUser(_) | &Self::HmacVerification => {
            StatusCode::FORBIDDEN
         },
         _ => StatusCode::INTERNAL_SERVER_ERROR,
      };

      // Render styled error page
      let msg = self.to_string();
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
         html_escape(&msg)
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
