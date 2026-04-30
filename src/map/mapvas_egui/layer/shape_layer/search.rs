use super::{SearchPattern, ShapeLayer};
use crate::map::{coordinates::PixelCoordinate, geometry_collection::Geometry};

impl ShapeLayer {
  /// Search for geometries that match the given query string (supports regex)
  pub fn search_geometries(&mut self, query: &str) {
    self.search_results.clear();

    // Try to compile as regex first, fallback to literal string search
    let search_pattern = match regex::Regex::new(query) {
      Ok(regex) => SearchPattern::Regex(regex),
      Err(_) => {
        // If regex compilation fails, treat as literal string (case-insensitive)
        SearchPattern::Literal(query.to_lowercase())
      }
    };

    // Collect results first to avoid borrowing issues
    let mut results = Vec::new();

    for (layer_id, shapes) in &self.shape_map {
      for (shape_idx, shape) in shapes.iter().enumerate() {
        Self::search_in_geometry_static(
          layer_id,
          shape_idx,
          &Vec::new(),
          shape,
          &search_pattern,
          &mut results,
        );
      }
    }

    self.search_results = results.clone();

    // Highlight all found geometries
    if results.is_empty() {
      // Clear highlighting if no results found
      self.geometry_highlighter.clear_highlighting();
    } else {
      // Clear any previous highlighting
      self.geometry_highlighter.clear_highlighting();

      // Highlight the first search result
      if let Some((layer_id, shape_idx, nested_path)) = results.first() {
        self.highlight_geometry(layer_id, *shape_idx, nested_path);

        // Show popup for the first search result
        self.show_search_result_popup();
      }
    }
  }

  /// Get current search results
  #[must_use]
  pub fn get_search_results(&self) -> &Vec<(String, usize, Vec<usize>)> {
    &self.search_results
  }

  /// Show popup for currently highlighted search result
  pub fn show_search_result_popup(&mut self) {
    if let Some((layer_id, shape_idx, nested_path)) =
      self.geometry_highlighter.get_highlighted_geometry()
      && let Some(detail_info) =
        self.generate_geometry_detail_info(&layer_id, shape_idx, &nested_path)
    {
      // Find the geometry to get its representative coordinate for popup positioning
      if let Some(coord) =
        self.get_geometry_representative_coordinate(&layer_id, shape_idx, &nested_path)
      {
        // Convert to screen position using current transform
        let screen_pos = if self.current_transform.is_invalid() {
          egui::pos2(0.0, 0.0) // Fallback position
        } else {
          let pixel_pos = self.current_transform.apply(coord);
          egui::pos2(pixel_pos.x, pixel_pos.y)
        };

        let creation_time = std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap_or_default()
          .as_secs_f64();

        self.pending_detail_popup = Some((screen_pos, coord, detail_info, creation_time));
      }
    }
  }

  /// Navigate to next search result
  pub fn next_search_result(&mut self) -> bool {
    if self.search_results.is_empty() {
      return false;
    }

    let current_highlighted = self.geometry_highlighter.get_highlighted_geometry();
    let current_idx = if let Some(current) = current_highlighted {
      self
        .search_results
        .iter()
        .position(|result| result == &current)
    } else {
      None
    };

    let next_idx = match current_idx {
      Some(idx) => (idx + 1) % self.search_results.len(),
      None => 0,
    };

    if let Some((layer_id, shape_idx, nested_path)) = self.search_results.get(next_idx).cloned() {
      self.highlight_geometry(&layer_id, shape_idx, &nested_path);
      self.show_search_result_popup();
      true
    } else {
      false
    }
  }

  /// Navigate to previous search result
  pub fn previous_search_result(&mut self) -> bool {
    if self.search_results.is_empty() {
      return false;
    }

    let current_highlighted = self.geometry_highlighter.get_highlighted_geometry();
    let current_idx = if let Some(current) = current_highlighted {
      self
        .search_results
        .iter()
        .position(|result| result == &current)
    } else {
      None
    };

    let prev_idx = match current_idx {
      Some(idx) => {
        if idx == 0 {
          self.search_results.len() - 1
        } else {
          idx - 1
        }
      }
      None => self.search_results.len() - 1,
    };

    if let Some((layer_id, shape_idx, nested_path)) = self.search_results.get(prev_idx).cloned() {
      self.highlight_geometry(&layer_id, shape_idx, &nested_path);
      self.show_search_result_popup();
      true
    } else {
      false
    }
  }

  /// Get representative coordinate for a geometry (used for popup positioning)
  fn get_geometry_representative_coordinate(
    &self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
  ) -> Option<PixelCoordinate> {
    let shapes = self.shape_map.get(layer_id)?;
    let mut current_geometry = shapes.get(shape_idx)?;

    // Navigate to the specific nested geometry if there's a path
    for &idx in nested_path {
      if let Geometry::GeometryCollection(geometries, _) = current_geometry {
        current_geometry = geometries.get(idx)?;
      } else {
        return None; // Invalid path
      }
    }

    // Return representative coordinate based on geometry type
    Self::get_geometry_first_coordinate(current_geometry)
  }

