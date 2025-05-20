use crate::config::TileProvider;
use crate::map::coordinates::Tile;
use anyhow::Result;
use log::{debug, error, trace};
use regex::Regex;
use std::collections::HashSet;
use std::fmt::Display;
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use surf::http::Method;
use surf::{Config, Request, Url};
use surf_governor::GovernorMiddleware;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TileLoaderError {
  #[error("Tile not availble.")]
  TileNotAvailableError { tile: Tile },
  #[error("Download already in progress.")]
  TileDownloadInProgressError { tile: Tile },
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

/// The interface the cached and non-cached tile loader.
pub trait TileLoader {
  /// Tries to fetch the tile data asyncroneously.
  async fn tile_data(&self, tile: &Tile, source: TileSource) -> Result<TileData>;
}

#[derive(Debug, Clone)]
struct TileCache {
  base_path: Option<PathBuf>,
}

impl TileCache {
  fn path(&self, tile: &Tile) -> Option<PathBuf> {
    self
      .base_path
      .clone()
      .map(|b| b.join(format!("{}_{}_{}.png", tile.zoom, tile.x, tile.y)))
  }

  fn cache_tile(&self, tile: &Tile, data: &[u8]) {
    if self.base_path.is_none() {
      return;
    }
    let succ = File::create(self.path(tile).unwrap()).map(|mut f| f.write_all(data));
    if succ.is_err() {
      debug!("Error when writing file: {}", succ.unwrap_err());
    }
  }
}

impl TileLoader for TileCache {
  async fn tile_data(&self, tile: &Tile, tile_source: TileSource) -> Result<TileData> {
    if tile_source == TileSource::Download {
      return Err(TileLoaderError::TileNotAvailableError { tile: *tile }.into());
    }
    match self.path(tile) {
      Some(p) => {
        if p.exists() {
          Ok(fs::read(p)?)
        } else {
          Err(TileLoaderError::TileNotAvailableError { tile: *tile }.into())
        }
      }

      None => Err(TileLoaderError::TileNotAvailableError { tile: *tile }.into()),
    }
  }
}

#[derive(Debug)]
struct TileDownloader {
  name: String,
  url_template: String,
  tiles_in_download: Arc<Mutex<HashSet<Tile>>>,
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
      tiles_in_download: Arc::default(),
      client: client.with(GovernorMiddleware::per_second(10).unwrap()),
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
      tiles_in_download: Arc::default(),
      client: client.with(GovernorMiddleware::per_second(10).unwrap()),
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
    if tile_source == TileSource::Cache {
      return Err(TileLoaderError::TileNotAvailableError { tile: *tile }.into());
    }

    {
      let mut tiles_in_download = self.tiles_in_download.lock().unwrap();
      debug!("Tiles in download: {tiles_in_download:?}");
      let is_in_progress = tiles_in_download.get(tile);
      if is_in_progress.is_some() {
        return Err(TileLoaderError::TileDownloadInProgressError { tile: *tile }.into());
      }
      tiles_in_download.insert(*tile);
    }

    let url = self.get_path_for_tile(tile);
    let request = Request::new(Method::Get, Url::parse(&url).unwrap());
    let result = self
      .client
      .send(request)
      .await
      .inspect_err(|e| error!("Error when downloading tile: {e}"))
      .map_err(|_| TileLoaderError::TileNotAvailableError { tile: *tile });
    let result = if let Ok(mut result) = result {
      if result.status() == 200 {
        result
          .body_bytes()
          .await
          .map_err(|_| TileLoaderError::TileNotAvailableError { tile: *tile })
      } else {
        error!(
          "Error when downloading tile: {}, {:?}",
          result.status(),
          result.body_string().await
        );
        Err(TileLoaderError::TileNotAvailableError { tile: *tile })
      }
    } else {
      debug!("{result:?}");
      Err(TileLoaderError::TileNotAvailableError { tile: *tile })
    };
    debug!("Downloaded {tile:?}.");

    let mut tiles_in_download = self.tiles_in_download.lock().unwrap();
    tiles_in_download.remove(tile);

    Ok(result?)
  }
}

#[derive(Debug)]
pub struct CachedTileLoader {
  tile_cache: TileCache,
  tile_loader: TileDownloader,
}

impl CachedTileLoader {
  pub fn name(&self) -> &str {
    self.tile_loader.name()
  }

  pub fn from_config(config: &crate::config::Config) -> impl Iterator<Item = Self> {
    config
      .tile_provider
      .iter()
      .map(|url| CachedTileLoader::from_url(url, config.tile_cache_dir.clone()))
  }

  fn from_url(url: &TileProvider, cache: Option<PathBuf>) -> Self {
    let tile_loader = TileDownloader::from_url(&url.url, url.name.clone());
    let cache_path = cache.map(|mut p| {
      let key_re = Regex::new("[Kk]ey=([A-Za-z0-9-_]*)").expect("re did not compile");
      let haystack = &tile_loader.url_template;
      let res = key_re.replace(haystack, "*");
      let mut hasher = DefaultHasher::new();
      res.hash(&mut hasher);
      p.push(hasher.finish().to_string());
      p
    });

    Self::create_cache(cache_path.as_ref());

    let tile_cache = TileCache {
      base_path: cache_path,
    };
    CachedTileLoader {
      tile_cache,
      tile_loader,
    }
  }

  pub async fn get_from_cache(&self, tile: &Tile, tile_source: TileSource) -> Result<TileData> {
    self.tile_cache.tile_data(tile, tile_source).await
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

  async fn download(&self, tile: &Tile, tile_source: TileSource) -> Result<TileData> {
    match self.tile_loader.tile_data(tile, tile_source).await {
      Ok(data) => {
        self.tile_cache.cache_tile(tile, &data);
        match data.len() {
          0..=100 => Err(TileLoaderError::TileNotAvailableError { tile: *tile }.into()),
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
      let key_re = Regex::new("[Kk]ey=([A-Za-z0-9-_]*)").expect("re did not compile");
      let res = key_re.replace(&tile_loader.url_template, "*");
      let mut hasher = DefaultHasher::new();
      res.hash(&mut hasher);
      p.push(hasher.finish().to_string());
      p
    });

    Self::create_cache(cache_path.as_ref());
    let tile_cache = TileCache {
      base_path: cache_path,
    };

    CachedTileLoader {
      tile_cache,
      tile_loader,
    }
  }
}

impl TileLoader for CachedTileLoader {
  async fn tile_data(&self, tile: &Tile, tile_source: TileSource) -> Result<TileData> {
    trace!("Loading tile from file {:?}", &tile);
    if let Ok(data) = self.get_from_cache(tile, tile_source).await {
      debug!("cache_hit: {tile:?}");
      Ok(data)
    } else {
      debug!("cache_miss: {tile:?}");
      self.download(tile, tile_source).await
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn downloader_test() {
    let downloader = CachedTileLoader::default();
    let data = downloader
      .tile_data(
        &Tile {
          x: 1,
          y: 1,
          zoom: 17,
        },
        TileSource::Download,
      )
      .await;
    assert!(data.is_ok());
    assert!(data.unwrap().len() > 100);
  }
}
