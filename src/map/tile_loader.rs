use crate::config::{TileProvider, TileType};
use crate::map::coordinates::{Tile, TilePriority};
use anyhow::Result;
use log::{debug, error, trace};
use regex::Regex;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt::Display;
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use surf::http::Method;

// Compile regex once at first use
static API_KEY_REGEX: LazyLock<Regex> =
  LazyLock::new(|| Regex::new("[Kk]ey=([A-Za-z0-9-_]*)").expect("API_KEY_REGEX pattern is valid"));
use surf::{Config, Request, Url};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TileLoaderError {
  #[error("Tile not available.")]
  TileNotAvailable { tile: Tile },
  #[error("Download already in progress.")]
  TileDownloadInProgress { tile: Tile },
  #[error("URL parsing failed: {0}")]
  UrlParse(#[from] surf::http::url::ParseError),
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
pub trait TileLoader {
  /// Tries to fetch the tile data asyncroneously.
  async fn tile_data(&self, tile: &Tile, source: TileSource) -> Result<TileData>;

  /// Tries to fetch the tile data with priority.
  async fn tile_data_with_priority(
    &self,
    tile: &Tile,
    source: TileSource,
    _priority: TilePriority,
  ) -> Result<TileData> {
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
  async fn tile_data(&self, tile: &Tile, tile_source: TileSource) -> Result<TileData> {
    if tile_source == TileSource::Download {
      return Err(TileLoaderError::TileNotAvailable { tile: *tile }.into());
    }
    match self.path(tile) {
      Some(p) => {
        if p.exists() {
          Ok(tokio::fs::read(p).await?)
        } else {
          Err(TileLoaderError::TileNotAvailable { tile: *tile }.into())
        }
      }

      None => Err(TileLoaderError::TileNotAvailable { tile: *tile }.into()),
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
    Self {
      tiles_in_download: Arc::new(Mutex::new(HashSet::new())),
      last_failed_attempt: Arc::new(Mutex::new(HashMap::new())),
      priority_queue: Arc::new(Mutex::new(BinaryHeap::new())),
      max_concurrent: 6, // Reduced from 10 to prioritize current tiles
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

impl<T: Hash + Eq + Clone> DownloadManager<T> {}

#[derive(Debug)]
struct TileDownloader {
  name: String,
  url_template: String,
  download_manager: Arc<DownloadManager<Tile>>,
  client: surf::Client,
}

impl TileDownloader {
  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn from_url(url: &str, name: String) -> Self {
    let url_template = url.to_string();
    let client: surf::Client = Config::new()
      .set_timeout(Some(Duration::from_secs(5)))
      .try_into()
      .expect("client");
    Self {
      name,
      url_template,
      download_manager: Arc::default(),
      client,
    }
  }

  pub fn from_env() -> Self {
    let url_template = std::env::var("MAPVAS_TILE_URL").unwrap_or(String::from(
      "https://tile.openstreetmap.org/{zoom}/{x}/{y}.png",
    ));
    let client: surf::Client = Config::new()
      .set_timeout(Some(Duration::from_secs(5)))
      .try_into()
      .expect("client");
    Self {
      name: "OSM".to_string(),
      url_template,
      download_manager: Arc::default(),
      client,
    }
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
  async fn tile_data(&self, tile: &Tile, tile_source: TileSource) -> Result<TileData> {
    self
      .tile_data_with_priority(tile, tile_source, TilePriority::Current)
      .await
  }

  async fn tile_data_with_priority(
    &self,
    tile: &Tile,
    tile_source: TileSource,
    priority: TilePriority,
  ) -> Result<TileData> {
    if tile_source == TileSource::Cache {
      return Err(TileLoaderError::TileNotAvailable { tile: *tile }.into());
    }

    let download_token =
      DownloadManager::download_with_priority(&self.download_manager, *tile, priority)
        .or_else(|| DownloadManager::try_next_from_queue(&self.download_manager));

    if download_token.is_none() {
      return Err(TileLoaderError::TileDownloadInProgress { tile: *tile }.into());
    }

    let url = self.get_path_for_tile(tile);
    let request = Request::new(Method::Get, Url::parse(&url).unwrap());
    let result = self
      .client
      .send(request)
      .await
      .inspect_err(|e| error!("Error when downloading tile: {e}"))
      .map_err(|_| TileLoaderError::TileNotAvailable { tile: *tile });
    let result = if let Ok(mut result) = result {
      if result.status() == 200 {
        result
          .body_bytes()
          .await
          .map_err(|_| TileLoaderError::TileNotAvailable { tile: *tile })
          .and_then(|data| {
            if data.is_empty() {
              error!("Tile {tile:?} has empty response");
              Err(TileLoaderError::TileNotAvailable { tile: *tile })
            } else {
              Ok(data)
            }
          })
      } else {
        self
          .download_manager
          .last_failed_attempt
          .lock()
          .unwrap()
          .insert(*tile, Instant::now());
        error!(
          "Error when downloading tile: {}, {:?}",
          result.status(),
          result.body_string().await
        );
        Err(TileLoaderError::TileNotAvailable { tile: *tile })
      }
    } else {
      debug!("{result:?}");
      Err(TileLoaderError::TileNotAvailable { tile: *tile })
    };
    debug!("Downloaded {tile:?}.");

    Ok(result?)
  }
}

#[derive(Debug)]
pub struct CachedTileLoader {
  cache: TileCache,
  loader: TileDownloader,
  format: TileType,
}

impl CachedTileLoader {
  pub fn name(&self) -> &str {
    self.loader.name()
  }

  pub fn tile_type(&self) -> TileType {
    self.format
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
      let haystack = &tile_loader.url_template;
      let res = API_KEY_REGEX.replace(haystack, "*");
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
    }
  }

  pub async fn get_from_cache(&self, tile: &Tile, tile_source: TileSource) -> Result<TileData> {
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
  ) -> Result<TileData> {
    match self
      .loader
      .tile_data_with_priority(tile, tile_source, priority)
      .await
    {
      Ok(data) => {
        self.cache.cache_tile(tile, &data);
        match data.len() {
          0..=100 => Err(TileLoaderError::TileNotAvailable { tile: *tile }.into()),
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
      let res = API_KEY_REGEX.replace(&tile_loader.url_template, "*");
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
    }
  }
}

impl TileLoader for CachedTileLoader {
  async fn tile_data(&self, tile: &Tile, tile_source: TileSource) -> Result<TileData> {
    self
      .tile_data_with_priority(tile, tile_source, TilePriority::Current)
      .await
  }

  async fn tile_data_with_priority(
    &self,
    tile: &Tile,
    tile_source: TileSource,
    priority: TilePriority,
  ) -> Result<TileData> {
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