  /// Get the first coordinate from any geometry type
  fn get_geometry_first_coordinate(
    geometry: &Geometry<PixelCoordinate>,
  ) -> Option<PixelCoordinate> {
    match geometry {
      Geometry::Point(coord, _) => Some(*coord),
      Geometry::LineString(coords, _) | Geometry::Polygon(coords, _) => coords.first().copied(),
      Geometry::Heatmap(coords, _) => coords.first().copied(),
      Geometry::GeometryCollection(geometries, _) => {
        // For collections, try to get coordinate from first child geometry
        geometries
          .first()
          .and_then(Self::get_geometry_first_coordinate)
      }
    }
  }

  /// Apply filter to hide non-matching geometries
  pub fn filter_geometries(&mut self, query: &str) {
    // Try to compile as regex first, fallback to literal string search
    let filter_pattern = match regex::Regex::new(query) {
      Ok(regex) => SearchPattern::Regex(regex),
      Err(_) => {
        // If regex compilation fails, treat as literal string (case-insensitive)
        SearchPattern::Literal(query.to_lowercase())
      }
    };

    self.filter_pattern = Some(filter_pattern);
    self.invalidate_cache();
  }

  /// Clear filter and show all geometries
  pub fn clear_filter(&mut self) {
    self.filter_pattern = None;
    self.invalidate_cache();
  }

  /// Check if a geometry matches the current filter
  pub(super) fn geometry_matches_filter(&self, geometry: &Geometry<PixelCoordinate>) -> bool {
    if let Some(ref pattern) = self.filter_pattern {
      Self::geometry_matches_pattern_static(geometry, pattern)
    } else {
      true // No filter active, show all geometries
    }
  }

  /// Check if a geometry matches a search pattern (static version)
  fn geometry_matches_pattern_static(
    geometry: &Geometry<PixelCoordinate>,
    pattern: &SearchPattern,
  ) -> bool {
    let metadata = match geometry {
      Geometry::Point(_, metadata)
      | Geometry::LineString(_, metadata)
      | Geometry::Polygon(_, metadata)
      | Geometry::GeometryCollection(_, metadata)
      | Geometry::Heatmap(_, metadata) => metadata,
    };

    // Check if metadata contains the search pattern
    if let Some(label) = &metadata.label {
      if Self::matches_pattern(&label.name, pattern)
        || Self::matches_pattern(&label.short(), pattern)
        || Self::matches_pattern(&label.full(), pattern)
      {
        return true;
      }

      if let Some(description) = &label.description
        && Self::matches_pattern(description, pattern)
      {
        return true;
      }
    }

    // For collections, check nested geometries recursively
    if let Geometry::GeometryCollection(geometries, _) = geometry {
      for nested_geometry in geometries {
        if Self::geometry_matches_pattern_static(nested_geometry, pattern) {
          return true;
        }
      }
    }

    false
  }

  /// Check if text matches the search pattern
  fn matches_pattern(text: &str, pattern: &SearchPattern) -> bool {
    match pattern {
      SearchPattern::Regex(regex) => regex.is_match(text),
      SearchPattern::Literal(literal) => text.to_lowercase().contains(literal),
    }
  }

