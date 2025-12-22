use std::ops::{Add, AddAssign, Mul};

use serde::{Deserialize, Serialize};

use super::Coordinate;

/// The fixed canvas size for ``PixelPosition``s.
const CANVAS_SIZE: f32 = 1024. * 2.;
pub const TILE_SIZE: f32 = 512.;

pub trait XY:
  Default + Copy + Clone + AddAssign<Self> + Mul<f32, Output = Self> + Add<Self, Output = Self>
{
  fn x(&self) -> f32;
  fn y(&self) -> f32;
  #[must_use]
  fn with_x(self, x: f32) -> Self;
  #[must_use]
  fn with_y(self, y: f32) -> Self;
}

#[must_use]
pub fn distance_in_meters(coord1: WGS84Coordinate, coord2: WGS84Coordinate) -> f32 {
  let d_lat = (coord2.lat - coord1.lat).to_radians();
  let d_lon = (coord2.lon - coord1.lon).to_radians();
  let a = f32::sin(d_lat / 2.0) * f32::sin(d_lat / 2.0)
    + f32::cos(coord1.lat.to_radians())
      * f32::cos(coord2.lat.to_radians())
      * f32::sin(d_lon / 2.0)
      * f32::sin(d_lon / 2.0);
  let c = 2.0 * f32::atan2(a.sqrt(), (1.0 - a).sqrt());
  6_371_000.0 * c
}

impl Coordinate for TileCoordinate {
  fn as_wgs84(&self) -> WGS84Coordinate {
    WGS84Coordinate::from(*self)
  }

  fn as_pixel_coordinate(&self) -> PixelCoordinate {
    PixelCoordinate::from(*self)
  }
}

impl Coordinate for PixelCoordinate {
  fn as_wgs84(&self) -> WGS84Coordinate {
    WGS84Coordinate::from(*self)
  }

  fn as_pixel_coordinate(&self) -> PixelCoordinate {
    *self
  }
}

impl From<TileCoordinate> for PixelCoordinate {
  fn from(tile_coord: TileCoordinate) -> Self {
    PixelCoordinate {
      x: tile_coord.x * TILE_SIZE / 2f32.powi(i32::from(tile_coord.zoom) - 2),
      y: tile_coord.y * TILE_SIZE / 2f32.powi(i32::from(tile_coord.zoom) - 2),
    }
  }
}

impl From<WGS84Coordinate> for PixelCoordinate {
  fn from(coord: WGS84Coordinate) -> Self {
    TileCoordinate::from_coordinate(coord, 2).into()
  }
}

impl From<egui::Pos2> for PixelPosition {
  fn from(pos: egui::Pos2) -> Self {
    PixelPosition { x: pos.x, y: pos.y }
  }
}

impl From<PixelPosition> for egui::Pos2 {
  fn from(pp: PixelPosition) -> Self {
    egui::Pos2::new(pp.x, pp.y)
  }
}

impl PixelCoordinate {
  #[must_use]
  pub fn clamp(&self) -> Self {
    PixelCoordinate {
      x: self.x.clamp(0.0, CANVAS_SIZE),
      y: self.y.clamp(0.0, CANVAS_SIZE),
    }
  }

  #[must_use]
  pub fn sq_dist(&self, p: &Self) -> f32 {
    let dx = p.x - self.x;
    let dy = p.y - self.y;
    dx * dx + dy * dy
  }

  #[must_use]
  pub fn sq_distance_line_segment(&self, l1: &PixelCoordinate, l2: &PixelCoordinate) -> f32 {
    let dbx = l2.x - l1.x;
    let dby = l2.y - l1.y;
    let dpx = self.x - l1.x;
    let dpy = self.y - l1.y;
    let dot = dbx * dpx + dby * dpy;
    let len_sq = dbx * dbx + dby * dby;

    if len_sq < 0.000_000_1 {
      return self.sq_dist(l1);
    }
    let param = (dot / len_sq).clamp(0., 1.);
    PixelCoordinate {
      x: l1.x + param * dbx,
      y: l1.y + param * dby,
    }
    .sq_dist(self)
  }
}

impl From<PixelCoordinate> for WGS84Coordinate {
  fn from(pp: PixelCoordinate) -> Self {
    WGS84Coordinate::from(TileCoordinate::from_pixel_position(pp, 2))
  }
}

impl TileCoordinate {
  #[must_use]
  pub fn from_coordinate(coord: WGS84Coordinate, zoom: u8) -> Self {
    let x = (coord.lon + 180.) / 360. * 2f32.powi(zoom.into());
    let y = (1. - ((coord.lat * PI / 180.).tan() + 1. / (coord.lat * PI / 180.).cos()).ln() / PI)
      * 2f32.powi((zoom - 1).into());
    Self { x, y, zoom }
  }

