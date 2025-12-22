use crate::map::{coordinates::PixelCoordinate, geometry_collection::Geometry};

/// Calculate the distance from a point to a geometry
#[must_use]
pub fn distance_to_geometry(
  geometry: &Geometry<PixelCoordinate>,
  click_coord: PixelCoordinate,
) -> Option<f64> {
  match geometry {
    Geometry::Point(coord, _) => Some(distance_to_point(*coord, click_coord)),
    Geometry::LineString(coords, _) => distance_to_line_string(coords, click_coord),
    Geometry::Polygon(coords, _) => distance_to_polygon(coords, click_coord),
    Geometry::GeometryCollection(geometries, _) => geometries
      .iter()
      .filter_map(|geom| distance_to_geometry(geom, click_coord))
      .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)),
  }
}

/// Calculate distance from a point to a point
fn distance_to_point(point: PixelCoordinate, click_coord: PixelCoordinate) -> f64 {
  let dx = point.x - click_coord.x;
  let dy = point.y - click_coord.y;
  f64::from(dx * dx + dy * dy).sqrt()
}

/// Calculate distance from a point to a line string
fn distance_to_line_string(
  coords: &[PixelCoordinate],
  click_coord: PixelCoordinate,
) -> Option<f64> {
  if coords.is_empty() {
    return None;
  }

  if coords.len() == 1 {
    // If only one point, treat as point
    Some(distance_to_point(coords[0], click_coord))
  } else {
    // Calculate distance to each line segment
    coords
      .windows(2)
      .map(|window| {
        let p1 = window[0];
        let p2 = window[1];
        distance_to_line_segment(click_coord, p1, p2)
      })
      .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
  }
}

/// Calculate distance from a point to a polygon (distance to vertices)
fn distance_to_polygon(coords: &[PixelCoordinate], click_coord: PixelCoordinate) -> Option<f64> {
  if coords.is_empty() {
    return None;
  }

  coords
    .iter()
    .map(|coord| distance_to_point(*coord, click_coord))
    .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

/// Calculate the distance from a point to a line segment
#[must_use]
pub fn distance_to_line_segment(
  point: PixelCoordinate,
  line_start: PixelCoordinate,
  line_end: PixelCoordinate,
) -> f64 {
  let dx = line_end.x - line_start.x;
  let dy = line_end.y - line_start.y;

  // If the line segment is actually a point
  if dx == 0.0 && dy == 0.0 {
    return distance_to_point(line_start, point);
  }

  // Calculate the parameter t for the closest point on the line
  let t = ((point.x - line_start.x) * dx + (point.y - line_start.y) * dy) / (dx * dx + dy * dy);

  // Clamp t to [0, 1] to stay within the line segment
  let t = t.clamp(0.0, 1.0);

  // Find the closest point on the line segment
  let closest_x = line_start.x + t * dx;
  let closest_y = line_start.y + t * dy;

  // Calculate distance from point to closest point on line segment
  let px = point.x - closest_x;
  let py = point.y - closest_y;
  f64::from(px * px + py * py).sqrt()
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
  use super::*;
  use crate::map::geometry_collection::Metadata;
  use rstest::rstest;

  fn pc(x: f32, y: f32) -> PixelCoordinate {
    PixelCoordinate { x, y }
  }

  #[rstest]
  #[case(pc(0.0, 5.0), pc(0.0, 0.0), pc(10.0, 0.0), 5.0)]
  #[case(pc(5.0, 5.0), pc(0.0, 0.0), pc(10.0, 0.0), 5.0)]
  #[case(pc(-5.0, 0.0), pc(0.0, 0.0), pc(10.0, 0.0), 5.0)]
  #[case(pc(15.0, 0.0), pc(0.0, 0.0), pc(10.0, 0.0), 5.0)]
  fn test_distance_to_line_segment(
    #[case] point: PixelCoordinate,
    #[case] start: PixelCoordinate,
    #[case] end: PixelCoordinate,
    #[case] expected: f64,
  ) {
    let dist = distance_to_line_segment(point, start, end);
    assert!(
      (dist - expected).abs() < 0.001,
      "expected {expected}, got {dist}"
    );
  }

  #[test]
  fn test_distance_to_line_segment_degenerate() {
    let point = pc(5.0, 5.0);
    let same = pc(0.0, 0.0);
    let dist = distance_to_line_segment(point, same, same);
    assert!((dist - 7.071).abs() < 0.01);
  }

  #[test]
  fn test_distance_to_geometry_point() {
    let geom = Geometry::Point(pc(10.0, 0.0), Metadata::default());
    let dist = distance_to_geometry(&geom, pc(0.0, 0.0));
    assert_eq!(dist, Some(10.0));
  }

  #[test]
  fn test_distance_to_geometry_linestring() {
    let geom = Geometry::LineString(vec![pc(0.0, 0.0), pc(10.0, 0.0)], Metadata::default());
    let dist = distance_to_geometry(&geom, pc(5.0, 5.0));
    assert_eq!(dist, Some(5.0));
  }

  #[test]
  fn test_distance_to_geometry_linestring_empty() {
    let geom = Geometry::LineString(vec![], Metadata::default());
    assert_eq!(distance_to_geometry(&geom, pc(0.0, 0.0)), None);
  }

  #[test]
  fn test_distance_to_geometry_linestring_single() {
    let geom = Geometry::LineString(vec![pc(10.0, 0.0)], Metadata::default());
    let dist = distance_to_geometry(&geom, pc(0.0, 0.0));
    assert_eq!(dist, Some(10.0));
  }

  #[test]
  fn test_distance_to_geometry_polygon() {
    let geom = Geometry::Polygon(
      vec![pc(0.0, 0.0), pc(10.0, 0.0), pc(10.0, 10.0), pc(0.0, 10.0)],
      Metadata::default(),
    );
    let dist = distance_to_geometry(&geom, pc(5.0, 5.0));
    assert!(dist.is_some());
  }

  #[test]
  fn test_distance_to_geometry_polygon_empty() {
    let geom = Geometry::Polygon(vec![], Metadata::default());
    assert_eq!(distance_to_geometry(&geom, pc(0.0, 0.0)), None);
  }

  #[test]
  fn test_distance_to_geometry_collection() {
    let geom = Geometry::GeometryCollection(
      vec![
        Geometry::Point(pc(100.0, 0.0), Metadata::default()),
        Geometry::Point(pc(10.0, 0.0), Metadata::default()),
      ],
      Metadata::default(),
    );
    let dist = distance_to_geometry(&geom, pc(0.0, 0.0));
    assert_eq!(dist, Some(10.0));
  }

  #[test]
  fn test_distance_to_geometry_collection_empty() {
    let geom = Geometry::GeometryCollection(vec![], Metadata::default());
    assert_eq!(distance_to_geometry(&geom, pc(0.0, 0.0)), None);
  }
}
