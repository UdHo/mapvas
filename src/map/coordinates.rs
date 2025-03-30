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
