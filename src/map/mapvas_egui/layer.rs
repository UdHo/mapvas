use crate::map::{
  coordinates::{PixelPosition, PixelRect, Transform},
  layer::Layer,
};
use egui::{Rect, Ui};

/// Allows to display results of commands that return coordinates.
mod commands;
/// Geometry highlighting logic.
mod geometry_highlighting;
/// Offscreen geometry rasterization for cached rendering.
mod geometry_rasterizer;
/// Geometry selection and closest point calculations.
mod geometry_selection;
/// Handles screenshot functionality.
mod screenshot;
/// Draws and holds the shapes on the map.
mod shape_layer;
/// Draws the map.
mod tile_layer;
/// Timeline overlay for temporal visualization.
mod timeline_layer;

pub use commands::{CommandLayer, ParameterUpdate};
pub use screenshot::ScreenshotLayer;
pub use shape_layer::ShapeLayer;
pub use tile_layer::TileLayer;
pub use timeline_layer::TimelineLayer;

#[derive(Debug, Clone, Copy)]
pub struct EguiMapFrame {
  pub rect: Rect,
  pub pixel_rect: PixelRect,
}

impl EguiMapFrame {
  #[must_use]
  pub fn from_rect(rect: Rect) -> Self {
    Self {
      rect,
      pixel_rect: PixelRect::from_min_max(
        PixelPosition {
          x: rect.min.x,
          y: rect.min.y,
        },
        PixelPosition {
          x: rect.max.x,
          y: rect.max.y,
        },
      ),
    }
  }
}

/// Egui-specific layer hooks used by the legacy egui map renderer and egui overlays.
pub trait EguiLayer: Layer {
  fn draw_egui(&mut self, ui: &mut Ui, transform: &Transform, frame: EguiMapFrame);

  fn draw_egui_overlay(&mut self, _ui: &mut Ui, _transform: &Transform) {}

  /// Draw egui attribution or licensing overlays after all map content is painted.
  fn draw_egui_attribution(&self, _ui: &mut Ui, _clip_rect: Rect) {}

  fn ui_egui_controls(&mut self, ui: &mut Ui) {
    ui.collapsing(self.name().to_owned(), |ui| {
      ui.checkbox(self.visible_mut(), "visible");
      self.ui_egui_content(ui);
    });
  }

  fn ui_egui_content(&mut self, ui: &mut Ui);
}
