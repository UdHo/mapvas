use serde::{Deserialize, Serialize};

/// A standard WGS-84 coordinates.
#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct Coordinate {
  #[serde(alias = "latitude")]
  pub lat: f32,
  #[serde(alias = "longitude")]
  pub lon: f32,
}

impl Coordinate {
  pub fn is_valid(&self) -> bool {
    -90.0 < self.lat && self.lat < 90.0 && -180.0 < self.lon && self.lon < 180.0
  }
}

/// The pixel on a sperical mercator tile with
#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct TileCoordinate {
  pub x: f32,
  pub y: f32,
  pub zoom: u8,
}

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct PixelPosition {
  pub x: f32,
  pub y: f32,
}

pub fn tiles_in_box(nw: TileCoordinate, se: TileCoordinate) -> impl Iterator<Item = Tile> {
  assert_eq!(nw.zoom, se.zoom);
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
    .filter(|t| t.exists())
}

pub const CANVAS_SIZE: f32 = 1000.;
pub const TILE_SIZE: f32 = 250.;

impl From<TileCoordinate> for PixelPosition {
  fn from(tile_coord: TileCoordinate) -> Self {
    PixelPosition {
      x: tile_coord.x * TILE_SIZE / 2f32.powi(tile_coord.zoom as i32 - 2),
      y: tile_coord.y * TILE_SIZE / 2f32.powi(tile_coord.zoom as i32 - 2),
    }
  }
}
impl From<Coordinate> for PixelPosition {
  fn from(coord: Coordinate) -> Self {
    TileCoordinate::from_coordinate(coord, 2).into()
  }
}

impl PixelPosition {
  pub fn clamp(&self) -> Self {
    PixelPosition {
      x: self.x.clamp(0.0, CANVAS_SIZE),
      y: self.y.clamp(0.0, CANVAS_SIZE),
    }
  }

  pub fn sq_dist(&self, p: &Self) -> f32 {
    let dx = p.x - self.x;
    let dy = p.y - self.y;
    dx * dx + dy * dy
  }

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

impl From<PixelPosition> for Coordinate {
  fn from(pp: PixelPosition) -> Self {
    Coordinate::from(TileCoordinate::from_pixel_position(pp, 2))
  }
}

#[derive(Debug, PartialEq, Copy, Clone, Hash, Eq, Serialize, Deserialize)]
pub struct Tile {
  pub x: u32,
  pub y: u32,
  pub zoom: u8,
}

impl Tile {
  pub fn exists(&self) -> bool {
    let max_tile = 2u32.pow(self.zoom.into()) - 1;
    self.x <= max_tile && self.y <= max_tile
  }

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
  fn from(tile_coord: TileCoordinate) -> Self {
    Self {
      x: tile_coord.x.floor() as u32,
      y: tile_coord.y.floor() as u32,
      zoom: tile_coord.zoom,
    }
  }
}

impl TileCoordinate {
  pub fn from_coordinate(coord: Coordinate, zoom: u8) -> Self {
    let x = (coord.lon + 180.) / 360. * 2f32.powi(zoom.into());
    let y = (1. - ((coord.lat * PI / 180.).tan() + 1. / (coord.lat * PI / 180.).cos()).ln() / PI)
      * 2f32.powi((zoom - 1).into());
    Self { x, y, zoom }
  }

  pub fn from_pixel_position(pixel_pos: PixelPosition, zoom: u8) -> Self {
    TileCoordinate {
      x: pixel_pos.x / TILE_SIZE * 2f32.powi(zoom as i32 - 2),
      y: pixel_pos.y / TILE_SIZE * 2f32.powi(zoom as i32 - 2),
      zoom,
    }
  }
}

const PI: f32 = std::f32::consts::PI;
impl From<TileCoordinate> for Coordinate {
  fn from(tile_coord: TileCoordinate) -> Self {
    Coordinate {
      lat: f32::atan(f32::sinh(
        PI - tile_coord.y / 2f32.powi(tile_coord.zoom.into()) * 2. * PI,
      )) * 180.
        / PI,
      lon: tile_coord.x / 2f32.powi(tile_coord.zoom.into()) * 360. - 180.,
    }
  }
}

#[derive(Debug)]
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
  pub fn new() -> Self {
    Self::get_invalid()
  }

  pub fn get_invalid() -> Self {
    Self {
      max_x: f32::MIN,
      min_x: f32::MAX,
      max_y: f32::MIN,
      min_y: f32::MAX,
    }
  }

  pub fn center(&self) -> PixelPosition {
    PixelPosition {
      x: (self.max_x + self.min_x) / 2.,
      y: (self.max_y + self.min_y) / 2.,
    }
  }

  pub fn from_iterator<I: IntoIterator<Item = PixelPosition>>(positions: I) -> Self {
    let mut bb = Self::get_invalid();
    positions.into_iter().for_each(|pos| bb.add_coordinate(pos));
    bb
  }

  pub fn is_valid(&self) -> bool {
    self.min_y <= self.max_y && self.min_x <= self.max_x
  }

  pub fn add_coordinate(&mut self, pp: PixelPosition) {
    self.min_y = self.min_y.min(pp.y);
    self.min_x = self.min_x.min(pp.x);
    self.max_y = self.max_y.max(pp.y);
    self.max_x = self.max_x.max(pp.x);
  }

  pub fn added_coordinate(mut self, pp: PixelPosition) -> Self {
    self.add_coordinate(pp);
    self
  }

  pub fn extend(&mut self, bb: &Self) {
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
  }

  pub fn width(&self) -> f32 {
    self.max_x - self.min_x
  }

  pub fn height(&self) -> f32 {
    self.max_y - self.min_y
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn coordinate_tile_conversions() {
    let coord = Coordinate {
      lat: 52.521_977,
      lon: 13.413_305,
    };

    let tc13 = TileCoordinate::from_coordinate(coord, 13);
    assert!(Coordinate::from(tc13).lat - coord.lat < 0.000_000_1);
    assert!(Coordinate::from(tc13).lon - coord.lon < 0.000_000_1);

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
    assert!(Coordinate::from(tc17).lat - coord.lat < 0.000_000_1);
    assert!(Coordinate::from(tc17).lon - coord.lon < 0.000_000_1);
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
  #[test]
  fn coordinate_to_pixel_zero() {
    let coord = Coordinate { lat: 0.0, lon: 0.0 };
    let tc2 = TileCoordinate::from_coordinate(coord, 2);
    let tc3 = TileCoordinate::from_coordinate(coord, 3);
    let tc4 = TileCoordinate::from_coordinate(coord, 4);
    let pp = PixelPosition { x: 500., y: 500. };
    assert_eq!(PixelPosition::from(tc2), pp);
    assert_eq!(PixelPosition::from(tc3), pp);
    assert_eq!(PixelPosition::from(tc4), pp);
  }

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
    assert_eq!(Coordinate::from(tc3), Coordinate::from(pp));
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
}
