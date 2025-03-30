/// Handles screenshot functionality.
mod screenshot;
/// Draws and holds the shapes on the map.
mod shape_layer;
/// Draws the map.
mod tile_layer;

use egui::{Rect, Ui};
pub use screenshot::ScreenshotLayer;
pub use shape_layer::ShapeLayer;
pub use tile_layer::TileLayer;

use crate::map::coordinates::{BoundingBox, Transform};

/// A layer represents everything that can be summarized as a logical unit on the map.
/// E.g. a layer to draw the map tiles and one to draw the shapes.
pub trait Layer {
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, rect: Rect);
  fn clear(&mut self) {}
  fn name(&self) -> &str;
  fn visible(&self) -> bool;
  fn visible_mut(&mut self) -> &mut bool;
  fn bounding_box(&self) -> Option<BoundingBox> {
    None
  }
}

/// Common properties for all layers.
pub struct LayerProperties {
  pub visible: bool,
}

impl Default for LayerProperties {
  fn default() -> Self {
    Self { visible: true }
  }
}
