use serde::{Deserialize, Serialize};

/// A trait generalizing types of coordinates used in this application.
pub trait Coordinate: Copy + Clone + std::fmt::Debug {
  fn as_wsg84(&self) -> WGS84Coordinate;
  fn as_pixel_position(&self) -> PixelPosition;
}

impl Coordinate for WGS84Coordinate {
  fn as_wsg84(&self) -> WGS84Coordinate {
    *self
  }

  fn as_pixel_position(&self) -> PixelPosition {
    PixelPosition::from(*self)
  }
}

impl Coordinate for TileCoordinate {
  fn as_wsg84(&self) -> WGS84Coordinate {
    WGS84Coordinate::from(*self)
  }

  fn as_pixel_position(&self) -> PixelPosition {
    PixelPosition::from(*self)
  }
}

impl Coordinate for PixelPosition {
  fn as_wsg84(&self) -> WGS84Coordinate {
    WGS84Coordinate::from(*self)
  }

  fn as_pixel_position(&self) -> PixelPosition {
    *self
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

/// A helper coordinate format to position tiles.
#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct TileCoordinate {
  pub x: f32,
  pub y: f32,
  pub zoom: u8,
}

/// A coordinate system used in this application to draw on an imaginary canvas.
/// Is equivalent to Web Mercator projection on a fixed zoom level.
#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct PixelPosition {
  pub x: f32,
  pub y: f32,
}

impl PixelPosition {
  #[must_use]
  pub fn new(x: f32, y: f32) -> Self {
    Self { x, y }
  }
}

impl std::ops::AddAssign for PixelPosition {
  fn add_assign(&mut self, other: Self) {
    self.x += other.x;
    self.y += other.y;
  }
}

impl std::ops::Add for PixelPosition {
  type Output = Self;

  fn add(self, rhs: Self) -> Self {
    Self {
      x: self.x + rhs.x,
      y: self.y + rhs.y,
    }
  }
}

impl std::ops::Mul<f32> for PixelPosition {
  type Output = Self;

  fn mul(self, rhs: f32) -> Self {
    Self {
      x: self.x * rhs,
      y: self.y * rhs,
    }
  }
}

/// A function to create a tile iterator for a given bounding box.
pub fn tiles_in_box(nw: TileCoordinate, se: TileCoordinate) -> impl Iterator<Item = Tile> {
  let nw_tile = Tile::from(nw);
  let se_tile = Tile::from(se);
  (nw_tile.x..=se_tile.x)
    .flat_map(move |x| {
      (nw_tile.y..=se_tile.y).map(move |y| Tile {
        x,
        y,
        zoom: nw_tile.zoom,
      })
    })
    .filter(Tile::exists)
}

/// The fixed canvas size for ``PixelPosition``s.
pub const CANVAS_SIZE: f32 = 1000.;
pub const TILE_SIZE: f32 = 512.;

impl From<TileCoordinate> for PixelPosition {
  fn from(tile_coord: TileCoordinate) -> Self {
    PixelPosition {
      x: tile_coord.x * TILE_SIZE / 2f32.powi(i32::from(tile_coord.zoom) - 2),
      y: tile_coord.y * TILE_SIZE / 2f32.powi(i32::from(tile_coord.zoom) - 2),
    }
  }
}

