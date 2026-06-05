use super::{
  coordinates::{BoundingBox, PixelPosition, Transform},
  viewport::GeometrySnapshot,
};

/// Info about a named sub-layer within a layer (e.g. each id sent via `mapcat`).
pub struct SubLayerInfo {
  pub id: String,
  pub visible: bool,
  pub shape_count: usize,
}

/// A layer represents everything that can be summarized as a logical unit on the map.
/// E.g. a layer to draw the map tiles and one to draw the shapes.
pub trait Layer {
  fn handle_hover(&mut self, _pos: Option<PixelPosition>, _transform: &Transform) {}
  fn clear(&mut self) {}
  fn name(&self) -> &str;
  fn visible(&self) -> bool;
  fn visible_mut(&mut self) -> &mut bool;
  fn bounding_box(&self) -> Option<BoundingBox> {
    None
  }
  fn process_pending_events(&mut self) {}
  fn discard_pending_events(&mut self) {}
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

  /// Find the closest geometry to the given position and handle selection if applicable.
  /// Returns Some(distance) if this layer can handle the click, None otherwise.
  /// If Some(distance) is returned, the layer should perform its selection action immediately.
  fn closest_geometry_with_selection(
    &mut self,
    _pos: PixelPosition,
    _transform: &Transform,
  ) -> Option<f64> {
    None
  }

  /// Update the layer's configuration.
  fn update_config(&mut self, _config: &crate::config::Config) {}

  /// Set temporal filtering for this layer.
  fn set_temporal_filter(
    &mut self,
    _current_time: Option<chrono::DateTime<chrono::Utc>>,
    _time_window: Option<chrono::Duration>,
  ) {
  }

  /// Check if this layer is visible.
  fn is_visible(&self) -> bool {
    self.visible()
  }

  /// Set visibility of this layer.
  fn set_visible(&mut self, visible: bool) {
    *self.visible_mut() = visible;
  }

  /// Get temporal range from this layer if it supports temporal data.
  fn get_temporal_range(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  ) {
    (None, None)
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

  /// Return visible map geometry for Bevy renderer paths.
  fn geometry_snapshot(&self) -> GeometrySnapshot {
    GeometrySnapshot::default()
  }

  /// Return snapshot metadata and transient overlays without re-collecting base geometry.
  fn geometry_snapshot_without_geometries(&self) -> GeometrySnapshot {
    GeometrySnapshot {
      version: self.geometry_snapshot_version(),
      geometry_version: self.geometry_base_snapshot_version(),
      geometries: Vec::new(),
      highlighted_geometries: Vec::new(),
    }
  }

  /// Return the version used to decide whether a geometry snapshot needs refreshing.
  fn geometry_snapshot_version(&self) -> u64 {
    0
  }

  /// Return the version for base geometry, excluding transient highlight-only changes.
  fn geometry_base_snapshot_version(&self) -> u64 {
    self.geometry_snapshot_version()
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