  #[must_use]
  pub fn from_pixel_position(pixel_pos: PixelCoordinate, zoom: u8) -> Self {
    TileCoordinate {
      x: pixel_pos.x / TILE_SIZE * 2f32.powi(i32::from(zoom) - 2),
      y: pixel_pos.y / TILE_SIZE * 2f32.powi(i32::from(zoom) - 2),
      zoom,
    }
  }
}

const PI: f32 = std::f32::consts::PI;
impl From<TileCoordinate> for WGS84Coordinate {
  fn from(tile_coord: TileCoordinate) -> Self {
    WGS84Coordinate {
      lat: f32::atan(f32::sinh(
        PI - tile_coord.y / 2f32.powi(tile_coord.zoom.into()) * 2. * PI,
      )) * 180.
        / PI,
      lon: tile_coord.x / 2f32.powi(tile_coord.zoom.into()) * 360. - 180.,
    }
  }
}

impl XY for PixelCoordinate {
  fn x(&self) -> f32 {
    self.as_pixel_coordinate().x
  }

  fn y(&self) -> f32 {
    self.as_pixel_coordinate().y
  }

  fn with_x(mut self, x: f32) -> Self {
    self.x = x;
    self
  }

  fn with_y(mut self, y: f32) -> Self {
    self.y = y;
    self
  }
}

impl XY for PixelPosition {
  fn x(&self) -> f32 {
    self.x
  }

  fn y(&self) -> f32 {
    self.y
  }

  fn with_x(mut self, x: f32) -> Self {
    self.x = x;
    self
  }

  fn with_y(mut self, y: f32) -> Self {
    self.y = y;
    self
  }
}

/// A helper coordinate format to position tiles.
#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct TileCoordinate {
  pub x: f32,
  pub y: f32,
  pub zoom: u8,
}

impl Eq for TileCoordinate {}

impl TileCoordinate {
  /// Exact equality comparison using bit representation
  #[must_use]
  pub fn exact_eq(&self, other: &Self) -> bool {
    self.x.to_bits() == other.x.to_bits()
      && self.y.to_bits() == other.y.to_bits()
      && self.zoom == other.zoom
  }
}

/// A coordinate system used in this application to draw on an imaginary canvas.
/// Is equivalent to Web Mercator projection on a fixed zoom level.
#[derive(Debug, Default, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct PixelCoordinate {
  pub x: f32,
  pub y: f32,
}

impl Eq for PixelCoordinate {}

impl PixelCoordinate {
  /// Exact equality comparison using bit representation
  #[must_use]
  pub fn exact_eq(&self, other: &Self) -> bool {
    self.x.to_bits() == other.x.to_bits() && self.y.to_bits() == other.y.to_bits()
  }
  #[must_use]
  pub fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }

  #[must_use]
  pub fn invalid() -> Self {
    Self { x: -1., y: -1. }
  }

  #[must_use]
  pub fn is_valid(&self) -> bool {
    self.x >= 0. && self.y >= 0. && self.x <= 2. * CANVAS_SIZE && self.y <= 2. * CANVAS_SIZE
  }
}

impl std::ops::AddAssign for PixelCoordinate {
  fn add_assign(&mut self, other: Self) {
    self.x += other.x;
    self.y += other.y;
  }
}

impl std::ops::Add for PixelCoordinate {
  type Output = Self;

  fn add(self, rhs: Self) -> Self {
    Self {
      x: self.x + rhs.x,
      y: self.y + rhs.y,
    }
  }
}

impl std::ops::Mul<f32> for PixelCoordinate {
  type Output = Self;

  fn mul(self, rhs: f32) -> Self {
    Self {
      x: self.x * rhs,
      y: self.y * rhs,
    }
  }
}

/// The standard WGS84 coordinate system.
#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct WGS84Coordinate {
  #[serde(alias = "latitude")]
  pub lat: f32,
  #[serde(alias = "longitude")]
  pub lon: f32,
}

impl WGS84Coordinate {
  #[must_use]
  pub fn new(lat: f32, lon: f32) -> Self {
    Self { lat, lon }
  }

  #[must_use]
  pub fn is_valid(&self) -> bool {
    -90.0 < self.lat && self.lat < 90.0 && -180.0 < self.lon && self.lon < 180.0
  }
}

impl Eq for WGS84Coordinate {}

impl WGS84Coordinate {
  /// Exact equality comparison using bit representation
  #[must_use]
  pub fn exact_eq(&self, other: &Self) -> bool {
    self.lat.to_bits() == other.lat.to_bits() && self.lon.to_bits() == other.lon.to_bits()
  }
}

/// Meant for actual pixel in the UI. Handled equivalently to a ``egui::Pos2``.
#[derive(Debug, Default, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct PixelPosition {
  pub x: f32,
  pub y: f32,
}

