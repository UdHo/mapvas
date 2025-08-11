use crate::map::{
  coordinates::{PixelCoordinate, Transform},
  geometry_collection::Geometry,
};
use egui::Pos2;

/// Find the closest geometry to a click position within a geometry (recursively for collections)
pub fn find_closest_in_geometry<F1, F2>(
  layer_id: &str,
  shape_idx: usize,
  nested_path: &[usize],
  geometry: &Geometry<PixelCoordinate>,
  click_pos: Pos2,
  transform: &Transform,
  closest_distance: &mut f64,
  closest_geometry: &mut Option<(String, usize, Vec<usize>)>,
  // Visibility and temporal filtering callbacks
  is_nested_visible: F1,
  is_temporal_visible: F2,
) where
  F1: Fn(&str, usize, &[usize]) -> bool + Copy,
  F2: Fn(&Geometry<PixelCoordinate>) -> bool + Copy,
{
  let click_pixel = transform.invert().apply(click_pos.into());

  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      // Recursively check each nested geometry
      for (nested_idx, nested_geometry) in geometries.iter().enumerate() {
        let mut new_path = nested_path.to_vec();
        new_path.push(nested_idx);

        // Check if this nested geometry is visible
        if !is_nested_visible(layer_id, shape_idx, &new_path) {
          continue;
        }

        // Apply temporal filtering to nested geometry
        if !is_temporal_visible(nested_geometry) {
          continue;
        }

        find_closest_in_geometry(
          layer_id,
          shape_idx,
          &new_path,
          nested_geometry,
          click_pos,
          transform,
          closest_distance,
          closest_geometry,
          is_nested_visible,
          is_temporal_visible,
        );
      }
    }
    _ => {
      // For individual geometries, calculate distance in map coordinates, then convert to screen pixels
      let distance_map_coords = calculate_distance_to_geometry(geometry, &click_pixel);
      let distance_screen_pixels = distance_map_coords * f64::from(transform.zoom);

      if distance_screen_pixels < *closest_distance {
        *closest_distance = distance_screen_pixels;
        *closest_geometry = Some((layer_id.to_string(), shape_idx, nested_path.to_vec()));
      }
    }
  }
}

/// Calculate distance from a point to any type of geometry
fn calculate_distance_to_geometry(
  geometry: &Geometry<PixelCoordinate>,
  click_pixel: &PixelCoordinate,
) -> f64 {
  match geometry {
    Geometry::Point(coord, _) => {
      let dx = click_pixel.x - coord.x;
      let dy = click_pixel.y - coord.y;
      (dx * dx + dy * dy).sqrt() as f64
    }
    Geometry::LineString(coords, _) => distance_to_linestring(coords, click_pixel),
    Geometry::Polygon(coords, _) => distance_to_polygon(coords, click_pixel),
    Geometry::GeometryCollection(_, _) => {
      // Should not reach here as collections are handled recursively above
      f64::INFINITY
    }
  }
}

/// Calculate distance from a point to a linestring
fn distance_to_linestring(coords: &[PixelCoordinate], click_pixel: &PixelCoordinate) -> f64 {
  if coords.len() < 2 {
    return f64::INFINITY;
  }

  let mut min_distance = f64::INFINITY;
  for segment in coords.windows(2) {
    let distance = point_to_line_segment_distance(click_pixel, &segment[0], &segment[1]);
    min_distance = min_distance.min(distance);
  }
  min_distance
}

/// Calculate distance from a point to a polygon (outline)
fn distance_to_polygon(coords: &[PixelCoordinate], click_pixel: &PixelCoordinate) -> f64 {
  if coords.len() < 3 {
    return f64::INFINITY;
  }

  let mut min_distance = f64::INFINITY;

  // Check distance to each edge of the polygon
  for i in 0..coords.len() {
    let start = &coords[i];
    let end = &coords[(i + 1) % coords.len()];
    let distance = point_to_line_segment_distance(click_pixel, start, end);
    min_distance = min_distance.min(distance);
  }

  min_distance
}

/// Calculate the shortest distance from a point to a line segment
fn point_to_line_segment_distance(
  point: &PixelCoordinate,
  line_start: &PixelCoordinate,
  line_end: &PixelCoordinate,
) -> f64 {
  let dx = line_end.x - line_start.x;
  let dy = line_end.y - line_start.y;

  if dx == 0.0 && dy == 0.0 {
    // Line segment is actually a point
    let dx = point.x - line_start.x;
    let dy = point.y - line_start.y;
    return (dx * dx + dy * dy).sqrt() as f64;
  }

  let t = ((point.x - line_start.x) * dx + (point.y - line_start.y) * dy) / (dx * dx + dy * dy);
  let t = t.clamp(0.0, 1.0);

  let closest_x = line_start.x + t * dx;
  let closest_y = line_start.y + t * dy;

  let dx = point.x - closest_x;
  let dy = point.y - closest_y;
  (dx * dx + dy * dy).sqrt() as f64
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_point_to_line_segment_distance() {
    let point = PixelCoordinate { x: 1.0, y: 1.0 };
    let line_start = PixelCoordinate { x: 0.0, y: 0.0 };
    let line_end = PixelCoordinate { x: 2.0, y: 0.0 };

    let distance = point_to_line_segment_distance(&point, &line_start, &line_end);
    assert!((distance - 1.0).abs() < 1e-10); // Should be 1.0
  }

  #[test]
  fn test_distance_to_linestring() {
    let coords = vec![
      PixelCoordinate { x: 0.0, y: 0.0 },
      PixelCoordinate { x: 10.0, y: 0.0 },
      PixelCoordinate { x: 10.0, y: 10.0 },
    ];
    let click_pixel = PixelCoordinate { x: 5.0, y: 1.0 };

    let distance = distance_to_linestring(&coords, &click_pixel);
    assert!((distance - 1.0).abs() < 1e-10); // Should be 1.0
  }
}
