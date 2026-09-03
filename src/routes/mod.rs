//! HTTP routes.

mod app;
mod debug;
mod embed;
pub mod helpers;
mod intent;
mod list;
mod media;
mod middleware;
mod notes;
mod pages;
mod preferences;
mod redirect;
mod rss;
mod search;
mod status;
mod timeline;
mod unsupported;

pub use app::router;
pub use middleware::{
   client_middleware,
   prefs_middleware,
   snowflake_guard,
};
