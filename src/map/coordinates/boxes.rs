use serde::{Deserialize, Serialize};

use super::{Coordinate, PixelCoordinate, TileCoordinate};

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
  pub fn position(&self) -> (PixelCoordinate, PixelCoordinate) {
    (
      PixelCoordinate::from(TileCoordinate {
        x: self.x as f32,
        y: self.y as f32,
        zoom: self.zoom,
      }),
      PixelCoordinate::from(TileCoordinate {
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
  pub fn center(&self) -> PixelCoordinate {
    PixelCoordinate {
      x: (self.max_x + self.min_x) / 2.,
      y: (self.max_y + self.min_y) / 2.,
    }
  }

  pub fn from_iterator<C: Coordinate, I: IntoIterator<Item = C>>(positions: I) -> Self {
    let mut bb = Self::get_invalid();
    positions
      .into_iter()
      .for_each(|pos| bb.add_coordinate(pos.as_pixel_coordinate()));
    bb
  }

  #[must_use]
  pub fn is_valid(&self) -> bool {
    self.min_y <= self.max_y
      && self.min_x <= self.max_x
      && self.min_x.abs() < 2048.
      && self.min_y.abs() < 2048.
      && self.max_x.abs() < 2048.
      && self.max_x.abs() < 2048.
  }

  #[must_use]
  pub fn is_box(&self) -> bool {
    self.is_valid() && self.width() > 0. && self.height() > 0.
  }

  pub fn frame(&mut self, frame: f32) {
    self.min_x -= frame;
    self.min_y -= frame;
    self.max_x += frame;
    self.max_y += frame;
  }

  pub fn add_coordinate(&mut self, pp: PixelCoordinate) {
    self.min_y = self.min_y.min(pp.y);
    self.min_x = self.min_x.min(pp.x);
    self.max_y = self.max_y.max(pp.y);
    self.max_x = self.max_x.max(pp.x);
  }

  #[must_use]
  pub fn extend(self, bb: &Self) -> Self {
    if !self.is_valid() {
      return *bb;
    }

    if !bb.is_valid() {
      return self;
    }

    Self {
      min_x: self.min_x.min(bb.min_x),
      min_y: self.min_y.min(bb.min_y),
      max_x: self.max_x.max(bb.max_x),
      max_y: self.max_y.max(bb.max_y),
    }
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

#[cfg(test)]
mod tests {
  use crate::map::coordinates::WGS84Coordinate;

  use super::*;

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
