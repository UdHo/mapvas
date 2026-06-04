use crate::config::{TileProvider, TileType};
use crate::map::coordinates::{Tile, TilePriority};
use log::{debug, error, trace, warn};
use regex::Regex;
use reqwest;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::error::Error as StdError;
use std::fmt::Display;
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
// Compile regex once at first use
static API_KEY_REGEX: LazyLock<Regex> =
  LazyLock::new(|| Regex::new("[Kk]ey=([A-Za-z0-9-_]*)").expect("API_KEY_REGEX pattern is valid"));
const DEFAULT_MAX_CONCURRENT_DOWNLOADS: usize = 6;
const OSM_MAX_CONCURRENT_DOWNLOADS: usize = 2;
const OSM_CACHE_NAMESPACE: &str = "osm-policy-v2";
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TileLoaderError {
  #[error("Tile not available.")]
  TileNotAvailable { tile: Tile },
  #[error("Download already in progress.")]
  TileDownloadInProgress { tile: Tile },
  #[error("Regex compilation failed: {0}")]
  Regex(#[from] regex::Error),
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),
}

/// The png data of a tile.
pub type TileData = Vec<u8>;

/// Determines if a tile should be downloaded or loaded from the cache.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum TileSource {
  All,
  Download,
  Cache,
}

impl Display for TileSource {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      TileSource::All => write!(f, "all"),
      TileSource::Download => write!(f, "download"),
      TileSource::Cache => write!(f, "cache"),
    }
  }
}

/// A prioritized tile request.
#[derive(Debug, PartialEq, Eq, Clone)]
struct PrioritizedTileRequest {
  tile: Tile,
  priority: TilePriority,
  timestamp: Instant,
}

impl PartialOrd for PrioritizedTileRequest {
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for PrioritizedTileRequest {
  fn cmp(&self, other: &Self) -> std::cmp::Ordering {
    // First by priority (lower number = higher priority)
    self
      .priority
      .cmp(&other.priority)
      // Then by timestamp (older requests first)
      .then_with(|| self.timestamp.cmp(&other.timestamp))
  }
}

/// The interface the cached and non-cached tile loader.
#[allow(async_fn_in_trait)]
pub trait TileLoader {
  /// Tries to fetch the tile data asyncroneously.
  async fn tile_data(&self, tile: &Tile, source: TileSource) -> Result<TileData, TileLoaderError>;

  /// Tries to fetch the tile data with priority.
  async fn tile_data_with_priority(
    &self,
    tile: &Tile,
    source: TileSource,
    _priority: TilePriority,
  ) -> Result<TileData, TileLoaderError> {
    // Default implementation ignores priority
    self.tile_data(tile, source).await
  }
}

#[derive(Debug, Clone)]
struct TileCache {
  base_path: Option<PathBuf>,
  tile_type: TileType,
}

impl TileCache {
  fn path(&self, tile: &Tile) -> Option<PathBuf> {
    let ext = match self.tile_type {
      TileType::Raster => "png",
      TileType::Vector => "pbf",
    };
    self
      .base_path
      .clone()
      .map(|b| b.join(format!("{}_{}_{}.{}", tile.zoom, tile.x, tile.y, ext)))
  }