impl Eq for PixelPosition {}

impl PixelPosition {
  /// Exact equality comparison using bit representation
  #[must_use]
  pub fn exact_eq(&self, other: &Self) -> bool {
    self.x.to_bits() == other.x.to_bits() && self.y.to_bits() == other.y.to_bits()
  }
}

impl Mul<f32> for PixelPosition {
  type Output = Self;

  fn mul(self, rhs: f32) -> Self {
    Self {
      x: self.x * rhs,
      y: self.y * rhs,
    }
  }
}

impl Add<PixelPosition> for PixelPosition {
  type Output = Self;

  fn add(self, rhs: PixelPosition) -> Self {
    Self {
      x: self.x + rhs.x,
      y: self.y + rhs.y,
    }
  }
}

impl AddAssign for PixelPosition {
  fn add_assign(&mut self, other: Self) {
    self.x += other.x;
    self.y += other.y;
  }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
  use super::*;
  use assert_approx_eq::assert_approx_eq;

  #[test]
  fn coordinate_to_pixel() {
    let tc3 = TileCoordinate {
      x: 2.,
      y: 1.,
      zoom: 3,
    };
    let pp = PixelCoordinate { x: 512., y: 256. };
    assert_eq!(PixelCoordinate::from(tc3), pp);
    assert_eq!(TileCoordinate::from_pixel_position(pp, 3), tc3);
    assert_eq!(WGS84Coordinate::from(tc3), WGS84Coordinate::from(pp));
  }

  #[test]
  fn coordinate_to_pixel_zero() {
    let coord = WGS84Coordinate { lat: 0.0, lon: 0.0 };
    let tc2 = TileCoordinate::from_coordinate(coord, 2);
    let tc3 = TileCoordinate::from_coordinate(coord, 3);
    let tc4 = TileCoordinate::from_coordinate(coord, 4);
    let pp = PixelCoordinate { x: 1024., y: 1024. };
    assert_eq!(PixelCoordinate::from(tc2), pp);
    assert_eq!(PixelCoordinate::from(tc3), pp);
    assert_eq!(PixelCoordinate::from(tc4), pp);
  }

  #[test]
  fn distance() {
    let coord1 = WGS84Coordinate { lat: 0.0, lon: 0.0 };
    let coord2 = WGS84Coordinate { lat: 0.0, lon: 1.0 };
    assert_approx_eq!(distance_in_meters(coord1, coord2), 111_195.08, 0.2);

    let alexanderplatz = WGS84Coordinate {
      lat: 52.520_754,
      lon: 13.409_496,
    };
    let hamburg_hbf = WGS84Coordinate {
      lat: 53.552_7,
      lon: 10.006_6,
    };
    assert_approx_eq!(
      distance_in_meters(alexanderplatz, hamburg_hbf),
      254_785.,
      1.
    );
  }

  #[test]
  fn test_pixel_coordinate_new_and_validity() {
    let valid = PixelCoordinate::new(100.0, 200.0);
    assert!(valid.is_valid());

    let invalid = PixelCoordinate::invalid();
    assert!(!invalid.is_valid());

    let out_of_bounds = PixelCoordinate::new(-10.0, 100.0);
    assert!(!out_of_bounds.is_valid());
  }

  #[test]
  fn test_pixel_coordinate_clamp() {
    let coord = PixelCoordinate::new(-10.0, 3000.0);
    let clamped = coord.clamp();
    assert_eq!(clamped.x, 0.0);
    assert_eq!(clamped.y, CANVAS_SIZE);
  }

  #[test]
  fn test_pixel_coordinate_sq_dist() {
    let p1 = PixelCoordinate::new(0.0, 0.0);
    let p2 = PixelCoordinate::new(3.0, 4.0);
    assert_eq!(p1.sq_dist(&p2), 25.0);
  }

  #[test]
  fn test_pixel_coordinate_sq_distance_line_segment() {
    let point = PixelCoordinate::new(5.0, 5.0);
    let l1 = PixelCoordinate::new(0.0, 0.0);
    let l2 = PixelCoordinate::new(10.0, 0.0);
    assert_eq!(point.sq_distance_line_segment(&l1, &l2), 25.0);

    // Degenerate line (point)
    let same = PixelCoordinate::new(0.0, 0.0);
    let dist = point.sq_distance_line_segment(&same, &same);
    assert_eq!(dist, 50.0);
  }

  #[test]
  fn test_pixel_coordinate_exact_eq() {
    let p1 = PixelCoordinate::new(1.0, 2.0);
    let p2 = PixelCoordinate::new(1.0, 2.0);
    let p3 = PixelCoordinate::new(1.0, 2.1);
    assert!(p1.exact_eq(&p2));
    assert!(!p1.exact_eq(&p3));
  }

