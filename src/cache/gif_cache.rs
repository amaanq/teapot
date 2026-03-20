use std::{
   path::{
      Path,
      PathBuf,
   },
   sync::Arc,
   time::SystemTime,
};

use indexmap::IndexMap;
use tokio::{
   fs,
   sync::RwLock,
};

struct GifCacheEntry {
   file_size:   u64,
   last_access: SystemTime,
}

pub struct GifCache {
   dir:       PathBuf,
   max_bytes: u64,
   entries:   Arc<RwLock<IndexMap<String, GifCacheEntry>>>,
}

impl GifCache {
   /// Create a new GIF cache, scanning existing files to rebuild state.
   pub async fn new(dir: &str, max_mb: u64) -> eyre::Result<Self> {
      let dir = PathBuf::from(dir);
      fs::create_dir_all(&dir).await?;

      let mut entries = IndexMap::new();
      let mut read_dir = fs::read_dir(&dir).await?;
      while let Some(entry) = read_dir.next_entry().await? {
         let path = entry.path();
         if path.extension().is_some_and(|ext| ext == "gif")
            && let Some(stem) = path.file_stem().and_then(|st| st.to_str())
            && let Ok(meta) = entry.metadata().await
         {
            entries.insert(stem.to_owned(), GifCacheEntry {
               file_size:   meta.len(),
               last_access: meta
                  .accessed()
                  .or_else(|_| meta.modified())
                  .unwrap_or_else(|_| SystemTime::now()),
            });
         }
      }

      // Sort by last access time (oldest first) for LRU ordering
      entries.sort_by(|_, lhs, _, rhs| lhs.last_access.cmp(&rhs.last_access));

      tracing::info!(
         "GIF cache loaded: {} entries from {}",
         entries.len(),
         dir.display()
      );

      Ok(Self {
         dir,
         max_bytes: max_mb * 1024 * 1024,
         entries: Arc::new(RwLock::new(entries)),
      })
   }

   /// Get the path for a cached GIF, returning `None` if not cached.
   #[expect(clippy::significant_drop_tightening, reason = "entry borrows from map")]
   pub async fn get(&self, hash: &str) -> Option<PathBuf> {
      let path = self.dir.join(format!("{hash}.gif"));
      if !fs::try_exists(&path).await.unwrap_or(false) {
         // File was deleted externally — remove from tracking
         self.entries.write().await.shift_remove(hash);
         return None;
      }

      let mut entries = self.entries.write().await;
      if let Some(entry) = entries.get_mut(hash) {
         entry.last_access = SystemTime::now();
         // Move to end (most recently used)
         let idx = entries.get_index_of(hash).unwrap_or_default();
         let last = entries.len().saturating_sub(1);
         entries.move_index(idx, last);
         Some(path)
      } else {
         None
      }
   }

   /// Insert a transcoded GIF into the cache, evicting LRU entries if needed.
   pub async fn put(&self, hash: &str, data: &[u8]) -> eyre::Result<PathBuf> {
      let path = self.dir.join(format!("{hash}.gif"));
      fs::write(&path, data).await?;

      let file_size = data.len() as u64;

      let mut entries = self.entries.write().await;
      entries.insert(hash.to_owned(), GifCacheEntry {
         file_size,
         last_access: SystemTime::now(),
      });
      Self::evict(&self.dir, &mut entries, self.max_bytes).await;
      drop(entries);

      Ok(path)
   }

   /// Remove oldest entries until total size is under the limit.
   async fn evict(dir: &Path, entries: &mut IndexMap<String, GifCacheEntry>, max_bytes: u64) {
      let total: u64 = entries.values().map(|entry| entry.file_size).sum();
      if total <= max_bytes {
         return;
      }

      let mut current = total;
      while current > max_bytes && !entries.is_empty() {
         // Remove from the front (oldest)
         if let Some((hash, entry)) = entries.shift_remove_index(0) {
            current -= entry.file_size;
            let path = dir.join(format!("{hash}.gif"));
            if let Err(err) = fs::remove_file(&path).await {
               tracing::warn!("Failed to evict cached GIF {}: {err}", path.display());
            }
         }
      }
   }
}
