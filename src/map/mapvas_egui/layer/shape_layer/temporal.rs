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
