use std::{
   fs,
   path::Path,
   process::Command,
};

use serde::Deserialize;

use crate::error::Result;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GifTranscodingMode {
   #[default]
   Off,
   Local,
   External,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GifTranscodingConfig {
   #[serde(default)]
   pub mode:            GifTranscodingMode,
   #[serde(default = "default_gif_cache_dir", rename = "cacheDir")]
   pub cache_dir:       String,
   #[serde(default = "default_gif_cache_max_mb", rename = "cacheMaxMb")]
   pub cache_max_mb:    u64,
   #[serde(default, rename = "externalDomain")]
   pub external_domain: String,
}

impl Default for GifTranscodingConfig {
   fn default() -> Self {
      Self {
         mode:            GifTranscodingMode::Off,
         cache_dir:       default_gif_cache_dir(),
         cache_max_mb:    default_gif_cache_max_mb(),
         external_domain: String::new(),
      }
   }
}

fn default_gif_cache_dir() -> String {
   "./cache/gif".to_owned()
}
const fn default_gif_cache_max_mb() -> u64 {
   512
}

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
   pub preferences:      PreferencesConfig,
   #[serde(default, rename = "gifTranscoding")]
   pub gif_transcoding:  GifTranscodingConfig,
   /// Precomputed `{scheme}://{hostname}` — set in `Config::load`.
   #[serde(skip)]
   pub url_prefix:       String,
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
   #[serde(default, rename = "publicPort")]
   pub public_port:          Option<u16>,
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
   #[serde(default = "default_paid_emoji", rename = "paidEmoji")]
   pub paid_emoji:          String,
   #[serde(default = "default_ai_emoji", rename = "aiEmoji")]
   pub ai_emoji:            String,
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
fn default_paid_emoji() -> String {
   "🤝".to_owned()
}
fn default_ai_emoji() -> String {
   "🤖".to_owned()
}
const fn default_true() -> bool {
   true
}

impl Config {
   #[expect(clippy::cognitive_complexity, reason = "validation is straightforward")]
   pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
      let content = fs::read_to_string(path)?;
      let mut config = toml::from_str::<Self>(&content)?;
      if config.config.hmac_key == "secretkey" {
         tracing::warn!(
            "Using default HMAC key — video proxy URLs are forgeable. Set config.hmacKey in your \
             config"
         );
      }
      // Validate GIF transcoding config
      match config.gif_transcoding.mode {
         GifTranscodingMode::Local => {
            if Command::new("ffmpeg")
               .arg("-version")
               .output()
               .is_err()
            {
               tracing::error!(
                  "gifTranscoding.mode is 'local' but ffmpeg was not found on PATH — falling back \
                   to 'off'"
               );
               config.gif_transcoding.mode = GifTranscodingMode::Off;
            }
         },
         GifTranscodingMode::External => {
            if config.gif_transcoding.external_domain.is_empty() {
               tracing::error!(
                  "gifTranscoding.mode is 'external' but externalDomain is empty — falling back \
                   to 'off'"
               );
               config.gif_transcoding.mode = GifTranscodingMode::Off;
            }
         },
         GifTranscodingMode::Off => {},
      }

      let scheme = if config.server.https { "https" } else { "http" };
      let default_port = if config.server.https { 443 } else { 80 };
      let url_port = config.server.public_port.unwrap_or(config.server.port);
      if url_port == default_port || config.server.hostname.contains(':') {
         config.url_prefix = format!("{scheme}://{}", config.server.hostname);
      } else {
         config.url_prefix = format!(
            "{scheme}://{}:{}",
            config.server.hostname, url_port
         );
      }
      Ok(config)
   }

   /// Get the full URL prefix for generating links.
   pub fn url_prefix(&self) -> &str {
      &self.url_prefix
   }
}