  fn cache_tile(&self, tile: &Tile, data: &[u8]) {
    if self.base_path.is_none() {
      return;
    }
    if let Some(path) = self.path(tile) {
      if let Err(e) = File::create(&path).and_then(|mut f| f.write_all(data)) {
        debug!("Error when writing file to {}: {}", path.display(), e);
      }
    } else {
      debug!("No valid path for caching tile: {tile:?}");
    }
  }
}

impl TileLoader for TileCache {
  async fn tile_data(
    &self,
    tile: &Tile,
    tile_source: TileSource,
  ) -> Result<TileData, TileLoaderError> {
    if tile_source == TileSource::Download {
      return Err(TileLoaderError::TileNotAvailable { tile: *tile });
    }
    match self.path(tile) {
      Some(p) => {
        if p.exists() {
          Ok(tokio::fs::read(p).await?)
        } else {
          Err(TileLoaderError::TileNotAvailable { tile: *tile })
        }
      }

      None => Err(TileLoaderError::TileNotAvailable { tile: *tile }),
    }
  }
}

#[derive(Debug)]
struct DownloadManager<T: Hash + Eq + Clone> {
  tiles_in_download: Arc<Mutex<HashSet<T>>>,
  last_failed_attempt: Arc<Mutex<HashMap<T, Instant>>>,
  priority_queue: Arc<Mutex<BinaryHeap<Reverse<PrioritizedTileRequest>>>>,
  max_concurrent: usize,
}

impl<T> Default for DownloadManager<T>
where
  T: Hash + Eq + Clone,
{
  fn default() -> Self {
    Self::new(DEFAULT_MAX_CONCURRENT_DOWNLOADS)
  }
}

impl<T> DownloadManager<T>
where
  T: Hash + Eq + Clone,
{
  fn new(max_concurrent: usize) -> Self {
    Self {
      tiles_in_download: Arc::new(Mutex::new(HashSet::new())),
      last_failed_attempt: Arc::new(Mutex::new(HashMap::new())),
      priority_queue: Arc::new(Mutex::new(BinaryHeap::new())),
      max_concurrent,
    }
  }
}

impl DownloadManager<Tile> {
  fn download_with_priority(
    dm: &Arc<Self>,
    tile: Tile,
    priority: TilePriority,
  ) -> Option<DownloadToken<Tile>> {
    let mut tiles_lock = dm
      .tiles_in_download
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut queue_lock = dm
      .priority_queue
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner);

    // Check if already downloading
    if tiles_lock.contains(&tile) {
      return None;
    }

    // For current priority tiles, skip queue if under limit
    if priority == TilePriority::Current && tiles_lock.len() < dm.max_concurrent {
      tiles_lock.insert(tile);
      drop(tiles_lock);
      drop(queue_lock);

      if let Some(ts) = dm
        .last_failed_attempt
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&tile)
      {
        let now = Instant::now();
        if now.duration_since(*ts).as_secs() < 5 {
          dm.tiles_in_download
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&tile);
          return None;
        }
      }

      return Some(DownloadToken::new(tile, dm.clone()));
    }

    // Add to priority queue if at capacity or lower priority
    let request = PrioritizedTileRequest {
      tile,
      priority,
      timestamp: Instant::now(),
    };

    queue_lock.push(Reverse(request));
    drop(queue_lock);
    drop(tiles_lock);

    None
  }

  fn try_next_from_queue(dm: &Arc<Self>) -> Option<DownloadToken<Tile>> {
    loop {
      let mut tiles_lock = dm
        .tiles_in_download
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
      let mut queue_lock = dm
        .priority_queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

      if tiles_lock.len() >= dm.max_concurrent {
        return None;
      }

      if let Some(Reverse(request)) = queue_lock.pop() {
        if !tiles_lock.contains(&request.tile) {
          tiles_lock.insert(request.tile);
          drop(tiles_lock);
          drop(queue_lock);

          if let Some(ts) = dm
            .last_failed_attempt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&request.tile)
          {
            let now = Instant::now();
            if now.duration_since(*ts).as_secs() < 5 {
              dm.tiles_in_download
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&request.tile);
              continue;
            }
          }

          return Some(DownloadToken::new(request.tile, dm.clone()));
        }
        // Continue with next item in queue
      } else {
        return None; // Queue is empty
      }
    }
  }
}

#[allow(dead_code)]
impl<T: Hash + Eq + Clone> DownloadManager<T> {
  fn download(dm: &Arc<Self>, tile: T) -> Option<DownloadToken<T>> {
    let mut lock = dm
      .tiles_in_download
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner);

    if lock.contains(&tile) || lock.len() >= dm.max_concurrent {
      return None;
    }
    lock.insert(tile.clone());
    drop(lock);

    if let Some(ts) = dm
      .last_failed_attempt
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .get(&tile)
    {
      let now = Instant::now();
      if now.duration_since(*ts).as_secs() < 5 {
        return None;
      }
    }

    Some(DownloadToken::new(tile, dm.clone()))
  }
}

struct DownloadToken<T: Hash + Eq + Clone> {
  tile: T,
  download_manager: Arc<DownloadManager<T>>,
}

impl<T: Hash + Eq + Clone> DownloadToken<T> {
  fn new(tile: T, download_manager: Arc<DownloadManager<T>>) -> Self {
    Self {
      tile,
      download_manager,
    }
  }
}

