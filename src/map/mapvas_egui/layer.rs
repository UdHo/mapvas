use crate::map::coordinates::{BoundingBox, Transform};
use egui::{Rect, Ui};

/// Allows to display results of commands that return coordinates.
mod commands;
/// Drawing abstactions.
mod drawable;
/// Handles screenshot functionality.
mod screenshot;
/// Draws and holds the shapes on the map.
mod shape_layer;
/// Draws the map.
mod tile_layer;

pub use commands::{CommandLayer, ParameterUpdate};
pub use screenshot::ScreenshotLayer;
pub use shape_layer::ShapeLayer;
pub use tile_layer::TileLayer;

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
  fn ui(&mut self, ui: &mut Ui) {
    ui.collapsing(self.name().to_owned(), |ui| {
      ui.checkbox(self.visible_mut(), "visible");
      self.ui_content(ui);
    });
  }
  fn ui_content(&mut self, ui: &mut Ui);
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
