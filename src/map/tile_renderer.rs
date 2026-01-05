mod raster;

pub use raster::RasterTileRenderer;

use egui::ColorImage;
use thiserror::Error;

use super::coordinates::Tile;

#[derive(Error, Debug)]
pub enum TileRenderError {
  #[error("Failed to decode image: {0}")]
  ImageDecode(String),
  #[error("Failed to parse tile data: {0}")]
  ParseError(String),
  #[error("Unsupported tile format")]
  UnsupportedFormat,
}

/// Trait for rendering tile data into a `ColorImage`.
///
/// This trait abstracts the rendering of tile data, allowing different
/// implementations for raster tiles (PNG/JPEG) and vector tiles (MVT/PBF).
pub trait TileRenderer: Send + Sync {
  /// Render raw tile data into a `ColorImage`.
  ///
  /// # Arguments
  /// * `tile` - The tile coordinates and zoom level
  /// * `data` - Raw tile data bytes (PNG, JPEG, MVT, etc.)
  ///
  /// # Returns
  /// A `ColorImage` ready to be uploaded as a texture, or an error.
  ///
  /// # Errors
  /// Returns `TileRenderError` if the tile data cannot be decoded or rendered.
  fn render(&self, tile: &Tile, data: &[u8]) -> Result<ColorImage, TileRenderError>;

  /// Returns the name of this renderer for display purposes.
  fn name(&self) -> &'static str;
}
