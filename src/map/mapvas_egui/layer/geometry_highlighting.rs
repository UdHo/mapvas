use crate::map::{
  coordinates::{PixelCoordinate, Transform},
  geometry_collection::{DEFAULT_STYLE, Geometry, Metadata},
};
use egui::{
  Color32, Painter, Shape, Stroke,
  epaint::{CircleShape, PathShape, PathStroke},
};
use std::collections::HashMap;

/// Manages geometry highlighting with unique IDs
pub struct GeometryHighlighter {
  highlighted_geometry_id: Option<u64>,
  next_geometry_id: u64,
  geometry_id_map: HashMap<(String, usize, Vec<usize>), u64>,
  just_highlighted: bool,
}

impl GeometryHighlighter {
  pub fn new() -> Self {
    Self {
      highlighted_geometry_id: None,
      next_geometry_id: 1,
      geometry_id_map: HashMap::new(),
      just_highlighted: false,
    }
  }

  /// Check if a geometry is currently highlighted
  pub fn is_highlighted(&self, layer_id: &str, shape_idx: usize, nested_path: &[usize]) -> bool {
    let geometry_key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
    if let Some(geometry_id) = self.geometry_id_map.get(&geometry_key) {
      self.highlighted_geometry_id == Some(*geometry_id)
    } else {
      false
    }
  }

  /// Highlight a geometry by its path
  pub fn highlight_geometry(
    &mut self,
    layer_id: String,
    shape_idx: usize,
    nested_path: Vec<usize>,
  ) {
    let geometry_id = self.get_or_create_geometry_id(&layer_id, shape_idx, &nested_path);
    self.highlight_geometry_by_id(geometry_id);
  }

  /// Highlight a geometry by its unique ID
  pub fn highlight_geometry_by_id(&mut self, geometry_id: u64) {
    self.highlighted_geometry_id = Some(geometry_id);
    self.just_highlighted = true;
  }

  /// Clear highlighting
  pub fn clear_highlighting(&mut self) {
    self.highlighted_geometry_id = None;
    self.just_highlighted = false;
  }

  /// Check if any geometry is highlighted
  pub fn has_highlighted_geometry(&self) -> bool {
    self.highlighted_geometry_id.is_some()
  }

  /// Check if highlighting was just set (for pagination updates)
  pub fn was_just_highlighted(&mut self) -> bool {
    let result = self.just_highlighted;
    self.just_highlighted = false;
    result
  }

  /// Get the currently highlighted geometry if any
  pub fn get_highlighted_geometry(&self) -> Option<(String, usize, Vec<usize>)> {
    if let Some(highlighted_id) = self.highlighted_geometry_id {
      // Find the geometry path that matches the highlighted ID
      for ((layer_id, shape_idx, nested_path), geometry_id) in &self.geometry_id_map {
        if *geometry_id == highlighted_id {
          return Some((layer_id.clone(), *shape_idx, nested_path.clone()));
        }
      }
    }
    None
  }

  /// Generate a unique ID for a geometry
  fn generate_geometry_id(&mut self) -> u64 {
    let id = self.next_geometry_id;
    self.next_geometry_id += 1;
    id
  }

  /// Get or create a unique ID for a geometry at the given path
  fn get_or_create_geometry_id(
    &mut self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
  ) -> u64 {
    let key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
    if let Some(&existing_id) = self.geometry_id_map.get(&key) {
      existing_id
    } else {
      let new_id = self.generate_geometry_id();
      self.geometry_id_map.insert(key.clone(), new_id);
      new_id
    }
  }
}

/// Draw highlighted geometry with solid colors
pub fn draw_highlighted_geometry(
  geometry: &Geometry<PixelCoordinate>,
  painter: &Painter,
  transform: &Transform,
  _highlight_all: bool,
) {
  match geometry {
    Geometry::Point(coord, metadata) => {
      draw_highlighted_point(coord, metadata, painter, transform);
    }
    Geometry::LineString(coords, metadata) => {
      draw_highlighted_linestring(coords, metadata, painter, transform);
    }
    Geometry::Polygon(coords, metadata) => {
      draw_highlighted_polygon(coords, metadata, painter, transform);
    }
    Geometry::GeometryCollection(geometries, _) => {
      // Draw all geometries in the collection as highlighted
      for nested_geometry in geometries {
        draw_highlighted_geometry(nested_geometry, painter, transform, false);
      }
    }
  }
}

/// Draw a highlighted point
fn draw_highlighted_point(
  coord: &PixelCoordinate,
  metadata: &Metadata,
  painter: &Painter,
  transform: &Transform,
) {
  let center = transform.apply(*coord).into();
  let base_color = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE).color();

  // Draw highlighted point filled with original color
  let highlight_fill = base_color;
  let highlight_stroke = base_color;

  let circle_shape = Shape::Circle(CircleShape {
    center,
    radius: 10.0,
    fill: highlight_fill,
    stroke: Stroke::new(2.0, highlight_stroke),
  });
  painter.add(circle_shape);
}

/// Draw a highlighted linestring
fn draw_highlighted_linestring(
  coords: &[PixelCoordinate],
  metadata: &Metadata,
  painter: &Painter,
  transform: &Transform,
) {
  let base_color = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE).color();

  // Draw highlighted linestring as solid thick line
  let shape = Shape::Path(PathShape {
    points: coords.iter().map(|c| transform.apply(*c).into()).collect(),
    closed: false,
    fill: Color32::TRANSPARENT,
    stroke: PathStroke::new(6.0, base_color), // Thicker and solid
  });
  painter.add(shape);
}

/// Draw a highlighted polygon
fn draw_highlighted_polygon(
  coords: &[PixelCoordinate],
  metadata: &Metadata,
  painter: &Painter,
  transform: &Transform,
) {
  let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
  let base_color = style.color();

  // Draw highlighted polygon with solid colors - fill matches stroke color
  let highlight_stroke = base_color; // Solid stroke in original color
  let highlight_fill = base_color; // Fill always matches stroke color

  let shape = Shape::Path(PathShape {
    points: coords.iter().map(|c| transform.apply(*c).into()).collect(),
    closed: true,
    fill: highlight_fill,
    stroke: PathStroke::new(6.0, highlight_stroke),
  });
  painter.add(shape);
}
