use std::{
   collections::HashMap,
   fs as std_fs,
   path::PathBuf,
   process::Stdio,
   sync::Arc,
   time::Duration,
};

use data_encoding::HEXLOWER;
use ring::digest;
use tokio::{
   fs,
   process::Command,
   sync::{
      Mutex,
      Semaphore,
   },
   time::timeout,
};

use crate::{
   api::HttpClient,
   cache::GifCache,
   config::GifTranscodingConfig,
};

const MAX_TRANSCODE_INPUT_BYTES: usize = 100 * 1024 * 1024;
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(90);

struct TempFiles(Vec<PathBuf>);

impl Drop for TempFiles {
   fn drop(&mut self) {
      for path in &self.0 {
         let _ = std_fs::remove_file(path);
      }
   }
}

pub struct GifTranscoder {
   cache:       GifCache,
   http_client: HttpClient,
   cache_dir:   PathBuf,
   semaphore:   Semaphore,
   inflight:    Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl GifTranscoder {
   pub async fn new(http_client: HttpClient, config: GifTranscodingConfig) -> eyre::Result<Self> {
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

      // Atomically register or join the per-URL operation. A mutex is
      // cancellation safe. If the active task is dropped, the next waiter
      // acquires it and resumes the work instead of missing a notification.
      let operation = {
         let mut map = self.inflight.lock().await;
         Arc::clone(
            map.entry(hash.clone())
               .or_insert_with(|| Arc::new(Mutex::new(()))),
         )
      };
      let operation_guard = operation.lock().await;

      // Acquire semaphore
      let _permit = self.semaphore.acquire().await?;

      // Double-check cache after acquiring permit
      if let Some(path) = self.cache.get(&hash).await {
         drop(operation_guard);
         self.remove_inflight_if_last(&hash, &operation).await;
         return Ok(path);
      }

      let result = self.do_transcode(mp4_url, &hash).await;
      drop(operation_guard);
      self.remove_inflight_if_last(&hash, &operation).await;

      result
   }

   async fn remove_inflight_if_last(&self, hash: &str, operation: &Arc<Mutex<()>>) {
      let mut map = self.inflight.lock().await;
      if Arc::strong_count(operation) == 2
         && map
            .get(hash)
            .is_some_and(|current| Arc::ptr_eq(current, operation))
      {
         map.remove(hash);
      }
   }

   async fn do_transcode(&self, mp4_url: &str, hash: &str) -> eyre::Result<PathBuf> {
      let cache_dir = &self.cache_dir;
      let input = cache_dir.join(format!("{hash}.mp4.tmp"));
      let palette = cache_dir.join(format!("{hash}.palette.png"));
      let output = cache_dir.join(format!("{hash}.gif.tmp"));
      let _temp_files = TempFiles(vec![input.clone(), palette.clone(), output.clone()]);

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
         .bytes_limited(MAX_TRANSCODE_INPUT_BYTES)
         .await
         .map_err(|err| eyre::eyre!("Failed to read MP4 body: {err}"))?;
      fs::write(&input, &bytes).await?;

      // Pass 1: generate palette
      let mut palette_command = Command::new("ffmpeg");
      palette_command
         .kill_on_drop(true)
         .args([
            "-loglevel",
            "error",
            "-i",
            &input.to_string_lossy(),
            "-vf",
            "fps=15,scale=480:-1:flags=lanczos,palettegen=stats_mode=diff",
            "-y",
            &palette.to_string_lossy(),
         ])
         .stdout(Stdio::null())
         .stderr(Stdio::piped());
      let palette_out = timeout(FFMPEG_TIMEOUT, palette_command.output())
         .await
         .map_err(|_| eyre::eyre!("ffmpeg palettegen timed out"))??;

      if !palette_out.status.success() {
         let stderr = String::from_utf8_lossy(&palette_out.stderr);
         return Err(eyre::eyre!("ffmpeg palettegen failed: {stderr}"));
      }

      // Pass 2: generate GIF with palette
      let mut gif_command = Command::new("ffmpeg");
      gif_command
         .kill_on_drop(true)
         .args([
            "-loglevel",
            "error",
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
         .stderr(Stdio::piped());
      let gif_out = timeout(FFMPEG_TIMEOUT, gif_command.output())
         .await
         .map_err(|_| eyre::eyre!("ffmpeg paletteuse timed out"))??;

      if !gif_out.status.success() {
         let stderr = String::from_utf8_lossy(&gif_out.stderr);
         return Err(eyre::eyre!("ffmpeg paletteuse failed: {stderr}"));
      }

      // Read the output GIF and insert into cache
      let gif_data = fs::read(&output).await?;
      self.cache.put(hash, &gif_data).await
   }
}