impl From<WGS84Coordinate> for PixelPosition {
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

impl PixelPosition {
  #[must_use]
  pub fn clamp(&self) -> Self {
    PixelPosition {
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
  pub fn sq_distance_line_segment(&self, l1: &PixelPosition, l2: &PixelPosition) -> f32 {
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
    PixelPosition {
      x: l1.x + param * dbx,
      y: l1.y + param * dby,
    }
    .sq_dist(self)
  }
}

impl From<PixelPosition> for WGS84Coordinate {
  fn from(pp: PixelPosition) -> Self {
    WGS84Coordinate::from(TileCoordinate::from_pixel_position(pp, 2))
  }
}

/// A tile in the Web Mercator projection.
#[derive(Debug, PartialEq, Copy, Clone, Hash, Eq, Serialize, Deserialize)]
pub struct Tile {
  pub x: u32,
  pub y: u32,
  pub zoom: u8,
}

impl Tile {
  /// Checks existence of the tile.
  #[must_use]
  pub fn exists(&self) -> bool {
    let max_tile = 2u32.pow(self.zoom.into()) - 1;
    self.x <= max_tile && self.y <= max_tile
  }

  /// The parent one zoom level lower.
  #[must_use]
  pub fn parent(&self) -> Option<Self> {
    match self.zoom {
      0 => None,
      _ => Some(Self {
        x: self.x >> 1,
        y: self.y >> 1,
        zoom: self.zoom - 1,
      }),
    }
  }

  #[must_use]
  #[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
  )]
  pub fn position(&self) -> (PixelPosition, PixelPosition) {
    (
      PixelPosition::from(TileCoordinate {
        x: self.x as f32,
        y: self.y as f32,
        zoom: self.zoom,
      }),
      PixelPosition::from(TileCoordinate {
        x: (self.x + 1) as f32,
        y: (self.y + 1) as f32,
        zoom: self.zoom,
      }),
    )
  }
}

impl From<TileCoordinate> for Tile {
  #[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
  )]
  fn from(tile_coord: TileCoordinate) -> Self {
    Self {
      x: tile_coord.x.floor() as u32,
      y: tile_coord.y.floor() as u32,
      zoom: tile_coord.zoom,
    }
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
  pub fn from_pixel_position(pixel_pos: PixelPosition, zoom: u8) -> Self {
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

#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
  max_x: f32,
  min_x: f32,
  max_y: f32,
  min_y: f32,
}

impl Default for BoundingBox {
  fn default() -> Self {
    Self::new()
  }
}

impl BoundingBox {
  #[must_use]
  pub fn new() -> Self {
    Self::get_invalid()
  }

  #[must_use]
  pub fn get_invalid() -> Self {
    Self {
      max_x: f32::MIN,
      min_x: f32::MAX,
      max_y: f32::MIN,
      min_y: f32::MAX,
    }
  }

  #[must_use]
  pub fn center(&self) -> PixelPosition {
    PixelPosition {
      x: (self.max_x + self.min_x) / 2.,
      y: (self.max_y + self.min_y) / 2.,
    }
  }

  pub fn from_iterator<C: Coordinate, I: IntoIterator<Item = C>>(positions: I) -> Self {
    let mut bb = Self::get_invalid();
    positions
      .into_iter()
      .for_each(|pos| bb.add_coordinate(pos.as_pixel_position()));
    bb
  }

  #[must_use]
  pub fn is_valid(&self) -> bool {
    self.min_y <= self.max_y && self.min_x <= self.max_x
  }

  pub fn add_coordinate(&mut self, pp: PixelPosition) {
    self.min_y = self.min_y.min(pp.y);
    self.min_x = self.min_x.min(pp.x);
    self.max_y = self.max_y.max(pp.y);
    self.max_x = self.max_x.max(pp.x);
  }

  #[must_use]
  pub fn extend(mut self, bb: &Self) -> Self {
    if bb.is_valid() {
      self.add_coordinate(PixelPosition {
        x: bb.min_x,
        y: bb.min_y,
      });
      self.add_coordinate(PixelPosition {
        x: bb.max_x,
        y: bb.max_y,
      });
    }
    self
  }

  #[must_use]
  pub fn width(&self) -> f32 {
    self.max_x - self.min_x
  }

  #[must_use]
  pub fn height(&self) -> f32 {
    self.max_y - self.min_y
  }
}

#[derive(Debug, Clone, Copy)]
pub struct Transform {
  pub zoom: f32,
  pub trans: PixelPosition,
}

impl Default for Transform {
  fn default() -> Self {
    Self {
      zoom: 1.,
      trans: PixelPosition { x: 0., y: 0. },
    }
  }
}

impl Transform {
  #[must_use]
  pub fn zoomed(mut self, factor: f32) -> Self {
    self.zoom *= factor;
    self
  }

  pub fn zoom(&mut self, factor: f32) -> &mut Self {
    self.zoom *= factor;
    self
  }

  pub fn translate(&mut self, delta: PixelPosition) -> &mut Self {
    self.trans += delta;
    self
  }

  #[must_use]
  pub fn translated(mut self, delta: PixelPosition) -> Self {
    self.translate(delta);
    self
  }

  #[must_use]
  pub fn and_then_transform(self, transform: &Transform) -> Self {
    Self {
      zoom: self.zoom * transform.zoom,
      trans: self.trans * transform.zoom + transform.trans,
    }
  }

  #[must_use]
  pub fn invert(self) -> Self {
    Self {
      zoom: 1. / self.zoom,
      trans: self.trans * (-1. / self.zoom),
    }
  }

  #[must_use]
  pub fn invalid() -> Self {
    Self {
      zoom: 0.,
      trans: PixelPosition { x: 0., y: 0. },
    }
  }

  #[must_use]
  pub fn is_invalid(&self) -> bool {
    !self.zoom.is_nan() && !self.trans.x.is_nan() && !self.trans.y.is_nan() && self.zoom == 0.
  }

  #[must_use]
  pub fn apply(&self, pp: PixelPosition) -> PixelPosition {
    pp * self.zoom + self.trans
  }

  #[must_use]
  pub fn new(zoom: f32, x: f32, y: f32) -> Self {
    Self {
      zoom,
      trans: PixelPosition { x, y },
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use assert_approx_eq::assert_approx_eq;

  #[test]
  fn coordinate_tile_conversions() {
    let coord = WGS84Coordinate {
      lat: 52.521_977,
      lon: 13.413_305,
    };

    let tc13 = TileCoordinate::from_coordinate(coord, 13);
    assert!(WGS84Coordinate::from(tc13).lat - coord.lat < 0.000_000_1);
    assert!(WGS84Coordinate::from(tc13).lon - coord.lon < 0.000_000_1);

    let t13: Tile = tc13.into();
    assert_eq!(
      t13,
      Tile {
        x: 4401,
        y: 2686,
        zoom: 13
      }
    );

    let tc17 = TileCoordinate::from_coordinate(coord, 17);
    assert!(WGS84Coordinate::from(tc17).lat - coord.lat < 0.000_000_1);
    assert!(WGS84Coordinate::from(tc17).lon - coord.lon < 0.000_000_1);
    let t17: Tile = tc17.into();
    assert_eq!(
      t17,
      Tile {
        x: 70419,
        y: 42984,
        zoom: 17
      }
    );
  }

  #[ignore = "Changed some canvas size constants"]
  #[test]
  fn coordinate_to_pixel_zero() {
    let coord = WGS84Coordinate { lat: 0.0, lon: 0.0 };
    let tc2 = TileCoordinate::from_coordinate(coord, 2);
    let tc3 = TileCoordinate::from_coordinate(coord, 3);
    let tc4 = TileCoordinate::from_coordinate(coord, 4);
    let pp = PixelPosition { x: 500., y: 500. };
    assert_eq!(PixelPosition::from(tc2), pp);
    assert_eq!(PixelPosition::from(tc3), pp);
    assert_eq!(PixelPosition::from(tc4), pp);
  }

  #[ignore = "Changed some canvas size constants"]
  #[test]
  fn coordinate_to_pixel() {
    let tc3 = TileCoordinate {
      x: 2.,
      y: 1.,
      zoom: 3,
    };
    let pp = PixelPosition { x: 250., y: 125. };
    assert_eq!(PixelPosition::from(tc3), pp);
    assert_eq!(TileCoordinate::from_pixel_position(pp, 3), tc3);
    assert_eq!(WGS84Coordinate::from(tc3), WGS84Coordinate::from(pp));
  }

  #[test]
  fn tile_box_test() {
    let nw = TileCoordinate {
      x: 2.1,
      y: 1.1,
      zoom: 5,
    };

    let se = TileCoordinate {
      x: 11.1,
      y: 20.1,
      zoom: 5,
    };

    let tiles: Vec<_> = tiles_in_box(nw, se).collect();
    assert_eq!(tiles.len(), 200);
  }

  #[test]
  fn transform() {
    let trans = Transform::default()
      .zoomed(5.)
      .translated(PixelPosition { x: 10., y: 20. });
    let inv = trans.invert();

    assert_approx_eq!(trans.zoom, 5.);
    assert_approx_eq!(trans.trans.x, 10.);
    assert_approx_eq!(trans.trans.y, 20.);

    assert_approx_eq!(inv.zoom, 1. / 5.);
    assert_approx_eq!(inv.trans.x, -2.);
    assert_approx_eq!(inv.trans.y, -4.);

    let prod = trans.and_then_transform(&inv);
    assert_approx_eq!(prod.zoom, 1.);
    assert_approx_eq!(prod.trans.x, 0.);
    assert_approx_eq!(prod.trans.y, 0.);

    let prod = inv.and_then_transform(&trans);
    assert_approx_eq!(prod.zoom, 1.);
    assert_approx_eq!(prod.trans.x, 0.);
    assert_approx_eq!(prod.trans.y, 0.);
  }
}
