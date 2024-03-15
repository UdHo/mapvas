use crate::map::coordinates::Tile;
use anyhow::Result;
use async_std::task::block_on;
use log::{debug, error, trace};
use std::collections::HashSet;
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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

/// The interface the cached and non-cached tile loader.
pub trait TileLoader {
  /// Tries to fetch the tile data asyncroneously.
  async fn tile_data(&self, tile: &Tile) -> Result<TileData>;
  /// A blocking version of tile_data.
  #[allow(unused)]
  fn tile_data_blocking(&self, tile: &Tile) -> Result<TileData> {
    block_on(self.tile_data(tile))
  }
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
  async fn tile_data(&self, tile: &Tile) -> Result<TileData> {
    match self.path(tile) {
      Some(p) => match p.exists() {
        true => Ok(std::fs::read(p)?),
        false => Err(TileLoaderError::TileNotAvailableError { tile: *tile }.into()),
      },
      None => Err(TileLoaderError::TileNotAvailableError { tile: *tile }.into()),
    }
  }
}

#[derive(Debug, Clone)]
struct TileDownloader {
  url_template: String,
  tiles_in_download: Arc<Mutex<HashSet<Tile>>>,
}

impl TileDownloader {
  pub fn from_env() -> Self {
    let url_template = std::env::var("MAPVAS_TILE_URL").unwrap_or(String::from(
      "https://tile.openstreetmap.org/{zoom}/{x}/{y}.png",
    ));
    Self {
      url_template,
      tiles_in_download: Default::default(),
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
  async fn tile_data(&self, tile: &Tile) -> Result<TileData> {
    {
      let mut data = self.tiles_in_download.lock().unwrap();
      let is_in_progress = data.get(tile);
      if is_in_progress.is_some() {
        return Err(TileLoaderError::TileDownloadInProgressError { tile: *tile }.into());
      }
      data.insert(*tile);
    }

    let url = self.get_path_for_tile(tile);
    debug!("Downloading {}.", url);
    let result = surf::get(&url)
      .recv_bytes()
      .await
      .map_err(|_| TileLoaderError::TileNotAvailableError { tile: *tile });

    {
      let mut data = self.tiles_in_download.lock().unwrap();
      data.remove(tile);
    }
    Ok(result?)
  }
}

#[derive(Debug, Clone)]
pub struct CachedTileLoader {
  tile_cache: TileCache,
  tile_loader: TileDownloader,
}

impl CachedTileLoader {
  async fn get_from_cache(&self, tile: &Tile) -> Result<TileData> {
    self.tile_cache.tile_data(tile).await
  }

  async fn download(&self, tile: &Tile) -> Result<TileData> {
    match self.tile_loader.tile_data(tile).await {
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
      let mut hasher = DefaultHasher::new();
      tile_loader.url_template.hash(&mut hasher);
      p.push(hasher.finish().to_string());
      p
    });

    if let Some(ref cache_path) = cache_path {
      if !cache_path.exists() {
        let _ = std::fs::create_dir_all(cache_path);
      }
    }

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
  async fn tile_data(&self, tile: &Tile) -> Result<TileData> {
    trace!("Loading tile from file {:?}", &tile);
    match self.get_from_cache(tile).await {
      Ok(data) => Ok(data),
      Err(_) => self.download(tile).await,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn downloader_test() {
    let downloader = CachedTileLoader::default();
    let data = downloader.tile_data_blocking(&Tile {
      x: 1,
      y: 1,
      zoom: 17,
    });
    assert!(data.is_ok());
    assert!(data.unwrap().len() > 100);
  }
}
