use super::super::geometry_selection;
use super::ShapeLayer;
use crate::map::{
  coordinates::{Coordinate, PixelCoordinate, Transform},
  geometry_collection::Geometry,
};
use chrono::{DateTime, Utc};
use egui::Pos2;
use std::fmt::Write;

impl ShapeLayer {
  /// Navigate to a specific geometry within nested collections
  pub(super) fn get_geometry_at_path<'a>(
    geometry: &'a Geometry<PixelCoordinate>,
    nested_path: &[usize],
  ) -> Option<&'a Geometry<PixelCoordinate>> {
    let mut current_geometry = geometry;

    for &path_index in nested_path {
      match current_geometry {
        Geometry::GeometryCollection(nested_geometries, _)
          if path_index < nested_geometries.len() =>
        {
          current_geometry = &nested_geometries[path_index];
        }
        _ => return None,
      }
    }

    Some(current_geometry)
  }

  /// Get temporal range from all geometries in this layer
  #[must_use]
  pub fn get_temporal_range(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let mut earliest: Option<DateTime<Utc>> = None;
    let mut latest: Option<DateTime<Utc>> = None;

    for shapes in self.shape_map.values() {
      for shape in shapes {
        Self::extract_temporal_from_geometry(shape, &mut earliest, &mut latest);
      }
    }

    (earliest, latest)
  }

  /// Recursively extract temporal data from a geometry and its children
  fn extract_temporal_from_geometry(
    geometry: &Geometry<PixelCoordinate>,
    earliest: &mut Option<DateTime<Utc>>,
    latest: &mut Option<DateTime<Utc>>,
  ) {
    let metadata = match geometry {
      Geometry::Point(_, meta)
      | Geometry::LineString(_, meta)
      | Geometry::Polygon(_, meta)
      | Geometry::Heatmap(_, meta) => meta,
      Geometry::GeometryCollection(children, meta) => {
        // Recursively process child geometries first
        for child in children {
          Self::extract_temporal_from_geometry(child, earliest, latest);
        }
        meta
      }
    };

    // Extract temporal data from this geometry's metadata
    if let Some(time_data) = &metadata.time_data {
      if let Some(timestamp) = time_data.timestamp {
        *earliest = Some(earliest.map_or(timestamp, |e| e.min(timestamp)));
        *latest = Some(latest.map_or(timestamp, |l| l.max(timestamp)));
      }

      if let Some(time_span) = &time_data.time_span {
        if let Some(begin) = time_span.begin {
          *earliest = Some(earliest.map_or(begin, |e| e.min(begin)));
        }
        if let Some(end) = time_span.end {
          *latest = Some(latest.map_or(end, |l| l.max(end)));
        }
      }
    }
  }

  /// Check if a top-level geometry should be visible at the given time
  pub(super) fn is_geometry_visible_at_time(
    &self,
    geometry: &Geometry<PixelCoordinate>,
    current_time: DateTime<Utc>,
  ) -> bool {
    match geometry {
      Geometry::Point(_, meta)
      | Geometry::LineString(_, meta)
      | Geometry::Polygon(_, meta)
      | Geometry::Heatmap(_, meta) => {
        // For individual geometries, check their metadata
        if let Some(time_window) = self.temporal_time_window {
          meta.is_visible_in_time_window(current_time, time_window)
        } else {
          meta.is_visible_at_time(current_time)
        }
      }
      Geometry::GeometryCollection(children, meta) => {
        // For GeometryCollections, first check if the collection itself has temporal data
        if meta.time_data.is_some() {
          if let Some(time_window) = self.temporal_time_window {
            meta.is_visible_in_time_window(current_time, time_window)
          } else {
            meta.is_visible_at_time(current_time)
          }
        } else {
          // If collection has no temporal data, check if ANY child is visible
          // We still show the collection if at least one child is visible
          children
            .iter()
            .any(|child| self.is_geometry_visible_at_time(child, current_time))
        }
      }
    }
  }

  /// Check if an individual geometry (not a collection) should be visible at the given time
  pub(super) fn is_individual_geometry_visible_at_time(
    &self,
    geometry: &Geometry<PixelCoordinate>,
    current_time: DateTime<Utc>,
  ) -> bool {
    let meta = match geometry {
      Geometry::Point(_, meta)
      | Geometry::LineString(_, meta)
      | Geometry::Polygon(_, meta)
      | Geometry::GeometryCollection(_, meta)
      | Geometry::Heatmap(_, meta) => meta, // Collections shouldn't reach here, but handle gracefully
    };

    if let Some(time_window) = self.temporal_time_window {
      meta.is_visible_in_time_window(current_time, time_window)
    } else {
      meta.is_visible_at_time(current_time)
    }
  }

  /// Generate detailed information about a geometry for popup display
  #[allow(clippy::too_many_lines)]
  pub(super) fn generate_geometry_detail_info(
    &self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
  ) -> Option<String> {
    let shapes = self.shape_map.get(layer_id)?;
    let current_shape = shapes.get(shape_idx)?;
    let mut current_geometry = current_shape;

    // Navigate to the specific nested geometry if there's a path
    for &idx in nested_path {
      if let Geometry::GeometryCollection(geometries, _) = current_geometry {
        current_geometry = geometries.get(idx)?;
      } else {
        return None; // Invalid path
      }
    }

    // Generate basic information for geometry type
    let detail_info = match current_geometry {
      Geometry::Point(coord, metadata) => {
        let wgs84 = coord.as_wgs84();
        let mut info = format!("📍 Point\nCoordinates: {:.6}, {:.6}", wgs84.lat, wgs84.lon);

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        if let Some(time_data) = &metadata.time_data
          && let Some(timestamp) = time_data.timestamp
        {
          write!(
            info,
            "\nTimestamp: {}",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
          )
          .unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }

      Geometry::LineString(coords, metadata) => {
        let mut info = format!("📏 LineString\nPoints: {}", coords.len());

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        if let Some(time_data) = &metadata.time_data
          && let Some(timestamp) = time_data.timestamp
        {
          write!(
            info,
            "\nTimestamp: {}",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
          )
          .unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }

      Geometry::Polygon(coords, metadata) => {
        let mut info = format!("⬟ Polygon\nVertices: {}", coords.len());

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        if let Some(time_data) = &metadata.time_data
          && let Some(timestamp) = time_data.timestamp
        {
          write!(
            info,
            "\nTimestamp: {}",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
          )
          .unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }

      Geometry::Heatmap(coords, metadata) => {
        let mut info = format!("🔥 Heatmap\nPoints: {}", coords.len());

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }

      Geometry::GeometryCollection(geometries, metadata) => {
        let mut info = format!("📁 Collection\nItems: {}", geometries.len());

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        if let Some(time_data) = &metadata.time_data
          && let Some(timestamp) = time_data.timestamp
        {
          write!(
            info,
            "\nTimestamp: {}",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
          )
          .unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }
    };

    Some(detail_info)
  }

  /// Recursively find the closest individual geometry to a point
  #[allow(clippy::too_many_arguments)]
  pub(super) fn find_closest_in_geometry(
    &self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
    geometry: &Geometry<PixelCoordinate>,
    click_pos: Pos2,
    transform: &Transform,
    closest_distance: &mut f64,
    closest_geometry: &mut Option<(String, usize, Vec<usize>)>,
  ) {
    geometry_selection::find_closest_in_geometry(
      layer_id,
      shape_idx,
      nested_path,
      geometry,
      click_pos,
      transform,
      closest_distance,
      closest_geometry,
      |layer_id, shape_idx, nested_path| {
        // Nested visibility check
        let nested_key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
        *self
          .nested_geometry_visibility
          .get(&nested_key)
          .unwrap_or(&true)
      },
      |nested_geometry| {
        // Temporal visibility check
        if let Some(current_time) = self.temporal_current_time {
          self.is_individual_geometry_visible_at_time(nested_geometry, current_time)
        } else {
          true
        }
      },
    );
  }
}

#[cfg(test)]
mod tests {
  use super::super::ShapeLayer;
  use crate::map::{
    coordinates::PixelCoordinate,
    geometry_collection::{Geometry, Metadata},
  };
  use chrono::{Duration, TimeZone, Utc};

  fn layer_with_shapes(shapes: Vec<Geometry<PixelCoordinate>>) -> ShapeLayer {
    let mut layer = ShapeLayer::new_with_test_receiver();
    layer.shape_map.insert("test".to_string(), shapes);
    layer
  }

  fn point(meta: Metadata) -> Geometry<PixelCoordinate> {
    Geometry::Point(PixelCoordinate { x: 0.0, y: 0.0 }, meta)
  }

  fn t(year: i32, month: u32, day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap()
  }

  // --- get_temporal_range ---

  #[test]
  fn temporal_range_empty_layer() {
    let layer = layer_with_shapes(vec![]);
    assert_eq!(layer.get_temporal_range(), (None, None));
  }

  #[test]
  fn temporal_range_no_time_data() {
    let layer = layer_with_shapes(vec![point(Metadata::default())]);
    assert_eq!(layer.get_temporal_range(), (None, None));
  }

  #[test]
  fn temporal_range_single_timestamp() {
    let ts = t(2024, 6, 1);
    let layer = layer_with_shapes(vec![point(Metadata::default().with_timestamp(ts))]);
    assert_eq!(layer.get_temporal_range(), (Some(ts), Some(ts)));
  }

  #[test]
  fn temporal_range_multiple_timestamps() {
    let t1 = t(2024, 1, 1);
    let t2 = t(2024, 6, 1);
    let t3 = t(2024, 12, 31);
    let layer = layer_with_shapes(vec![
      point(Metadata::default().with_timestamp(t2)),
      point(Metadata::default().with_timestamp(t1)),
      point(Metadata::default().with_timestamp(t3)),
    ]);
    assert_eq!(layer.get_temporal_range(), (Some(t1), Some(t3)));
  }

  #[test]
  fn temporal_range_from_time_span() {
    let begin = t(2024, 3, 1);
    let end = t(2024, 9, 1);
    let layer = layer_with_shapes(vec![point(
      Metadata::default().with_time_span(Some(begin), Some(end)),
    )]);
    assert_eq!(layer.get_temporal_range(), (Some(begin), Some(end)));
  }

  #[test]
  fn temporal_range_nested_in_collection() {
    let ts = t(2024, 6, 15);
    let nested = point(Metadata::default().with_timestamp(ts));
    let collection = Geometry::GeometryCollection(vec![nested], Metadata::default());
    let layer = layer_with_shapes(vec![collection]);
    assert_eq!(layer.get_temporal_range(), (Some(ts), Some(ts)));
  }

  // --- is_geometry_visible_at_time ---

  #[test]
  fn visible_at_time_no_time_data_always_visible() {
    let layer = layer_with_shapes(vec![]);
    let geo = point(Metadata::default());
    assert!(layer.is_geometry_visible_at_time(&geo, t(2024, 1, 1)));
  }

  #[test]
  fn visible_at_time_matching_timestamp() {
    let layer = layer_with_shapes(vec![]);
    let ts = t(2024, 6, 1);
    let geo = point(Metadata::default().with_timestamp(ts));
    assert!(layer.is_geometry_visible_at_time(&geo, ts));
  }

  #[test]
  fn visible_at_time_non_matching_timestamp() {
    let layer = layer_with_shapes(vec![]);
    let geo = point(Metadata::default().with_timestamp(t(2024, 6, 1)));
    assert!(!layer.is_geometry_visible_at_time(&geo, t(2024, 1, 1)));
  }

  #[test]
  fn visible_at_time_within_span() {
    let layer = layer_with_shapes(vec![]);
    let geo = point(Metadata::default().with_time_span(Some(t(2024, 1, 1)), Some(t(2024, 12, 31))));
    assert!(layer.is_geometry_visible_at_time(&geo, t(2024, 6, 15)));
  }

  #[test]
  fn visible_at_time_outside_span() {
    let layer = layer_with_shapes(vec![]);
    let geo = point(Metadata::default().with_time_span(Some(t(2024, 1, 1)), Some(t(2024, 6, 30))));
    assert!(!layer.is_geometry_visible_at_time(&geo, t(2024, 12, 1)));
  }

  #[test]
  fn visible_at_time_collection_no_own_time_data_delegates_to_children() {
    let mut layer = layer_with_shapes(vec![]);
    layer.temporal_current_time = Some(t(2024, 6, 1));
    let visible_child = point(Metadata::default().with_timestamp(t(2024, 6, 1)));
    let hidden_child = point(Metadata::default().with_timestamp(t(2024, 1, 1)));
    let collection =
      Geometry::GeometryCollection(vec![visible_child, hidden_child], Metadata::default());
    assert!(layer.is_geometry_visible_at_time(&collection, t(2024, 6, 1)));
  }

  #[test]
  fn visible_at_time_collection_all_children_hidden() {
    let layer = layer_with_shapes(vec![]);
    let child1 = point(Metadata::default().with_timestamp(t(2024, 1, 1)));
    let child2 = point(Metadata::default().with_timestamp(t(2024, 2, 1)));
    let collection = Geometry::GeometryCollection(vec![child1, child2], Metadata::default());
    assert!(!layer.is_geometry_visible_at_time(&collection, t(2024, 12, 1)));
  }

  // --- is_geometry_visible_at_time with time window ---

  #[test]
  fn visible_with_time_window_timestamp_in_window() {
    let mut layer = layer_with_shapes(vec![]);
    layer.temporal_time_window = Some(Duration::hours(1));
    let ts = t(2024, 6, 1);
    let geo = point(Metadata::default().with_timestamp(ts));
    // current_time is 30 min after ts — within 1h window
    assert!(layer.is_geometry_visible_at_time(&geo, ts + Duration::minutes(30)));
  }

  #[test]
  fn visible_with_time_window_timestamp_outside_window() {
    let mut layer = layer_with_shapes(vec![]);
    layer.temporal_time_window = Some(Duration::hours(1));
    let ts = t(2024, 6, 1);
    let geo = point(Metadata::default().with_timestamp(ts));
    // current_time is 2h after ts — outside 1h window
    assert!(!layer.is_geometry_visible_at_time(&geo, ts + Duration::hours(2)));
  }

  // --- get_geometry_at_path ---

  #[test]
  fn get_geometry_at_path_empty_path_returns_root() {
    let geo = point(Metadata::default());
    let result = ShapeLayer::get_geometry_at_path(&geo, &[]);
    assert!(result.is_some());
  }

  #[test]
  fn get_geometry_at_path_valid_nested() {
    let inner = point(Metadata::default().with_label("inner"));
    let collection = Geometry::GeometryCollection(vec![inner], Metadata::default());
    let result = ShapeLayer::get_geometry_at_path(&collection, &[0]);
    assert!(matches!(result, Some(Geometry::Point(_, _))));
  }

  #[test]
  fn get_geometry_at_path_out_of_bounds() {
    let collection = Geometry::GeometryCollection(vec![], Metadata::default());
    assert!(ShapeLayer::get_geometry_at_path(&collection, &[0]).is_none());
  }

  #[test]
  fn get_geometry_at_path_non_collection_with_index() {
    let geo = point(Metadata::default());
    assert!(ShapeLayer::get_geometry_at_path(&geo, &[0]).is_none());
  }
}
