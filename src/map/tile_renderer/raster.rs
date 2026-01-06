use egui::ColorImage;

use super::{TileRenderError, TileRenderer};
use crate::map::coordinates::Tile;

/// Raster tile renderer that decodes PNG/JPEG images.
///
/// This is the default renderer for standard raster tile sources like OSM.
#[derive(Debug, Clone, Default)]
pub struct RasterTileRenderer;

impl RasterTileRenderer {
  #[must_use]
  pub fn new() -> Self {
    Self
  }
}

impl TileRenderer for RasterTileRenderer {
  fn render(&self, tile: &Tile, data: &[u8]) -> Result<ColorImage, TileRenderError> {
    let start = std::time::Instant::now();

    let img_reader =
      image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().map_err(|e| {
        TileRenderError::ImageDecode(format!("Failed to create image reader for {tile:?}: {e}"))
      })?;

    let img = img_reader.decode().map_err(|e| {
      TileRenderError::ImageDecode(format!("Failed to decode image for {tile:?}: {e}"))
    })?;

    let size = [img.width() as usize, img.height() as usize];
    let image_buffer = img.to_rgba8();
    let pixels = image_buffer.as_flat_samples();

    let total_time = start.elapsed();
    log::info!("Tile {tile:?} (raster) decoded in {total_time:?}");

    Ok(ColorImage::from_rgba_unmultiplied(size, pixels.as_slice()))
  }

  fn name(&self) -> &'static str {
    "Raster"
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_raster_renderer_name() {
    let renderer = RasterTileRenderer::new();
    assert_eq!(renderer.name(), "Raster");
  }

  #[test]
  fn test_raster_renderer_invalid_data() {
    let renderer = RasterTileRenderer::new();
    let tile = Tile { x: 0, y: 0, zoom: 0 };
    let result = renderer.render(&tile, &[0, 1, 2, 3]);
    assert!(result.is_err());
  }
}
