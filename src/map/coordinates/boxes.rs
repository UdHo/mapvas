use serde::{Deserialize, Serialize};

use super::{Coordinate, PixelCoordinate, TileCoordinate};

/// Priority levels for tile loading
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub enum TilePriority {
  /// Currently visible tiles - highest priority
  Current = 0,
  /// Tiles adjacent to current view - medium priority
  Adjacent = 1,
  /// Tiles at different zoom levels - lower priority
  ZoomLevel = 2,
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

  /// Get the four child tiles at the next zoom level.
  #[must_use]
  pub fn children(&self) -> Vec<Self> {
    let child_zoom = self.zoom + 1;
    let base_x = self.x << 1;
    let base_y = self.y << 1;

    vec![
      Self {
        x: base_x,
        y: base_y,
        zoom: child_zoom,
      },
      Self {
        x: base_x + 1,
        y: base_y,
        zoom: child_zoom,
      },
      Self {
        x: base_x,
        y: base_y + 1,
        zoom: child_zoom,
      },
      Self {
        x: base_x + 1,
        y: base_y + 1,
        zoom: child_zoom,
      },
    ]
    .into_iter()
    .filter(Tile::exists)
    .collect()
  }

  /// Get surrounding tiles (8 adjacent tiles around this one).
  /// Wraps horizontally across the antimeridian.
  #[must_use]
  #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
  pub fn surrounding_tiles(&self) -> Vec<Self> {
    let mut tiles = Vec::with_capacity(8);
    let num_tiles = 2i32.pow(self.zoom.into());

    for dx in -1_i32..=1_i32 {
      for dy in -1_i32..=1_i32 {
        if dx == 0 && dy == 0 {
          continue;
        }

        let new_x = (self.x as i32 + dx).rem_euclid(num_tiles);
        let new_y = self.y as i32 + dy;

        if new_y >= 0 && new_y < num_tiles {
          tiles.push(Self {
            x: new_x as u32,
            y: new_y as u32,
            zoom: self.zoom,
          });
        }
      }
    }

    tiles
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
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
  )]
  fn from(tile_coord: TileCoordinate) -> Self {
    let num_tiles = 2u32.pow(tile_coord.zoom.into());
    let x = tile_coord.x.floor() as i32;
    Self {
      x: x.rem_euclid(num_tiles as i32) as u32,
      y: tile_coord.y.floor() as u32,
      zoom: tile_coord.zoom,
    }
  }
}

/// A function to create a tile iterator for a given bounding box.
/// Handles antimeridian wrapping when `nw.x > se.x` and viewports wider than one world.
#[allow(
  clippy::cast_possible_truncation,
  clippy::cast_sign_loss,
  clippy::cast_precision_loss
)]
pub fn tiles_in_box(nw: TileCoordinate, se: TileCoordinate) -> impl Iterator<Item = Tile> {
  let nw_tile = Tile::from(nw);
  let se_tile = Tile::from(se);
  let num_tiles = 2u32.pow(nw_tile.zoom.into());
  let max_tile = num_tiles - 1;

  // If the unwrapped x-span covers a full world or more, return all x tiles.
  let x_span = se.x - nw.x;

  let x_ranges: Vec<(u32, u32)> = if x_span >= num_tiles as f32 {
    vec![(0, max_tile)]
  } else if nw_tile.x <= se_tile.x {
    vec![(nw_tile.x, se_tile.x)]
  } else {
    vec![(nw_tile.x, max_tile), (0, se_tile.x)]
  };

  x_ranges
    .into_iter()
    .flat_map(move |(x_start, x_end)| {
      (x_start..=x_end).flat_map(move |x| {
        (nw_tile.y..=se_tile.y).map(move |y| Tile {
          x,
          y,
          zoom: nw_tile.zoom,
        })
      })
    })
    .filter(Tile::exists)
}