impl<T: Hash + Eq + Clone> Drop for DownloadToken<T> {
  fn drop(&mut self) {
    let mut tiles_in_download = self
      .download_manager
      .tiles_in_download
      .lock()
      .expect("lock");
    tiles_in_download.remove(&self.tile);
    self
      .download_manager
      .last_failed_attempt
      .lock()
      .unwrap()
      .insert(self.tile.clone(), Instant::now());
  }
}

fn is_osm_tile_provider_url(url_template: &str) -> bool {
  url_template.starts_with("https://tile.openstreetmap.org/")
    || url_template.starts_with("http://tile.openstreetmap.org/")
}

fn max_concurrent_downloads_for_url(url_template: &str) -> usize {
  if is_osm_tile_provider_url(url_template) {
    OSM_MAX_CONCURRENT_DOWNLOADS
  } else {
    DEFAULT_MAX_CONCURRENT_DOWNLOADS
  }
}

fn allows_preloading_for_url(url_template: &str) -> bool {
  !is_osm_tile_provider_url(url_template)
}

fn cache_key_for_url(url_template: &str) -> String {
  let sanitized_url = API_KEY_REGEX.replace(url_template, "*");
  if is_osm_tile_provider_url(url_template) {
    format!("{sanitized_url}:{OSM_CACHE_NAMESPACE}")
  } else {
    sanitized_url.into_owned()
  }
}

fn format_reqwest_error(error: &reqwest::Error) -> String {
  let mut message = error.to_string();
  let mut source = StdError::source(error);
  while let Some(error) = source {
    message.push_str(": ");
    message.push_str(&error.to_string());
    source = error.source();
  }
  message
}

#[derive(Debug)]
struct TileDownloader {
  name: String,
  url_template: String,
  download_manager: Arc<DownloadManager<Tile>>,
  client: reqwest::Client,
}

impl TileDownloader {
  pub fn name(&self) -> &str {
    &self.name
  }

  fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
      .user_agent(crate::APP_USER_AGENT)
      .timeout(Duration::from_secs(5))
      .build()
      .expect("client")
  }

  pub fn from_url(url: &str, name: String) -> Self {
    Self {
      name,
      url_template: url.to_string(),
      download_manager: Arc::new(DownloadManager::new(max_concurrent_downloads_for_url(url))),
      client: Self::build_client(),
    }
  }

  pub fn from_env() -> Self {
    let url_template = std::env::var("MAPVAS_TILE_URL").unwrap_or(String::from(
      "https://tile.openstreetmap.org/{zoom}/{x}/{y}.png",
    ));
    Self {
      name: "OSM".to_string(),
      download_manager: Arc::new(DownloadManager::new(max_concurrent_downloads_for_url(
        &url_template,
      ))),
      url_template,
      client: Self::build_client(),
    }
  }

  fn requires_osm_attribution(&self) -> bool {
    is_osm_tile_provider_url(&self.url_template)
  }

  fn allows_preloading(&self) -> bool {
    allows_preloading_for_url(&self.url_template)
  }

  fn get_path_for_tile(&self, tile: &Tile) -> String {
    self
      .url_template
      .replace("{x}", &tile.x.to_string())
      .replace("{y}", &tile.y.to_string())
      .replace("{zoom}", &tile.zoom.to_string())
  }
}

impl TileLoader for TileDownloader {
  async fn tile_data(
    &self,
    tile: &Tile,
    tile_source: TileSource,
  ) -> Result<TileData, TileLoaderError> {
    self
      .tile_data_with_priority(tile, tile_source, TilePriority::Current)
      .await
  }

