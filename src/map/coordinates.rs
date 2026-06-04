mod boxes;
mod coords;
mod transform;

/// Tiles and bounding boxes.
pub use boxes::*;
/// Coordinates.
pub use coords::*;
/// Transforms.
use transform::TTransform;

/// Keeps track of the transform of the map.
pub type Transform = TTransform<PixelCoordinate, PixelPosition>;

#[must_use]
pub fn tile_zoom_for_transform(transform: &Transform) -> u8 {
  let zoom = transform.zoom.max(1.0).log2().floor() as i32 + 2;
  zoom.clamp(0, i32::from(u8::MAX)) as u8
}

#[must_use]
pub fn transform_zoom_for_tile_zoom(tile_zoom: u8) -> f32 {
  2f32.powi(i32::from(tile_zoom) - 2)
}

/// A trait generalizing types of coordinates used in this application.
pub trait Coordinate: Copy + Clone + std::fmt::Debug {
  fn as_wgs84(&self) -> WGS84Coordinate;
  fn as_pixel_coordinate(&self) -> PixelCoordinate;
}

impl Coordinate for WGS84Coordinate {
  fn as_wgs84(&self) -> WGS84Coordinate {
    *self
  }

  fn as_pixel_coordinate(&self) -> PixelCoordinate {
    PixelCoordinate::from(*self)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tile_zoom_depends_on_map_scale_not_viewport_size() {
    let mut transform = Transform::default();

    transform.zoom = 1.0;
    assert_eq!(tile_zoom_for_transform(&transform), 2);

    transform.zoom = 1.99;
    assert_eq!(tile_zoom_for_transform(&transform), 2);

    transform.zoom = 2.0;
    assert_eq!(tile_zoom_for_transform(&transform), 3);
  }

  #[test]
  fn transform_zoom_matches_tile_zoom() {
    for tile_zoom in [2, 3, 10, 19, 21] {
      let mut transform = Transform::default();
      transform.zoom = transform_zoom_for_tile_zoom(tile_zoom);
      assert_eq!(tile_zoom_for_transform(&transform), tile_zoom);
    }
  }
}