/// Generate preload tiles for a given set of visible tiles.
#[must_use]
pub fn generate_preload_tiles(visible_tiles: &[Tile]) -> Vec<(Tile, TilePriority)> {
  let mut preload_tiles = Vec::new();

  for tile in visible_tiles {
    // Add surrounding tiles at same zoom level
    for surrounding in tile.surrounding_tiles() {
      preload_tiles.push((surrounding, TilePriority::Adjacent));
    }

    // Add parent tile (one zoom level lower)
    if let Some(parent) = tile.parent() {
      preload_tiles.push((parent, TilePriority::ZoomLevel));
    }

    // Add child tiles (one zoom level higher)
    for child in tile.children() {
      preload_tiles.push((child, TilePriority::ZoomLevel));
    }
  }

  // Remove duplicates and sort by priority
  preload_tiles.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.zoom.cmp(&b.0.zoom)));
  preload_tiles.dedup_by(|a, b| a.0 == b.0);

  preload_tiles
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
      x: f32::midpoint(self.max_x, self.min_x),
      y: f32::midpoint(self.max_y, self.min_y),
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
      && self.max_y.abs() < 2048.
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

  #[must_use]
  pub fn min_x(&self) -> f32 {
    self.min_x
  }

  #[must_use]
  pub fn min_y(&self) -> f32 {
    self.min_y
  }

  #[must_use]
  pub fn max_x(&self) -> f32 {
    self.max_x
  }

  #[must_use]
  pub fn max_y(&self) -> f32 {
    self.max_y
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

  #[test]
  fn test_tile_children() {
    let tile = Tile {
      x: 1,
      y: 1,
      zoom: 5,
    };
    let children = tile.children();

    assert_eq!(children.len(), 4);
    assert_eq!(
      children[0],
      Tile {
        x: 2,
        y: 2,
        zoom: 6
      }
    );
    assert_eq!(
      children[1],
      Tile {
        x: 3,
        y: 2,
        zoom: 6
      }
    );
    assert_eq!(
      children[2],
      Tile {
        x: 2,
        y: 3,
        zoom: 6
      }
    );
    assert_eq!(
      children[3],
      Tile {
        x: 3,
        y: 3,
        zoom: 6
      }
    );
  }

  #[test]
  fn test_tile_parent() {
    let tile = Tile {
      x: 4,
      y: 6,
      zoom: 10,
    };
    let parent = tile.parent().unwrap();

    assert_eq!(
      parent,
      Tile {
        x: 2,
        y: 3,
        zoom: 9
      }
    );
  }

  #[test]
  fn test_surrounding_tiles() {
    let tile = Tile {
      x: 5,
      y: 5,
      zoom: 10,
    };
    let surrounding = tile.surrounding_tiles();

    assert_eq!(surrounding.len(), 8);
    // Check some expected surrounding tiles
    assert!(surrounding.contains(&Tile {
      x: 4,
      y: 4,
      zoom: 10
    })); // NW
    assert!(surrounding.contains(&Tile {
      x: 5,
      y: 4,
      zoom: 10
    })); // N
    assert!(surrounding.contains(&Tile {
      x: 6,
      y: 4,
      zoom: 10
    })); // NE
    assert!(surrounding.contains(&Tile {
      x: 4,
      y: 5,
      zoom: 10
    })); // W
    assert!(surrounding.contains(&Tile {
      x: 6,
      y: 5,
      zoom: 10
    })); // E
  }

  #[test]
  fn test_generate_preload_tiles() {
    let visible_tiles = vec![Tile {
      x: 5,
      y: 5,
      zoom: 10,
    }];

    let preload = generate_preload_tiles(&visible_tiles);

    // Should have surrounding tiles, parent, and children
    assert!(!preload.is_empty());

    // Check that we have different priorities
    let priorities: std::collections::HashSet<_> = preload.iter().map(|(_, p)| *p).collect();
    assert!(priorities.contains(&TilePriority::Adjacent)); // Surrounding tiles
    assert!(priorities.contains(&TilePriority::ZoomLevel)); // Parent and children
  }

  #[test]
  fn test_tile_priority_ordering() {
    assert!(TilePriority::Current < TilePriority::Adjacent);
    assert!(TilePriority::Adjacent < TilePriority::ZoomLevel);
  }

  #[test]
  fn tiles_in_box_wraps_antimeridian() {
    // At zoom 5, there are 32 tiles (0..31). Viewport from x=30 to x=1 crosses the antimeridian.
    let nw = TileCoordinate {
      x: 30.5,
      y: 1.0,
      zoom: 5,
    };
    let se = TileCoordinate {
      x: 1.5,
      y: 3.0,
      zoom: 5,
    };
    let tiles: Vec<_> = tiles_in_box(nw, se).collect();
    // Should get tiles at x=30,31,0,1 * y=1,2,3 = 12 tiles
    assert_eq!(tiles.len(), 12);
    let xs: std::collections::HashSet<u32> = tiles.iter().map(|t| t.x).collect();
    assert!(xs.contains(&30));
    assert!(xs.contains(&31));
    assert!(xs.contains(&0));
    assert!(xs.contains(&1));
  }

  #[test]
  fn tile_from_coordinate_wraps_x() {
    // Tile coordinate with negative x should wrap
    let tc = TileCoordinate {
      x: -0.5,
      y: 1.0,
      zoom: 5,
    };
    let tile = Tile::from(tc);
    assert_eq!(tile.x, 31); // -1 mod 32 = 31
  }

  #[test]
  fn surrounding_tiles_wraps_at_antimeridian() {
    let tile = Tile {
      x: 0,
      y: 5,
      zoom: 5,
    };
    let surrounding = tile.surrounding_tiles();
    // Western neighbor should wrap to x=31
    assert!(surrounding.contains(&Tile {
      x: 31,
      y: 5,
      zoom: 5
    }));

    let tile_end = Tile {
      x: 31,
      y: 5,
      zoom: 5,
    };
    let surrounding_end = tile_end.surrounding_tiles();
    // Eastern neighbor should wrap to x=0
    assert!(surrounding_end.contains(&Tile {
      x: 0,
      y: 5,
      zoom: 5
    }));
  }
}