  async fn tile_data_with_priority(
    &self,
    tile: &Tile,
    tile_source: TileSource,
    priority: TilePriority,
  ) -> Result<TileData, TileLoaderError> {
    if tile_source == TileSource::Cache {
      return Err(TileLoaderError::TileNotAvailable { tile: *tile });
    }

    let download_token =
      DownloadManager::download_with_priority(&self.download_manager, *tile, priority)
        .or_else(|| DownloadManager::try_next_from_queue(&self.download_manager));

    if download_token.is_none() {
      return Err(TileLoaderError::TileDownloadInProgress { tile: *tile });
    }

    let url = self.get_path_for_tile(tile);
    let response = {
      let mut attempt = 1;
      loop {
        match self.client.get(&url).send().await {
          Ok(response) => break response,
          Err(error) if attempt == 1 => {
            warn!(
              "Retrying tile {tile:?} from {url} with provider {} after request error: {}",
              self.name,
              format_reqwest_error(&error)
            );
            attempt += 1;
            tokio::time::sleep(Duration::from_millis(100)).await;
          }
          Err(error) => {
            let formatted_error = format_reqwest_error(&error);
            error!(
              "Error when downloading tile {tile:?} from {url} with provider {} after {attempt} attempts: {formatted_error}",
              self.name
            );
            return Err(TileLoaderError::TileNotAvailable { tile: *tile });
          }
        }
      }
    };

    let result = if response.status() == reqwest::StatusCode::OK {
      let data = match response.bytes().await {
        Ok(data) => data,
        Err(error) => {
          let formatted_error = format_reqwest_error(&error);
          error!(
            "Error when reading tile response {tile:?} from {url} with provider {}: {formatted_error}",
            self.name
          );
          return Err(TileLoaderError::TileNotAvailable { tile: *tile });
        }
      };
      if data.is_empty() {
        error!(
          "Tile {tile:?} from {url} with provider {} has empty response",
          self.name
        );
        Err(TileLoaderError::TileNotAvailable { tile: *tile })
      } else {
        Ok(data.to_vec())
      }
    } else {
      self
        .download_manager
        .last_failed_attempt
        .lock()
        .unwrap()
        .insert(*tile, Instant::now());
      error!(
        "Error when downloading tile {tile:?} from {url} with provider {}: HTTP {}",
        self.name,
        response.status()
      );
      Err(TileLoaderError::TileNotAvailable { tile: *tile })
    };
    debug!("Downloaded {tile:?}.");

    result
  }
}

#[derive(Debug)]
pub struct CachedTileLoader {
  cache: TileCache,
  loader: TileDownloader,
  format: TileType,
  max_zoom: u8,
}

impl CachedTileLoader {
  #[must_use]
  pub fn name(&self) -> &str {
    self.loader.name()
  }

  #[must_use]
  pub fn tile_type(&self) -> TileType {
    self.format
  }

  #[must_use]
  pub fn max_zoom(&self) -> u8 {
    self.max_zoom
  }

  #[must_use]
  pub fn requires_osm_attribution(&self) -> bool {
    self.loader.requires_osm_attribution()
  }

  #[must_use]
  pub fn allows_preloading(&self) -> bool {
    self.loader.allows_preloading()
  }

  #[must_use]
  pub fn tiles_downloading(&self) -> usize {
    self
      .loader
      .download_manager
      .tiles_in_download
      .lock()
      .unwrap()
      .len()
  }

  #[must_use]
  pub fn tiles_queued(&self) -> usize {
    self
      .loader
      .download_manager
      .priority_queue
      .lock()
      .unwrap()
      .len()
  }

  pub fn from_config(config: &crate::config::Config) -> impl Iterator<Item = Self> {
    config
      .tile_provider
      .iter()
      .map(|url| CachedTileLoader::from_url(url, config.tile_cache_dir.clone()))
  }

  fn from_url(provider: &TileProvider, cache: Option<PathBuf>) -> Self {
    let tile_loader = TileDownloader::from_url(&provider.url, provider.name.clone());
    let cache_path = cache.map(|mut p| {
      let res = cache_key_for_url(&tile_loader.url_template);
      let mut hasher = DefaultHasher::new();
      res.hash(&mut hasher);
      p.push(hasher.finish().to_string());
      p
    });

    Self::create_cache(cache_path.as_ref());

    let tile_type = provider.tile_type;
    let tile_cache = TileCache {
      base_path: cache_path,
      tile_type,
    };
    CachedTileLoader {
      cache: tile_cache,
      loader: tile_loader,
      format: tile_type,
      max_zoom: provider.get_max_zoom(),
    }
  }

  /// Reads tile data from the local cache without triggering a network download.
  ///
  /// # Errors
  ///
  /// Returns [`TileLoaderError::TileNotAvailable`] if the tile is not present in
  /// the cache, or an I/O error if reading the cached file fails.
  pub async fn get_from_cache(
    &self,
    tile: &Tile,
    tile_source: TileSource,
  ) -> Result<TileData, TileLoaderError> {
    self.cache.tile_data(tile, tile_source).await
  }

  fn create_cache(cache_path: Option<&PathBuf>) {
    let Some(cache_path) = cache_path else { return };
    if cache_path.exists() {
      return;
    }
    let _ = fs::create_dir_all(cache_path).inspect_err(|e| {
      error!("Failed to create cache directory: {e}");
    });
  }

