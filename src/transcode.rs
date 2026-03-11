use std::{
   collections::HashMap,
   path::PathBuf,
   process::Stdio,
   sync::Arc,
};

use data_encoding::HEXLOWER;
use ring::digest;
use tokio::{
   fs,
   process::Command,
   sync::{Mutex, Notify, Semaphore},
};

use crate::{
   api::HttpClient,
   cache::GifCache,
   config::GifTranscodingConfig,
};

pub struct GifTranscoder {
   cache:       GifCache,
   http_client: HttpClient,
   cache_dir:   PathBuf,
   semaphore:   Semaphore,
   inflight:    Mutex<HashMap<String, Arc<Notify>>>,
}

impl GifTranscoder {
   pub async fn new(
      http_client: HttpClient,
      config: GifTranscodingConfig,
   ) -> eyre::Result<Self> {
      let cache = GifCache::new(&config.cache_dir, config.cache_max_mb).await?;
      Ok(Self {
         cache,
         http_client,
         cache_dir: PathBuf::from(config.cache_dir),
         semaphore: Semaphore::new(3),
         inflight: Mutex::new(HashMap::new()),
      })
   }

   /// Hash an MP4 URL to a cache key (truncated SHA-256, 32 hex chars).
   fn hash_url(url: &str) -> String {
      let hash = digest::digest(&digest::SHA256, url.as_bytes());
      HEXLOWER.encode(hash.as_ref())[..32].to_owned()
   }

   /// Get a cached GIF or transcode the MP4. Returns the path to the GIF file.
   pub async fn get_or_transcode(&self, mp4_url: &str) -> eyre::Result<PathBuf> {
      let hash = Self::hash_url(mp4_url);

      // Check cache
      if let Some(path) = self.cache.get(&hash).await {
         return Ok(path);
      }

      // Atomically check-and-register: either wait on an existing transcoder or
      // register ourselves as the inflight one.
      let existing = {
         let map = self.inflight.lock().await;
         map.get(&hash).map(Arc::clone)
      };
      if let Some(existing) = existing {
         existing.notified().await;
         if let Some(path) = self.cache.get(&hash).await {
            return Ok(path);
         }
      }

      let notify = Arc::new(Notify::new());
      self.inflight
         .lock()
         .await
         .insert(hash.clone(), Arc::clone(&notify));

      // Acquire semaphore
      let _permit = self.semaphore.acquire().await?;

      // Double-check cache after acquiring permit
      if let Some(path) = self.cache.get(&hash).await {
         self.inflight.lock().await.remove(&hash);
         notify.notify_waiters();
         return Ok(path);
      }

      let result = self.do_transcode(mp4_url, &hash).await;

      // Notify waiters and remove inflight entry
      self.inflight.lock().await.remove(&hash);
      notify.notify_waiters();

      result
   }

   async fn do_transcode(&self, mp4_url: &str, hash: &str) -> eyre::Result<PathBuf> {
      let cache_dir = &self.cache_dir;
      let input = cache_dir.join(format!("{hash}.mp4.tmp"));
      let palette = cache_dir.join(format!("{hash}.palette.png"));
      let output = cache_dir.join(format!("{hash}.gif.tmp"));

      // Fetch MP4
      let response = self
         .http_client
         .get(mp4_url)
         .await
         .map_err(|err| eyre::eyre!("Failed to fetch MP4: {err}"))?;

      if !response.status().is_success() {
         return Err(eyre::eyre!("MP4 fetch returned {}", response.status()));
      }

      let bytes = response
         .bytes()
         .await
         .map_err(|err| eyre::eyre!("Failed to read MP4 body: {err}"))?;
      fs::write(&input, &bytes).await?;

      // Pass 1: generate palette
      let palette_out = Command::new("ffmpeg")
         .args([
            "-i",
            &input.to_string_lossy(),
            "-vf",
            "fps=15,scale=480:-1:flags=lanczos,palettegen=stats_mode=diff",
            "-y",
            &palette.to_string_lossy(),
         ])
         .stdout(Stdio::null())
         .stderr(Stdio::piped())
         .output()
         .await?;

      if !palette_out.status.success() {
         let stderr = String::from_utf8_lossy(&palette_out.stderr);
         Self::cleanup(&[&input, &palette, &output]).await;
         return Err(eyre::eyre!("ffmpeg palettegen failed: {stderr}"));
      }

      // Pass 2: generate GIF with palette
      let gif_out = Command::new("ffmpeg")
         .args([
            "-i",
            &input.to_string_lossy(),
            "-i",
            &palette.to_string_lossy(),
            "-lavfi",
            "fps=15,scale=480:-1:flags=lanczos [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=5",
            "-f",
            "gif",
            "-y",
            &output.to_string_lossy(),
         ])
         .stdout(Stdio::null())
         .stderr(Stdio::piped())
         .output()
         .await?;

      if !gif_out.status.success() {
         let stderr = String::from_utf8_lossy(&gif_out.stderr);
         Self::cleanup(&[&input, &palette, &output]).await;
         return Err(eyre::eyre!("ffmpeg paletteuse failed: {stderr}"));
      }

      // Read the output GIF and insert into cache
      let gif_data = fs::read(&output).await?;
      let result = self.cache.put(hash, &gif_data).await;

      // Clean up temp files
      Self::cleanup(&[&input, &palette, &output]).await;

      result
   }

   async fn cleanup(paths: &[&PathBuf]) {
      for path in paths {
         let _ = fs::remove_file(path).await;
      }
   }
}
