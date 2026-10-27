// topcoat views nest one type per element, and a full tweet page goes deeper
// than the default limit of 128.
#![recursion_limit = "256"]

mod api;
mod cache;
mod config;
mod error;
mod routes;
mod transcode;
mod translation;
mod types;
mod utils;
mod views;

use std::{
   net::{
      IpAddr,
      SocketAddr,
   },
   path::PathBuf,
   sync::Arc,
   time::Duration,
};

use axum::{
   Router,
   ServiceExt,
   extract::Request,
   http::{
      StatusCode,
      header::HeaderValue,
   },
   middleware,
};
use hyper::header;
use misstep::{
   Result,
   ResultExt as _,
};
use pound::Parse;
use tokio::net::TcpListener;
use topcoat::asset::AssetBundle;
use tower::{
   Layer as _,
   limit::ConcurrencyLimitLayer,
};
use tower_http::{
   normalize_path::NormalizePathLayer,
   services::{
      ServeDir,
      ServeFile,
   },
   set_header::SetResponseHeaderLayer,
   timeout::TimeoutLayer,
};
use tracing_subscriber::{
   filter::Targets,
   fmt,
   layer::SubscriberExt as _,
   util::SubscriberInitExt as _,
};

use crate::{
   api::{
      auth::SessionPool,
      client::ApiClient,
      http::HttpClient,
   },
   cache::store::Cache,
   config::{
      Config,
      GifTranscodingMode,
   },
   transcode::GifTranscoder,
};

/// A privacy-focused Twitter/X frontend.
#[derive(Parse)]
#[pound(name = "teapot")]
struct Cli {
   /// The config file to load.
   #[pound(short, long, env = "TEAPOT_CONF_FILE", default = "config/teapot.toml")]
   config:        PathBuf,
   /// The JSONL file of X sessions to rotate through.
   #[pound(short, long, env = "TEAPOT_SESSIONS_FILE", default = "sessions.jsonl")]
   sessions:      PathBuf,
   /// Where session rate limits persist across restarts, empty to keep them in
   /// memory.
   #[pound(
      long,
      env = "TEAPOT_SESSION_STATE_FILE",
      default = "session-limits.json"
   )]
   session_state: PathBuf,
   /// A topcoat asset bundle directory, in place of the one beside the binary.
   #[pound(long, env = "TEAPOT_ASSETS_DIR")]
   assets:        Option<PathBuf>,
   /// Tracing targets and levels to log.
   #[pound(long, env = "RUST_LOG", default = "teapot=debug,tower_http=debug")]
   log:           Targets,
}

/// Application state shared across all routes.
#[derive(Clone)]
pub struct AppState {
   pub config:              Arc<Config>,
   pub cache:               Cache,
   pub api:                 ApiClient,
   pub http_client:         HttpClient,
   pub gif_transcoder:      Option<Arc<GifTranscoder>>,
   pub translation_limiter: Arc<translation::TranslationLimiter>,
   /// Peers permitted to name a different caller through forwarding headers.
   pub trusted_proxies:     Arc<[IpAddr]>,
}

