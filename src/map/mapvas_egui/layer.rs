use crate::map::coordinates::{BoundingBox, Transform};
use egui::{Pos2, Rect, Ui};

/// Allows to display results of commands that return coordinates.
mod commands;
/// Drawing abstactions.
mod drawable;
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

/// Info about a named sub-layer within a layer (e.g. each id sent via `mapcat`).
pub struct SubLayerInfo {
  pub id: String,
  pub visible: bool,
  pub shape_count: usize,
}

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
  fn process_pending_events(&mut self) {}
  fn discard_pending_events(&mut self) {}
  fn ui(&mut self, ui: &mut Ui) {
    ui.collapsing(self.name().to_owned(), |ui| {
      ui.checkbox(self.visible_mut(), "visible");
      self.ui_content(ui);
    });
  }
  fn ui_content(&mut self, ui: &mut Ui);
  fn handle_double_click(&mut self, _pos: Pos2, _transform: &Transform) -> bool {
    false
  }
  fn find_closest_geometry(&mut self, _pos: Pos2, _transform: &Transform) -> (f64, bool) {
    (f64::INFINITY, false)
  }
  fn has_highlighted_geometry(&self) -> bool {
    false
  }

  fn has_double_click_action(&self) -> bool {
    false
  }

  /// Capability accessor: layers that support search/filter override this.
  fn as_searchable(&self) -> Option<&dyn Searchable> {
    None
  }

  fn as_searchable_mut(&mut self) -> Option<&mut dyn Searchable> {
    None
  }

  /// Find the closest geometry to the given position and handle selection if applicable
  /// Returns Some(distance) if this layer can handle the click, None otherwise
  /// If Some(distance) is returned, the layer should perform its selection action immediately
  fn closest_geometry_with_selection(&mut self, _pos: Pos2, _transform: &Transform) -> Option<f64> {
    None
  }
  /// Update the layer's configuration
  fn update_config(&mut self, _config: &crate::config::Config) {
    // Default implementation does nothing - layers can override if they need config updates
  }
  /// Set temporal filtering for this layer
  fn set_temporal_filter(
    &mut self,
    _current_time: Option<chrono::DateTime<chrono::Utc>>,
    _time_window: Option<chrono::Duration>,
  ) {
    // Default implementation does nothing - layers can override if they support temporal filtering
  }
  /// Check if this layer is visible
  fn is_visible(&self) -> bool {
    self.visible()
  }

  /// Set visibility of this layer
  fn set_visible(&mut self, visible: bool) {
    *self.visible_mut() = visible;
  }

  /// Get temporal range from this layer if it supports temporal data
  fn get_temporal_range(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  ) {
    (None, None) // Default implementation returns no temporal data
  }

  /// Whether this layer has pending async work (e.g. tiles downloading/rendering).
  fn has_pending_work(&self) -> bool {
    false
  }

  /// Configure this layer for headless rendering (disable preloading, etc.).
  fn set_headless(&mut self) {}

  /// Return info about named sub-layers (e.g. each id sent via `mapcat`).
  fn sub_layers(&self) -> Vec<SubLayerInfo> {
    vec![]
  }

  /// Set visibility of a named sub-layer.
  fn set_sub_layer_visible(&mut self, _id: &str, _visible: bool) {}

  /// Return the bounding box of a single named sub-layer.
  fn sub_layer_bounding_box(&self, _id: &str) -> Option<BoundingBox> {
    None
  }

  /// Return shape-level info for a named sub-layer.
  fn sub_layer_shapes(&self, _id: &str) -> Vec<crate::remote::ShapeInfo> {
    vec![]
  }

  /// Return the bounding box of a single shape within a sub-layer.
  fn shape_bounding_box(&self, _layer_id: &str, _shape_idx: usize) -> Option<BoundingBox> {
    None
  }
}

/// Capability for layers that support text search and filtering of their contents.
pub trait Searchable {
  fn search_geometries(&mut self, query: &str);
  fn next_search_result(&mut self) -> bool;
  fn previous_search_result(&mut self) -> bool;
  fn search_results_count(&self) -> usize;
  fn show_search_result_popup(&mut self);
  fn filter_geometries(&mut self, query: &str);
  fn clear_filter(&mut self);
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
