#[derive(Debug, PartialEq, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub struct Coordinate {
  pub lat: f32,
  pub lon: f32,
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct TileCoordinate {
  pub x: f32,
  pub y: f32,
  pub zoom: u8,
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct PixelPosition {
  pub x: f32,
  pub y: f32,
}

pub fn tiles_in_box(nw: TileCoordinate, se: TileCoordinate) -> Vec<Tile> {
  assert_eq!(nw.zoom, se.zoom);
  let nw_tile = Tile::from(nw);
  let se_tile = Tile::from(se);
  let mut tiles = Vec::<Tile>::with_capacity(
    ((se_tile.x - nw_tile.x + 1) * (se_tile.y - nw_tile.y + 1)) as usize,
  );
  for (x, y) in itertools::iproduct!(nw_tile.x..=se_tile.x, nw_tile.y..=se_tile.y) {
    let tile = Tile {
      x,
      y,
      zoom: nw_tile.zoom,
    };
    if tile.exists() {
      tiles.push(tile);
    };
  }
  tiles
}

const CANVAS_SIZE: f32 = 1000.;
pub const TILE_SIZE: f32 = 250.;
impl From<TileCoordinate> for PixelPosition {
  fn from(tile_coord: TileCoordinate) -> Self {
    PixelPosition {
      x: tile_coord.x * TILE_SIZE / 2f32.powi(tile_coord.zoom as i32 - 2),
      y: tile_coord.y * TILE_SIZE / 2f32.powi(tile_coord.zoom as i32 - 2),
    }
  }
}

impl PixelPosition {
  pub fn clamp(&self) -> Self {
    PixelPosition {
      x: self.x.clamp(0.0, CANVAS_SIZE),
      y: self.y.clamp(0.0, CANVAS_SIZE),
    }
  }
}

impl From<PixelPosition> for Coordinate {
  fn from(pp: PixelPosition) -> Self {
    Coordinate::from(TileCoordinate::from_pixel_position(pp, 2))
  }
}

#[derive(Debug, PartialEq, Copy, Clone, Hash, Eq)]
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn coordinate_tile_conversions() {
    let coord = Coordinate {
      lat: 52.521977,
      lon: 13.413305,
    };

    let tc13 = TileCoordinate::from_coordinate(coord, 13);
    assert!(Coordinate::from(tc13).lat - coord.lat < 0.0000001);
    assert!(Coordinate::from(tc13).lon - coord.lon < 0.0000001);

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
    assert!(Coordinate::from(tc17).lat - coord.lat < 0.0000001);
    assert!(Coordinate::from(tc17).lon - coord.lon < 0.0000001);
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

    let tiles = tiles_in_box(nw, se);
    assert_eq!(tiles.len(), 200);
  }
}
