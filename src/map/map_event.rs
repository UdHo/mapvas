use serde::{Deserialize, Serialize};

use super::coordinates::{Coordinate, Tile};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Segment {
  pub from: Coordinate,
  pub to: Coordinate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MapEvent {
  Shutdown,
  TileDataArrived { tile: Tile, data: Vec<u8> },
  DrawEvent { segment: Segment },
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn coordinate_tile_conversions() {
    let berlin = Coordinate {
      lat: 52.521977,
      lon: 13.413305,
    };
    let heinsberg = Coordinate {
      lat: 52.521977,
      lon: 13.413305,
    };

    assert_eq!(
      serde_json::to_string(&MapEvent::DrawEvent {
        segment: Segment {
          from: heinsberg,
          to: berlin
        }
      })
      .unwrap(),
      "".to_string()
    );
  }
}
