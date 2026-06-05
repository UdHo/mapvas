mod raster;
mod vector;

pub use raster::RasterTileRenderer;
pub use vector::VectorTileRenderer;
pub use vector::styling::{
  MapStyle, Rgb, RoadStyle, StyleConfig, background_color, get_fill_color, get_place_font_size,
  get_road_styling, init_style_config, save_style_config, set_style_config, should_show_place,
  style_config, style_version,
};

use thiserror::Error;

use super::coordinates::Tile;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileImage {
  pub size: [usize; 2],
  pub rgba: Vec<u8>,
}

impl TileImage {
  #[must_use]
  pub fn from_rgba_unmultiplied(size: [usize; 2], rgba: Vec<u8>) -> Self {
    Self { size, rgba }
  }

  #[must_use]
  pub fn width(&self) -> usize {
    self.size[0]
  }

  #[must_use]
  pub fn height(&self) -> usize {
    self.size[1]
  }

  #[must_use]
  pub fn as_raw(&self) -> &[u8] {
    &self.rgba
  }
}

#[derive(Error, Debug)]
pub enum TileRenderError {
  #[error("Failed to decode image: {0}")]
  ImageDecode(String),
  #[error("Failed to parse tile data: {0}")]
  ParseError(String),
  #[error("Unsupported tile format")]
  UnsupportedFormat,
}

/// Trait for rendering tile data into a `TileImage`.
///
/// This trait abstracts the rendering of tile data, allowing different
/// implementations for raster tiles (PNG/JPEG) and vector tiles (MVT/PBF).
pub trait TileRenderer: Send + Sync {
  /// Render raw tile data into a `TileImage`.
  ///
  /// # Arguments
  /// * `tile` - The tile coordinates and zoom level
  /// * `data` - Raw tile data bytes (PNG, JPEG, MVT, etc.)
  ///
  /// # Returns
  /// A `TileImage` ready to be uploaded as a texture, or an error.
  ///
  /// # Errors
  /// Returns `TileRenderError` if the tile data cannot be decoded or rendered.
  fn render(&self, tile: &Tile, data: &[u8]) -> Result<TileImage, TileRenderError>;

  /// Render raw tile data at a specific scale factor.
  ///
  /// # Arguments
  /// * `tile` - The tile coordinates and zoom level
  /// * `data` - Raw tile data bytes
  /// * `scale` - Scale factor (e.g., 2 for 2x resolution, resulting in 512x512 for standard 256x256 tiles)
  ///
  /// # Returns
  /// A `TileImage` at the scaled resolution, or an error.
  ///
  /// # Errors
  /// Returns `TileRenderError` if the tile data cannot be decoded or rendered.
  ///
  /// Default implementation calls `render()` and ignores the scale (suitable for raster tiles).
  fn render_scaled(
    &self,
    tile: &Tile,
    data: &[u8],
    _scale: u32,
  ) -> Result<TileImage, TileRenderError> {
    self.render(tile, data)
  }

  /// Returns the name of this renderer for display purposes.
  fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tile_image_exposes_dimensions_and_raw_rgba() {
    let source = TileImage::from_rgba_unmultiplied([2, 1], vec![255, 0, 0, 255, 0, 128, 255, 64]);

    assert_eq!(source.width(), 2);
    assert_eq!(source.height(), 1);
    assert_eq!(source.as_raw(), [255, 0, 0, 255, 0, 128, 255, 64]);
  }
}
