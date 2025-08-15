use super::Parser;
use serde::{Deserialize, Serialize};

use crate::map::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata},
  map_event::{Layer, MapEvent},
};
use chrono::{DateTime, Utc};
use time::OffsetDateTime;

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

  fn convert_gpx_time_to_chrono(gpx_time: &gpx::Time) -> Option<DateTime<Utc>> {
    // Convert gpx::Time to time::OffsetDateTime, then to chrono::DateTime
    let offset_dt: OffsetDateTime = (*gpx_time).into();
    DateTime::<Utc>::from_timestamp(offset_dt.unix_timestamp(), 0)
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
          
          let mut metadata = Metadata::default();
          if let Some(time) = waypoint.time.as_ref() {
            if let Some(timestamp) = Self::convert_gpx_time_to_chrono(time) {
              metadata = metadata.with_timestamp(timestamp);
            }
          }
          
          geometries.push(Geometry::Point(coord, metadata));
        }

        for track in &gpx_data.tracks {
          for segment in &track.segments {
            let track_points_with_time: Vec<(PixelCoordinate, Option<DateTime<Utc>>)> = segment
              .points
              .iter()
              .map(|p| {
                let coord = PixelCoordinate::from(WGS84Coordinate::new(
                  p.point().y() as f32,
                  p.point().x() as f32,
                ));
                let timestamp = p.time.as_ref().and_then(Self::convert_gpx_time_to_chrono);
                (coord, timestamp)
              })
              .collect();

            if track_points_with_time.len() >= 2 {
              // Check if we have temporal data
              let has_timestamps = track_points_with_time.iter().any(|(_, time)| time.is_some());
              
              if has_timestamps {
                // Create individual point geometries with timestamps for temporal visualization
                for (coord, timestamp) in &track_points_with_time {
                  if let Some(time) = timestamp {
                    let metadata = Metadata::default().with_timestamp(*time);
                    geometries.push(Geometry::Point(*coord, metadata));
                  }
                }
              }
              
              // Also create the linestring for the track path
              let track_points: Vec<PixelCoordinate> = track_points_with_time
                .iter()
                .map(|(coord, _)| *coord)
                .collect();
              
              // Create metadata for the linestring with time span if we have timestamps
              let mut metadata = Metadata::default();
              if has_timestamps {
                let timestamps: Vec<DateTime<Utc>> = track_points_with_time
                  .iter()
                  .filter_map(|(_, time)| *time)
                  .collect();
                
                if !timestamps.is_empty() {
                  let begin = timestamps.iter().min().copied();
                  let end = timestamps.iter().max().copied();
                  metadata = metadata.with_time_span(begin, end);
                }
              }
              
              geometries.push(Geometry::LineString(track_points, metadata));
            }
          }
        }

        for route in &gpx_data.routes {
          let route_points_with_time: Vec<(PixelCoordinate, Option<DateTime<Utc>>)> = route
            .points
            .iter()
            .map(|p| {
              let coord = PixelCoordinate::from(WGS84Coordinate::new(
                p.point().y() as f32,
                p.point().x() as f32,
              ));
              let timestamp = p.time.as_ref().and_then(Self::convert_gpx_time_to_chrono);
              (coord, timestamp)
            })
            .collect();

          if route_points_with_time.len() >= 2 {
            let route_points: Vec<PixelCoordinate> = route_points_with_time
              .iter()
              .map(|(coord, _)| *coord)
              .collect();
            
            // Create metadata with time span if we have timestamps
            let mut metadata = Metadata::default();
            let timestamps: Vec<DateTime<Utc>> = route_points_with_time
              .iter()
              .filter_map(|(_, time)| *time)
              .collect();
            
            if !timestamps.is_empty() {
              let begin = timestamps.iter().min().copied();
              let end = timestamps.iter().max().copied();
              metadata = metadata.with_time_span(begin, end);
            }
            
            geometries.push(Geometry::LineString(route_points, metadata));
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

  #[test]
  fn test_gpx_temporal_parsing() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let gpx_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/temporal_track.gpx"
    );
    let file = File::open(gpx_path).expect("Could not open temporal GPX file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = GpxParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GPX parser should produce events");

    if let Some(MapEvent::Layer(layer)) = events.first() {
      // Should have waypoints (2) + track points (5) + track linestring (1) = 8 geometries
      assert!(
        layer.geometries.len() >= 7,
        "Should have at least 7 geometries (2 waypoints + 5 track points + 1 track linestring), got {}",
        layer.geometries.len()
      );

      let mut timestamped_points = 0;
      let mut linestrings_with_timespan = 0;

      for geometry in &layer.geometries {
        match geometry {
          Geometry::Point(_, metadata) => {
            if metadata.time_data.is_some() {
              timestamped_points += 1;
            }
          }
          Geometry::LineString(_, metadata) => {
            if let Some(time_data) = &metadata.time_data {
              if time_data.time_span.is_some() {
                linestrings_with_timespan += 1;
              }
            }
          }
          _ => {}
        }
      }

      assert!(
        timestamped_points >= 7,
        "Should have at least 7 timestamped points (2 waypoints + 5 track points), got {}",
        timestamped_points
      );
      assert_eq!(
        linestrings_with_timespan, 1,
        "Should have exactly 1 linestring with time span, got {}",
        linestrings_with_timespan
      );

      // Verify that at least one geometry has proper timestamp
      let has_valid_timestamp = layer.geometries.iter().any(|geom| {
        match geom {
          Geometry::Point(_, metadata) => {
            if let Some(time_data) = &metadata.time_data {
              time_data.timestamp.is_some()
            } else {
              false
            }
          }
          _ => false,
        }
      });

      assert!(
        has_valid_timestamp,
        "At least one point should have a valid timestamp"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }
}
