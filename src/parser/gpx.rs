use super::Parser;
use serde::{Deserialize, Serialize};

use crate::map::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata},
  map_event::{Layer, MapEvent},
};

#[derive(Serialize, Deserialize, Clone)]
pub struct GpxParser {
  #[serde(skip)]
  data: String,
  #[serde(skip)]
  layer_name: String,
}

impl Default for GpxParser {
  fn default() -> Self {
    Self::new()
  }
}

impl GpxParser {
  #[must_use]
  pub fn new() -> Self {
    Self {
      data: String::new(),
      layer_name: "gpx".to_string(),
    }
  }

  #[allow(clippy::cast_possible_truncation)]
  fn parse_gpx(&self) -> Vec<Geometry<PixelCoordinate>> {
    let mut geometries = Vec::new();

    match gpx::read(&mut self.data.as_bytes()) {
      Ok(gpx_data) => {
        for waypoint in &gpx_data.waypoints {
          let coord = PixelCoordinate::from(WGS84Coordinate::new(
            waypoint.point().y() as f32,
            waypoint.point().x() as f32,
          ));
          geometries.push(Geometry::Point(coord, Metadata::default()));
        }

        for track in &gpx_data.tracks {
          for segment in &track.segments {
            let track_points: Vec<PixelCoordinate> = segment
              .points
              .iter()
              .map(|p| {
                PixelCoordinate::from(WGS84Coordinate::new(
                  p.point().y() as f32,
                  p.point().x() as f32,
                ))
              })
              .collect();

            if track_points.len() >= 2 {
              geometries.push(Geometry::LineString(track_points, Metadata::default()));
            }
          }
        }

        for route in &gpx_data.routes {
          let route_points: Vec<PixelCoordinate> = route
            .points
            .iter()
            .map(|p| {
              PixelCoordinate::from(WGS84Coordinate::new(
                p.point().y() as f32,
                p.point().x() as f32,
              ))
            })
            .collect();

          if route_points.len() >= 2 {
            geometries.push(Geometry::LineString(route_points, Metadata::default()));
          }
        }
      }
      Err(e) => {
        log::warn!("Error parsing GPX: {e}");
      }
    }

    geometries
  }
}

impl Parser for GpxParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    self.data.push_str(line);
    None
  }

  fn finalize(&mut self) -> Option<MapEvent> {
    let geometries = self.parse_gpx();
    if !geometries.is_empty() {
      let mut layer = Layer::new(self.layer_name.clone());
      layer.geometries = geometries;
      return Some(MapEvent::Layer(layer));
    }
    None
  }

  fn set_layer_name(&mut self, layer_name: String) {
    self.layer_name = layer_name;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::map::{
    coordinates::{PixelCoordinate, WGS84Coordinate},
    geometry_collection::{Geometry, Metadata},
    map_event::MapEvent,
  };

  #[test]
  fn test_gpx_waypoint() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let gpx_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/simple_waypoint.gpx"
    );
    let file = File::open(gpx_path).expect("Could not open simple waypoint GPX file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = GpxParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GPX parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.geometries.len(), 1, "Should have 1 waypoint");

      let expected_coord = PixelCoordinate::from(WGS84Coordinate::new(52.0, 10.0));
      let expected_geometry = Geometry::Point(expected_coord, Metadata::default());

      assert_eq!(
        layer.geometries[0], expected_geometry,
        "Waypoint geometry should match expected"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_gpx_track() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let gpx_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/simple_track.gpx"
    );
    let file = File::open(gpx_path).expect("Could not open simple track GPX file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = GpxParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GPX parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.geometries.len(), 1, "Should have 1 track");

      let expected_coords = vec![
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.0, 10.0),
        ),
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.1, 10.1),
        ),
      ];
      let expected_geometry = Geometry::LineString(expected_coords, Metadata::default());

      assert_eq!(
        layer.geometries[0], expected_geometry,
        "Track geometry should match expected"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_gpx_parsing() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let gpx_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/test_track.gpx"
    );
    let file = File::open(gpx_path).expect("Could not open test GPX file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = GpxParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GPX parser should produce events");

    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert!(
        layer.geometries.len() >= 4,
        "Should have at least 4 geometries (2 waypoints, 1 track, 1 route)"
      );

      let mut points = 0;
      let mut linestrings = 0;

      for geometry in &layer.geometries {
        match geometry {
          Geometry::Point(_, _) => points += 1,
          Geometry::LineString(_, _) => linestrings += 1,
          _ => {}
        }
      }

      assert!(points >= 2, "Should have at least 2 waypoints");
      assert!(
        linestrings >= 2,
        "Should have at least 2 linestrings (track + route)"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }
}
