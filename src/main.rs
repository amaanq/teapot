mod api;
mod cache;
mod config;
mod error;
mod routes;
mod transcode;
mod types;
mod utils;
mod views;

use std::{
   env,
   net::SocketAddr,
   sync::Arc,
};

use axum::{
   Router,
   http::header::HeaderValue,
   middleware,
};
use hyper::header;
use tokio::net::TcpListener;
use tower_http::{
   services::{
      ServeDir,
      ServeFile,
   },
   set_header::SetResponseHeaderLayer,
};
use tracing_subscriber::{
   fmt,
   layer::SubscriberExt as _,
   util::SubscriberInitExt as _,
};

use crate::{
   api::{
      ApiClient,
      HttpClient,
      SessionPool,
   },
   cache::Cache,
   config::{
      Config,
      GifTranscodingMode,
   },
   transcode::GifTranscoder,
};

/// Application state shared across all routes.
#[derive(Clone)]
pub struct AppState {
   pub config:         Arc<Config>,
   pub cache:          Cache,
   pub api:            ApiClient,
   pub http_client:    HttpClient,
   pub gif_transcoder: Option<Arc<GifTranscoder>>,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
   // Initialize logging
   tracing_subscriber::registry()
      .with(
         tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "teapot=debug,tower_http=debug".into()),
      )
      .with(fmt::layer())
      .init();

   // Load configuration
   let config_path =
      env::var("TEAPOT_CONF_FILE").unwrap_or_else(|_| "config/teapot.toml".to_owned());
   let config = Config::load(&config_path)?;
   let config = Arc::new(config);

   tracing::info!(
      "Starting {} on {}:{}",
      config.server.title,
      config.server.address,
      config.server.port
   );

   // Initialize cache
   let cache = Cache::new();

   // Initialize session pool
   let sessions_path =
      env::var("TEAPOT_SESSIONS_FILE").unwrap_or_else(|_| "sessions.jsonl".to_owned());
   let sessions = SessionPool::load(&sessions_path).await?;

   // Initialize API client
   let api = ApiClient::new(&config, sessions);

   // Initialize GIF transcoder if local mode
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

   // Create application state
   let state = AppState {
      config: Arc::clone(&config),
      cache,
      api,
      http_client,
      gif_transcoder,
   };

   // Build router - order matters: specific routes first, then static files
   let static_dir = config.server.static_dir.clone();
   let app = Router::new()
        .merge(routes::router())
        // Serve static files at various paths
        .nest_service("/public", ServeDir::new(&static_dir))
        .nest_service("/css", ServeDir::new(format!("{static_dir}/css")))
        .nest_service("/js", ServeDir::new(format!("{static_dir}/js")))
        .nest_service("/fonts", ServeDir::new(format!("{static_dir}/fonts")))
        .nest_service("/md", ServeDir::new(format!("{static_dir}/md")))
        // Root-level static files
        .route_service("/logo.svg", ServeFile::new(format!("{static_dir}/logo.svg")))
        .route_service("/logo.png", ServeFile::new(format!("{static_dir}/logo.png")))
        .route_service("/favicon.ico", ServeFile::new(format!("{static_dir}/favicon.ico")))
        .route_service("/favicon-16x16.png", ServeFile::new(format!("{static_dir}/favicon-16x16.png")))
        .route_service("/favicon-32x32.png", ServeFile::new(format!("{static_dir}/favicon-32x32.png")))
        .route_service("/apple-touch-icon.png", ServeFile::new(format!("{static_dir}/apple-touch-icon.png")))
        .route_service("/site.webmanifest", ServeFile::new(format!("{static_dir}/site.webmanifest")))
        .route_service("/robots.txt", ServeFile::new(format!("{static_dir}/robots.txt")))
        .route_service("/opensearch.xml", ServeFile::new(format!("{static_dir}/opensearch.xml")))
        .layer(middleware::from_fn(routes::prefs_middleware))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'none'; \
                 script-src 'self'; \
                 style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data:; \
                 media-src 'self' blob:; \
                 font-src 'self'; \
                 connect-src 'self'; \
                 form-action 'self'; \
                 base-uri 'self'; \
                 frame-ancestors 'none'"
            ),
        ))
        .with_state(state);

   // Start server
   let addr = SocketAddr::new(config.server.address.parse()?, config.server.port);
   let listener = TcpListener::bind(addr).await?;

   tracing::info!("Listening on {addr}");
   axum::serve(listener, app).await?;

   Ok(())
}
