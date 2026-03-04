mod api;
mod cache;
mod config;
mod error;
mod routes;
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
   middleware,
};
use tokio::net::TcpListener;
use tower_http::services::{
   ServeDir,
   ServeFile,
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
   config::Config,
};

/// Application state shared across all routes.
#[derive(Clone)]
pub struct AppState {
   pub config:      Arc<Config>,
   pub cache:       Cache,
   pub api:         ApiClient,
   pub http_client: HttpClient,
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
   let config_path = env::var("TEAPOT_CONF_FILE").unwrap_or_else(|_| "config/teapot.toml".to_owned());
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

   // Create application state
   let state = AppState {
      config: Arc::clone(&config),
      cache,
      api,
      http_client: HttpClient::new(&config.config.proxy, &config.config.proxy_auth),
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
        .with_state(state);

   // Start server
   let addr = SocketAddr::new(config.server.address.parse()?, config.server.port);
   let listener = TcpListener::bind(addr).await?;

   tracing::info!("Listening on {addr}");
   axum::serve(listener, app).await?;

   Ok(())
}
