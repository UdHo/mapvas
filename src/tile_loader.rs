use async_std::task::block_on;
use log::{error, info, trace};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use crate::coordinates::Tile;

/// The png data of a tile.
pub type TileData = Vec<u8>;
/// TileData wrapped in a result.
pub type TileDataResult = std::result::Result<TileData, TileNotAvailableError>;

/// A single error type returned if a [`TileLoader`] fails to retrieve the tile.
#[derive(Debug, Clone)]
pub struct TileNotAvailableError;
impl fmt::Display for TileNotAvailableError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Tile not available")
    }
}

/// The interface the cached and non-cached tile loader.
pub trait TileGetter {
    /// Tries to fetch the tile data asyncroneously.
    async fn tile_data(&self, tile: &Tile) -> TileDataResult;
    /// A blocking version of tile_data.
    fn tile_data_blocking(&self, tile: &Tile) -> TileDataResult {
        block_on(self.tile_data(tile))
    }
}

#[derive(Debug, Clone)]
struct TileCache {
    base_path: PathBuf,
}

impl TileCache {
    fn path(&self, tile: &Tile) -> PathBuf {
        self.base_path
            .join(format!("{}_{}_{}.png", tile.zoom, tile.x, tile.y))
    }

    fn cache_tile(&self, tile: &Tile, data: &Vec<u8>) {
        let succ = File::create(self.path(&tile)).map(|mut f| f.write_all(&data));
        if succ.is_err() {
            error!("Error when writing file: {}", succ.unwrap_err());
        }
    }
}

impl TileGetter for TileCache {
    async fn tile_data(&self, tile: &Tile) -> TileDataResult {
        if self.path(&tile).exists() {
            return std::fs::read(self.path(tile)).map_err(|_| TileNotAvailableError);
        }
        Err(TileNotAvailableError)
    }
}

#[derive(Debug, Clone)]
struct TileLoader {
    base_url: String,
}

impl TileGetter for TileLoader {
    async fn tile_data(&self, tile: &Tile) -> TileDataResult {
        let url = format!("{}/{}/{}/{}.png", self.base_url, tile.zoom, tile.x, tile.y);
        info!("Downloading {}.", url);
        surf::get(&url)
            .recv_bytes()
            .await
            .map_err(|_| TileNotAvailableError)
    }
}

#[derive(Debug, Clone)]
pub struct CachedTileLoader {
    tile_cache: TileCache,
    tile_loader: TileLoader,
}

impl CachedTileLoader {
    async fn get_from_cache(&self, tile: &Tile) -> TileDataResult {
        self.tile_cache.tile_data(&tile).await
    }

    async fn download(&self, tile: &Tile) -> TileDataResult {
        info!("Downloading {:?}.", tile);
        match self.tile_loader.tile_data(tile).await {
            Ok(data) => {
                self.tile_cache.cache_tile(&tile, &data);
                match data.len() {
                    0..=100 => Err(TileNotAvailableError),
                    _ => Ok(data),
                }
            }
            Err(e) => {
                error!("Download of tile {:?} failed with: {:?}.", tile, e);
                Err(e)
            }
        }
    }
}

impl Default for CachedTileLoader {
    fn default() -> CachedTileLoader {
        let base_path = PathBuf::from("/home/udo/.imagecache");
        let tile_cache = TileCache { base_path };

        let base_url = String::from("https://tile.openstreetmap.org");
        let tile_loader = TileLoader { base_url };
        CachedTileLoader {
            tile_cache,
            tile_loader,
        }
    }
}

impl TileGetter for CachedTileLoader {
    async fn tile_data(&self, tile: &Tile) -> TileDataResult {
        trace!("Loading tile {:?}", &tile);
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
