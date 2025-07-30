use super::Parser;
use serde::{Deserialize, Serialize};

use crate::map::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata},
  map_event::{Layer, MapEvent},
};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct GpxParser {
  #[serde(skip)]
  data: String,
}

impl GpxParser {
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

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
        log::warn!("Error parsing GPX: {}", e);
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
      let mut layer = Layer::new("gpx".to_string());
      layer.geometries = geometries;
      return Some(MapEvent::Layer(layer));
    }
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_gpx_waypoint() {
    let gpx_data = r#"<?xml version="1.0"?>
<gpx version="1.1">
  <wpt lat="52.0" lon="10.0">
    <name>Waypoint 1</name>
  </wpt>
</gpx>"#;

    let mut parser = GpxParser::new();
    parser.parse_line(gpx_data);
    let result = parser.finalize();
    assert!(result.is_some());
  }

  #[test]
  fn test_gpx_track() {
    let gpx_data = r#"<?xml version="1.0"?>
<gpx version="1.1">
  <trk>
    <trkseg>
      <trkpt lat="52.0" lon="10.0"></trkpt>
      <trkpt lat="52.1" lon="10.1"></trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    let mut parser = GpxParser::new();
    parser.parse_line(gpx_data);
    let result = parser.finalize();
    assert!(result.is_some());
  }
}