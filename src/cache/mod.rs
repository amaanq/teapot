//! Caching layer.

pub mod gif_cache;
/// Cache key builders.
pub mod keys;
mod store;
/// Cache TTL constants (in seconds).
pub mod ttl;
pub use gif_cache::GifCache;
pub use store::{
   Cache,
   Hit,
};