  async fn download_with_priority(
    &self,
    tile: &Tile,
    tile_source: TileSource,
    priority: TilePriority,
  ) -> Result<TileData, TileLoaderError> {
    match self
      .loader
      .tile_data_with_priority(tile, tile_source, priority)
      .await
    {
      Ok(data) => {
        self.cache.cache_tile(tile, &data);
        match data.len() {
          0..=100 => Err(TileLoaderError::TileNotAvailable { tile: *tile }),
          _ => Ok(data),
        }
      }
      Err(e) => Err(e),
    }
  }
}

impl Default for CachedTileLoader {
  fn default() -> CachedTileLoader {
    let base_path = match std::env::var("TILECACHE") {
      Ok(path) => Some(PathBuf::from(path)),
      Err(_) => None,
    };

    let tile_loader = TileDownloader::from_env();
    let cache_path = base_path.map(|mut p| {
      let res = cache_key_for_url(&tile_loader.url_template);
      let mut hasher = DefaultHasher::new();
      res.hash(&mut hasher);
      p.push(hasher.finish().to_string());
      p
    });

    Self::create_cache(cache_path.as_ref());
    let tile_cache = TileCache {
      base_path: cache_path,
      tile_type: TileType::Raster,
    };

    CachedTileLoader {
      cache: tile_cache,
      loader: tile_loader,
      format: TileType::Raster,
      max_zoom: 19, // Default for raster tiles
    }
  }
}

impl TileLoader for CachedTileLoader {
  async fn tile_data(
    &self,
    tile: &Tile,
    tile_source: TileSource,
  ) -> Result<TileData, TileLoaderError> {
    self
      .tile_data_with_priority(tile, tile_source, TilePriority::Current)
      .await
  }

  async fn tile_data_with_priority(
    &self,
    tile: &Tile,
    tile_source: TileSource,
    priority: TilePriority,
  ) -> Result<TileData, TileLoaderError> {
    trace!("Loading tile from file {:?}", &tile);
    if let Ok(data) = self.get_from_cache(tile, tile_source).await {
      debug!("cache_hit: {tile:?}");
      Ok(data)
    } else {
      debug!("cache_miss: {tile:?}");
      self
        .download_with_priority(tile, tile_source, priority)
        .await
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn app_user_agent_identifies_mapvas() {
    assert!(crate::APP_USER_AGENT.starts_with("MapVas/"));
    assert!(crate::APP_USER_AGENT.contains(env!("CARGO_PKG_HOMEPAGE")));
  }

  #[test]
  fn detects_osm_tile_provider_urls() {
    assert!(is_osm_tile_provider_url(
      "https://tile.openstreetmap.org/{zoom}/{x}/{y}.png"
    ));
    assert!(is_osm_tile_provider_url(
      "http://tile.openstreetmap.org/{zoom}/{x}/{y}.png"
    ));
    assert!(!is_osm_tile_provider_url(
      "https://tiles.example.com/{zoom}/{x}/{y}.png"
    ));
  }

  #[test]
  fn throttles_only_osm_tile_provider() {
    let osm = "https://tile.openstreetmap.org/{zoom}/{x}/{y}.png";
    let custom = "https://tiles.example.com/{zoom}/{x}/{y}.png";
    let local = "http://192.168.0.44:3000/20260109/{zoom}/{x}/{y}";

    assert_eq!(
      max_concurrent_downloads_for_url(osm),
      OSM_MAX_CONCURRENT_DOWNLOADS
    );
    assert_eq!(
      max_concurrent_downloads_for_url(custom),
      DEFAULT_MAX_CONCURRENT_DOWNLOADS
    );
    assert_eq!(
      max_concurrent_downloads_for_url(local),
      DEFAULT_MAX_CONCURRENT_DOWNLOADS
    );

    assert!(!allows_preloading_for_url(osm));
    assert!(allows_preloading_for_url(custom));
    assert!(allows_preloading_for_url(local));
  }

  #[test]
  fn namespaces_osm_cache_key() {
    let osm_key = cache_key_for_url("https://tile.openstreetmap.org/{zoom}/{x}/{y}.png");
    let custom_key = cache_key_for_url("https://tiles.example.com/{zoom}/{x}/{y}.png");

    assert!(osm_key.ends_with(OSM_CACHE_NAMESPACE));
    assert!(!custom_key.ends_with(OSM_CACHE_NAMESPACE));
  }
}