  #[test]
  fn test_pixel_coordinate_ops() {
    let mut p1 = PixelCoordinate::new(1.0, 2.0);
    let p2 = PixelCoordinate::new(3.0, 4.0);

    let sum = p1 + p2;
    assert_eq!(sum.x, 4.0);
    assert_eq!(sum.y, 6.0);

    p1 += p2;
    assert_eq!(p1.x, 4.0);
    assert_eq!(p1.y, 6.0);

    let scaled = p2 * 2.0;
    assert_eq!(scaled.x, 6.0);
    assert_eq!(scaled.y, 8.0);
  }

  #[test]
  fn test_wgs84_coordinate_validity() {
    let valid = WGS84Coordinate::new(52.5, 13.4);
    assert!(valid.is_valid());

    let invalid_lat = WGS84Coordinate::new(95.0, 13.4);
    assert!(!invalid_lat.is_valid());

    let invalid_lon = WGS84Coordinate::new(52.5, 200.0);
    assert!(!invalid_lon.is_valid());
  }

  #[test]
  fn test_wgs84_coordinate_exact_eq() {
    let c1 = WGS84Coordinate::new(52.5, 13.4);
    let c2 = WGS84Coordinate::new(52.5, 13.4);
    let c3 = WGS84Coordinate::new(52.5, 13.5);
    assert!(c1.exact_eq(&c2));
    assert!(!c1.exact_eq(&c3));
  }

  #[test]
  fn test_tile_coordinate_exact_eq() {
    let t1 = TileCoordinate {
      x: 1.0,
      y: 2.0,
      zoom: 3,
    };
    let t2 = TileCoordinate {
      x: 1.0,
      y: 2.0,
      zoom: 3,
    };
    let t3 = TileCoordinate {
      x: 1.0,
      y: 2.0,
      zoom: 4,
    };
    assert!(t1.exact_eq(&t2));
    assert!(!t1.exact_eq(&t3));
  }

  #[test]
  fn test_pixel_position_ops() {
    let mut p1 = PixelPosition { x: 1.0, y: 2.0 };
    let p2 = PixelPosition { x: 3.0, y: 4.0 };

    let sum = p1 + p2;
    assert_eq!(sum.x, 4.0);
    assert_eq!(sum.y, 6.0);

    p1 += p2;
    assert_eq!(p1.x, 4.0);

    let scaled = p2 * 2.0;
    assert_eq!(scaled.x, 6.0);
  }

  #[test]
  fn test_pixel_position_exact_eq() {
    let p1 = PixelPosition { x: 1.0, y: 2.0 };
    let p2 = PixelPosition { x: 1.0, y: 2.0 };
    assert!(p1.exact_eq(&p2));
  }

  #[test]
  fn test_pixel_position_from_egui() {
    let pos = egui::Pos2::new(10.0, 20.0);
    let pp = PixelPosition::from(pos);
    assert_eq!(pp.x, 10.0);
    assert_eq!(pp.y, 20.0);

    let back: egui::Pos2 = pp.into();
    assert_eq!(back.x, 10.0);
    assert_eq!(back.y, 20.0);
  }

  #[test]
  fn test_xy_trait() {
    let pc = PixelCoordinate::new(5.0, 10.0);
    assert_eq!(pc.x(), 5.0);
    assert_eq!(pc.y(), 10.0);
    assert_eq!(pc.with_x(7.0).x, 7.0);
    assert_eq!(pc.with_y(12.0).y, 12.0);

    let pp = PixelPosition { x: 5.0, y: 10.0 };
    assert_eq!(pp.x(), 5.0);
    assert_eq!(pp.y(), 10.0);
    assert_eq!(pp.with_x(7.0).x, 7.0);
    assert_eq!(pp.with_y(12.0).y, 12.0);
  }

  #[test]
  fn test_coordinate_trait() {
    let wgs = WGS84Coordinate::new(52.5, 13.4);
    assert_eq!(wgs.as_wgs84(), wgs);
    assert!(wgs.as_pixel_coordinate().is_valid());

    let tile = TileCoordinate {
      x: 1.0,
      y: 1.0,
      zoom: 2,
    };
    assert!(tile.as_wgs84().is_valid());
    assert!(tile.as_pixel_coordinate().is_valid());

    let pixel = PixelCoordinate::new(100.0, 100.0);
    assert!(pixel.as_wgs84().is_valid());
    assert_eq!(pixel.as_pixel_coordinate(), pixel);
  }

  #[test]
  fn test_wgs84_to_pixel_roundtrip() {
    let wgs = WGS84Coordinate::new(52.5, 13.4);
    let pixel = PixelCoordinate::from(wgs);
    let back = WGS84Coordinate::from(pixel);
    assert_approx_eq!(back.lat, wgs.lat, 0.01);
    assert_approx_eq!(back.lon, wgs.lon, 0.01);
  }
}
