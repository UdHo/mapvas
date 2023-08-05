use serde::{Deserialize, Serialize};

use super::coordinates::{Coordinate, Tile};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Segment {
  pub from: Coordinate,
  pub to: Coordinate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
//#[serde(untagged)]
pub enum MapEvent {
  Shutdown,
  TileDataArrived { tile: Tile, data: Vec<u8> },
  DrawEvent { coords: Vec<Coordinate> },
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
      serde_json::to_string(&crate::MapEvent::Shutdown).unwrap(),
      "\"Shutdown\"".to_string()
    );
  }
}