#[tokio::main]
async fn main() -> Result<()> {
   let cli = Cli::parse();

   tracing_subscriber::registry()
      .with(cli.log)
      .with(fmt::layer())
      .init();

   let config =
      Config::load(&cli.config).with_context(|| format!("loading {}", cli.config.display()))?;
   let config = Arc::new(config);

   tracing::info!(
      "Starting {} on {}:{}",
      config.server.title,
      config.server.address,
      config.server.port
   );

   error::set_messages(error::Messages {
      client_budget:  config.config.client_budget_message.clone(),
      rate_limited:   config.config.rate_limited_message.clone(),
      internal_error: config.config.internal_error_message.clone(),
   });

   let cache = Cache::new(config.cache.max_entries);

   let session_state = Some(cli.session_state).filter(|path| !path.as_os_str().is_empty());
   let sessions = SessionPool::load(
      &cli.sessions,
      session_state,
      config.config.max_concurrent_reqs,
   )
   .await
   .with_context(|| format!("loading {}", cli.sessions.display()))?;

   let api = ApiClient::new(&config, sessions);

   let http_client = HttpClient::new(&config.config.proxy, &config.config.proxy_auth);
   let gif_transcoder = if config.gif_transcoding.mode == GifTranscodingMode::Local {
      match GifTranscoder::new(http_client.clone(), config.gif_transcoding.clone()).await {
         Ok(transcoder) => {
            tracing::info!("GIF transcoder enabled (local mode)");
            Some(Arc::new(transcoder))
         },
         Err(err) => {
            tracing::error!("Failed to initialize GIF transcoder: {err}");
            None
         },
      }
   } else {
      if config.gif_transcoding.mode == GifTranscodingMode::External {
         tracing::info!(
            "GIF transcoding enabled (external mode, domain: {})",
            config.gif_transcoding.external_domain
         );
      }
      None
   };

   // Validated at load, so anything here parses.
   let trusted_proxies = config
      .config
      .trusted_proxies
      .iter()
      .filter_map(|proxy| proxy.parse::<IpAddr>().ok())
      .collect::<Vec<_>>();
   if trusted_proxies.is_empty() {
      tracing::info!("No trustedProxies configured, billing clients by socket address");
   }

   let state = AppState {
      config: Arc::clone(&config),
      cache,
      api,
      http_client,
      gif_transcoder,
      translation_limiter: Arc::new(translation::TranslationLimiter::new()),
      trusted_proxies: trusted_proxies.into(),
   };

   let assets = cli
      .assets
      .map_or_else(AssetBundle::load, AssetBundle::load_dir)
      .context("loading the topcoat asset bundle")?;

   // Build the router with specific routes before static files
   let static_dir = config.server.static_dir.clone();
   let app = Router::new()
      .merge(routes::app::router(&state, assets))
      .nest_service("/public", ServeDir::new(&static_dir))
      .nest_service("/css", ServeDir::new(format!("{static_dir}/css")))
      .nest_service("/js", ServeDir::new(format!("{static_dir}/js")))
      .nest_service("/fonts", ServeDir::new(format!("{static_dir}/fonts")))
      .nest_service("/md", ServeDir::new(format!("{static_dir}/md")))
      .route_service(
         "/logo.svg",
         ServeFile::new(format!("{static_dir}/logo.svg")),
      )
      .route_service(
         "/logo.png",
         ServeFile::new(format!("{static_dir}/logo.png")),
      )
      .route_service(
         "/favicon.ico",
         ServeFile::new(format!("{static_dir}/favicon.ico")),
      )
      .route_service(
         "/favicon-16x16.png",
         ServeFile::new(format!("{static_dir}/favicon-16x16.png")),
      )
      .route_service(
         "/favicon-32x32.png",
         ServeFile::new(format!("{static_dir}/favicon-32x32.png")),
      )
      .route_service(
         "/apple-touch-icon.png",
         ServeFile::new(format!("{static_dir}/apple-touch-icon.png")),
      )
      .route_service(
         "/site.webmanifest",
         ServeFile::new(format!("{static_dir}/site.webmanifest")),
      )
      .route_service(
         "/robots.txt",
         ServeFile::new(format!("{static_dir}/robots.txt")),
      )
      .route_service(
         "/opensearch.xml",
         ServeFile::new(format!("{static_dir}/opensearch.xml")),
      )
      .layer(middleware::from_fn(routes::middleware::prefs_middleware))
      .layer(middleware::from_fn(routes::middleware::snowflake_guard))
      .layer(middleware::from_fn_with_state(
         state.clone(),
         routes::middleware::client_middleware,
      ))
      .layer(SetResponseHeaderLayer::overriding(
         header::REFERRER_POLICY,
         HeaderValue::from_static("no-referrer"),
      ))
      .layer(SetResponseHeaderLayer::if_not_present(
         header::CONTENT_SECURITY_POLICY,
         HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src \
             'self' data:; media-src 'self' blob:; font-src 'self'; connect-src 'self'; \
             form-action 'self'; base-uri 'self'; frame-ancestors 'none'",
         ),
      ))
      .layer(SetResponseHeaderLayer::if_not_present(
         header::X_CONTENT_TYPE_OPTIONS,
         HeaderValue::from_static("nosniff"),
      ))
      .layer(SetResponseHeaderLayer::if_not_present(
         header::STRICT_TRANSPORT_SECURITY,
         HeaderValue::from_static("max-age=31536000; includeSubDomains"),
      ))
      .layer(TimeoutLayer::with_status_code(
         StatusCode::REQUEST_TIMEOUT,
         Duration::from_secs(45),
      ))
      .layer(ConcurrencyLimitLayer::new(
         usize::try_from(config.server.http_max_connections).unwrap_or(usize::MAX),
      ))
      .with_state(state);

   let addr = SocketAddr::new(config.server.address.parse()?, config.server.port);
   let listener = TcpListener::bind(addr).await?;

   tracing::info!("Listening on {addr}");
   axum::serve(
      listener,
      ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(
         NormalizePathLayer::trim_trailing_slash().layer(app),
      ),
   )
   .await?;

   Ok(())
}
