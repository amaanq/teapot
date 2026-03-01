use std::{
   fs,
   path::Path,
};

use serde::Deserialize;

use crate::error::Result;

#[derive(Debug, Clone, Deserialize)]
#[expect(
   clippy::struct_field_names,
   reason = "config field in Config mirrors TOML structure"
)]
pub struct Config {
   pub server:      ServerConfig,
   pub cache:       CacheConfig,
   pub config:      AppConfig,
   #[serde(default)]
   pub preferences: PreferencesConfig,
   /// Precomputed `{scheme}://{hostname}` — set in `Config::load`.
   #[serde(skip)]
   pub url_prefix:  String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
   #[serde(default = "default_hostname")]
   pub hostname:             String,
   #[serde(default = "default_title")]
   pub title:                String,
   #[serde(default = "default_address")]
   pub address:              String,
   #[serde(default = "default_port")]
   pub port:                 u16,
   #[serde(default)]
   pub https:                bool,
   #[serde(
      default = "default_http_max_connections",
      rename = "httpMaxConnections"
   )]
   pub http_max_connections: u32,
   #[serde(default = "default_static_dir", rename = "staticDir")]
   pub static_dir:           String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
   #[serde(default = "default_list_minutes", rename = "listMinutes")]
   pub list_minutes: u64,
   #[serde(default = "default_rss_minutes", rename = "rssMinutes")]
   pub rss_minutes:  u64,
}

#[derive(Debug, Clone, Deserialize)]
#[expect(
   clippy::struct_excessive_bools,
   reason = "mirrors TOML config booleans"
)]
pub struct AppConfig {
   #[serde(default = "default_hmac_key", rename = "hmacKey")]
   pub hmac_key:            String,
   #[serde(default, rename = "base64Media")]
   pub base64_media:        bool,
   #[serde(default = "default_true", rename = "enableRSS")]
   pub enable_rss:          bool,
   #[serde(default, rename = "enableDebug")]
   pub enable_debug:        bool,
   #[serde(default)]
   pub proxy:               String,
   #[serde(default, rename = "proxyAuth")]
   pub proxy_auth:          String,
   #[serde(default, rename = "apiProxy")]
   pub api_proxy:           String,
   #[serde(default, rename = "disableTid")]
   pub disable_tid:         bool,
   #[serde(default = "default_max_concurrent_reqs", rename = "maxConcurrentReqs")]
   pub max_concurrent_reqs: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PreferencesConfig {
   #[serde(default = "default_theme")]
   pub theme:           String,
   #[serde(default, rename = "replaceTwitter")]
   pub replace_twitter: String,
   #[serde(default, rename = "replaceYouTube")]
   pub replace_youtube: String,
   #[serde(default, rename = "replaceReddit")]
   pub replace_reddit:  String,
   #[serde(default, rename = "infiniteScroll")]
   pub infinite_scroll: bool,
}

// Default value functions
fn default_hostname() -> String {
   "localhost".to_owned()
}
fn default_title() -> String {
   "teapot".to_owned()
}
fn default_address() -> String {
   "0.0.0.0".to_owned()
}
const fn default_port() -> u16 {
   8080
}
const fn default_http_max_connections() -> u32 {
   100
}
fn default_static_dir() -> String {
   "./public".to_owned()
}
const fn default_list_minutes() -> u64 {
   120
}
const fn default_rss_minutes() -> u64 {
   10
}
fn default_hmac_key() -> String {
   "secretkey".to_owned()
}
const fn default_max_concurrent_reqs() -> u32 {
   2
}
fn default_theme() -> String {
   "teapot".to_owned()
}
const fn default_true() -> bool {
   true
}

impl Config {
   pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
      let content = fs::read_to_string(path)?;
      let mut config = toml::from_str::<Self>(&content)?;
      if config.config.hmac_key == "secretkey" {
         tracing::warn!(
            "Using default HMAC key — video proxy URLs are forgeable. Set config.hmacKey in your \
             config"
         );
      }
      let scheme = if config.server.https {
         "https"
      } else {
         "http"
      };
      config.url_prefix = format!("{scheme}://{}", config.server.hostname);
      Ok(config)
   }

   /// Get the full URL prefix for generating links.
   pub fn url_prefix(&self) -> &str {
      &self.url_prefix
   }
}