  /// Recursively search within a geometry for the query string (static version to avoid borrow checker issues)
  fn search_in_geometry_static(
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
    geometry: &Geometry<PixelCoordinate>,
    pattern: &SearchPattern,
    results: &mut Vec<(String, usize, Vec<usize>)>,
  ) {
    let metadata = match geometry {
      Geometry::Point(_, metadata)
      | Geometry::LineString(_, metadata)
      | Geometry::Polygon(_, metadata)
      | Geometry::GeometryCollection(_, metadata)
      | Geometry::Heatmap(_, metadata) => metadata,
    };

    // Check if metadata contains the search pattern
    let mut matches = false;

    if let Some(label) = &metadata.label {
      if Self::matches_pattern(&label.name, pattern)
        || Self::matches_pattern(&label.short(), pattern)
        || Self::matches_pattern(&label.full(), pattern)
      {
        matches = true;
      }

      if let Some(description) = &label.description
        && Self::matches_pattern(description, pattern)
      {
        matches = true;
      }
    }

    if matches {
      results.push((layer_id.to_string(), shape_idx, nested_path.to_vec()));
    }

    // Recursively search in nested geometries
    if let Geometry::GeometryCollection(nested_geometries, _) = geometry {
      for (nested_idx, nested_geometry) in nested_geometries.iter().enumerate() {
        let mut nested_path = nested_path.to_vec();
        nested_path.push(nested_idx);
        Self::search_in_geometry_static(
          layer_id,
          shape_idx,
          &nested_path,
          nested_geometry,
          pattern,
          results,
        );
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::super::ShapeLayer;
  use crate::map::{
    coordinates::PixelCoordinate,
    geometry_collection::{Geometry, Label, Metadata},
  };

  fn point_with_label(name: &str) -> Geometry<PixelCoordinate> {
    Geometry::Point(
      PixelCoordinate { x: 0.0, y: 0.0 },
      Metadata::default().with_label(name),
    )
  }

  fn layer_with(shapes: Vec<Geometry<PixelCoordinate>>) -> ShapeLayer {
    let mut layer = ShapeLayer::new_with_test_receiver();
    layer.shape_map.insert("layer".to_string(), shapes);
    layer
  }

  // --- search_geometries ---

  #[test]
  fn search_empty_layer_returns_no_results() {
    let mut layer = layer_with(vec![]);
    layer.search_geometries("foo");
    assert!(layer.get_search_results().is_empty());
  }

  #[test]
  fn search_literal_match() {
    let mut layer = layer_with(vec![point_with_label("Berlin"), point_with_label("Paris")]);
    layer.search_geometries("Berlin");
    assert_eq!(layer.get_search_results().len(), 1);
    assert_eq!(layer.get_search_results()[0].0, "layer");
    assert_eq!(layer.get_search_results()[0].1, 0);
  }

  #[test]
  fn search_literal_case_insensitive() {
    // Invalid regex triggers the literal (case-insensitive) fallback path
    let mut layer = layer_with(vec![point_with_label("BERLIN")]);
    layer.search_geometries("[invalid regex");
    assert!(layer.get_search_results().is_empty());

    // Case-insensitive literal match via regex flag
    layer.search_geometries("(?i)berlin");
    assert_eq!(layer.get_search_results().len(), 1);
  }

  #[test]
  fn search_regex_match() {
    let mut layer = layer_with(vec![
      point_with_label("stop_123"),
      point_with_label("stop_456"),
      point_with_label("depot"),
    ]);
    layer.search_geometries("stop_[0-9]+");
    assert_eq!(layer.get_search_results().len(), 2);
  }

  #[test]
  fn search_no_match_clears_results() {
    let mut layer = layer_with(vec![point_with_label("Berlin")]);
    layer.search_geometries("Berlin");
    assert_eq!(layer.get_search_results().len(), 1);
    layer.search_geometries("Tokyo");
    assert!(layer.get_search_results().is_empty());
  }

  #[test]
  fn search_in_nested_collection() {
    let inner = point_with_label("Hamburg");
    let collection = Geometry::GeometryCollection(vec![inner], Metadata::default());
    let mut layer = layer_with(vec![collection]);
    layer.search_geometries("Hamburg");
    assert_eq!(layer.get_search_results().len(), 1);
    assert_eq!(layer.get_search_results()[0].2, vec![0]); // nested_path points to child
  }

  // --- next/previous_search_result ---

  #[test]
  fn next_result_cycles_through_results() {
    let mut layer = layer_with(vec![
      point_with_label("stop_1"),
      point_with_label("stop_2"),
      point_with_label("stop_3"),
    ]);
    layer.search_geometries("stop_");
    assert_eq!(layer.get_search_results().len(), 3);

    assert!(layer.next_search_result());
    assert!(layer.next_search_result());
    assert!(layer.next_search_result()); // wraps around
  }

  #[test]
  fn previous_result_on_empty_returns_false() {
    let mut layer = layer_with(vec![]);
    layer.search_geometries("anything");
    assert!(!layer.previous_search_result());
  }

  // --- filter_geometries / geometry_matches_filter ---

  #[test]
  fn filter_matches_hides_non_matching() {
    let berlin = point_with_label("Berlin");
    let paris = point_with_label("Paris");

    let layer = layer_with(vec![]);
    assert!(layer.geometry_matches_filter(&berlin)); // no filter → always true

    let mut layer = layer_with(vec![]);
    layer.filter_geometries("Berlin");
    assert!(layer.geometry_matches_filter(&berlin));
    assert!(!layer.geometry_matches_filter(&paris));
  }

  #[test]
  fn clear_filter_shows_all() {
    let mut layer = layer_with(vec![]);
    let paris = point_with_label("Paris");
    layer.filter_geometries("berlin");
    assert!(!layer.geometry_matches_filter(&paris));
    layer.clear_filter();
    assert!(layer.geometry_matches_filter(&paris));
  }

  #[test]
  fn filter_regex() {
    let mut layer = layer_with(vec![]);
    layer.filter_geometries("stop_[0-9]+");
    assert!(layer.geometry_matches_filter(&point_with_label("stop_42")));
    assert!(!layer.geometry_matches_filter(&point_with_label("depot")));
  }

  #[test]
  fn filter_matches_description() {
    let mut layer = layer_with(vec![]);
    layer.filter_geometries("central hub");
    let geo = Geometry::Point(
      PixelCoordinate { x: 0.0, y: 0.0 },
      Metadata::default()
        .with_label(Label::new("stop".to_string()).with_description("central hub".to_string())),
    );
    assert!(layer.geometry_matches_filter(&geo));
  }
}
