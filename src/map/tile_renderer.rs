mod raster;
mod vector;

pub use raster::RasterTileRenderer;
pub use vector::VectorTileRenderer;
pub use vector::styling::{
  init_style_config, save_style_config, set_style_config, style_config, style_version, Rgb,
  RoadStyle, StyleConfig,
};

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

  /// Render raw tile data at a specific scale factor.
  ///
  /// # Arguments
  /// * `tile` - The tile coordinates and zoom level
  /// * `data` - Raw tile data bytes
  /// * `scale` - Scale factor (e.g., 2 for 2x resolution, resulting in 512x512 for standard 256x256 tiles)
  ///
  /// # Returns
  /// A `ColorImage` at the scaled resolution, or an error.
  ///
  /// # Errors
  /// Returns `TileRenderError` if the tile data cannot be decoded or rendered.
  ///
  /// Default implementation calls `render()` and ignores the scale (suitable for raster tiles).
  fn render_scaled(&self, tile: &Tile, data: &[u8], _scale: u32) -> Result<ColorImage, TileRenderError> {
    self.render(tile, data)
  }

  /// Returns the name of this renderer for display purposes.
  fn name(&self) -> &'static str;
}
