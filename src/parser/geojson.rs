use super::{Parser, style::StyleParser};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
  map::{
    coordinates::{PixelCoordinate, WGS84Coordinate},
    geometry_collection::{Geometry, Metadata, TimeData, TimeSpan},
    map_event::{Layer, MapEvent},
  },
  profile_scope,
};
use chrono::{DateTime, Utc};

#[derive(Serialize, Deserialize, Clone)]
pub struct GeoJsonParser {
  #[serde(skip)]
  data: String,
  #[serde(skip)]
  geometry: Vec<Geometry<PixelCoordinate>>,
  #[serde(skip)]
  layer_name: String,
}

impl Default for GeoJsonParser {
  fn default() -> Self {
    Self::new()
  }
}

impl GeoJsonParser {
  #[must_use]
  pub fn new() -> Self {
    Self {
      data: String::new(),
      geometry: Vec::new(),
      layer_name: "geojson".to_string(),
    }
  }

  /// Parse `GeoJSON` data and extract geometries with style information
  fn parse_geojson(&mut self, value: &Value) -> Result<(), String> {
    profile_scope!("GeoJsonParser::parse_geojson");
    match value {
      Value::Object(obj) => {
        if let Some(geotype) = obj.get("type").and_then(Value::as_str) {
          match geotype {
            "FeatureCollection" => {
              if let Some(features) = obj.get("features").and_then(Value::as_array) {
                for feature in features {
                  if let Err(e) = self.parse_feature(feature) {
                    log::warn!("Error parsing feature: {e}");
                  }
                }
              }
            }
            "Feature" => {
              self.parse_feature(value)?;
            }
            "Point" | "LineString" | "Polygon" | "MultiPoint" | "MultiLineString"
            | "MultiPolygon" | "GeometryCollection" => {
              if let Some(geometry) = self.parse_geometry(value, &Metadata::default()) {
                self.geometry.push(geometry);
              }
            }
            _ => {
              return Err(format!("Unknown GeoJSON type: {geotype}"));
            }
          }
        } else {
          return Err("Missing 'type' field for GeoJSON".to_string());
        }
      }
      _ => {
        return Err("GeoJSON must be an object".to_string());
      }
    }
    Ok(())
  }

  /// Parse a `GeoJSON` Feature object
  fn parse_feature(&mut self, feature: &Value) -> Result<(), String> {
    let obj = feature.as_object().ok_or("Feature must be an object")?;

    let mut metadata = Self::extract_metadata_from_properties(obj.get("properties"));

    // Check for GeoJSON-T "when" temporal objects
    if let Some(when_obj) = obj.get("when")
      && let Some(when_time_data) = Self::extract_temporal_data_from_when(when_obj)
    {
      metadata = metadata.with_time_data(when_time_data);
    }

    if let Some(geometry_value) = obj.get("geometry")
      && let Some(geometry) = self.parse_geometry(geometry_value, &metadata)
    {
      self.geometry.push(geometry);
    }

    Ok(())
  }

  /// Extract metadata and style from `GeoJSON` properties
  fn extract_metadata_from_properties(properties: Option<&Value>) -> Metadata {
    StyleParser::extract_metadata_from_json(properties)
  }

  /// Extract temporal data from GeoJSON-T "when" objects
  fn extract_temporal_data_from_when(when_obj: &Value) -> Option<TimeData> {
    let obj = when_obj.as_object()?;

    // Handle timespans array
    if let Some(timespans) = obj.get("timespans")
      && let Some(array) = timespans.as_array()
      && !array.is_empty()
    {
      // Take the first timespan
      if let Some(timespan) = array[0].as_object() {
        let start = timespan
          .get("start")
          .and_then(Value::as_str)
          .and_then(Self::parse_when_timestamp);
        let end = timespan
          .get("end")
          .and_then(Value::as_str)
          .and_then(Self::parse_when_timestamp);

        if start.is_some() || end.is_some() {
          return Some(TimeData {
            timestamp: None,
            time_span: Some(TimeSpan { begin: start, end }),
          });
        }
      }
    }

    // Handle other GeoJSON-T temporal formats if needed
    None
  }

  /// Parse timestamp from GeoJSON-T when objects
  fn parse_when_timestamp(timestamp_str: &str) -> Option<DateTime<Utc>> {
    // Use the same parsing logic as StyleParser
    super::style::StyleParser::parse_timestamp(timestamp_str)
  }

  /// Parse `GeoJSON` geometry with metadata
  #[allow(
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::only_used_in_recursion
  )]
  fn parse_geometry(
    &self,
    geometry: &Value,
    metadata: &Metadata,
  ) -> Option<Geometry<PixelCoordinate>> {
    let obj = geometry.as_object()?;
    let geom_type = obj.get("type")?.as_str()?;
    let coordinates = obj.get("coordinates")?;

    match geom_type {
      "Point" => {
        let coord = Self::parse_coordinate(coordinates)?;
        Some(Geometry::Point(coord, metadata.clone()))
      }
      "LineString" => {
        let coords = Self::parse_coordinate_array(coordinates)?;
        if coords.len() >= 2 {
          Some(Geometry::LineString(coords, metadata.clone()))
        } else {
          None
        }
      }
      "Polygon" => {
        if let Some(rings) = coordinates.as_array() {
          if let Some(exterior_ring) = rings.first() {
            let coords = Self::parse_coordinate_array(exterior_ring)?;
            if coords.len() >= 3 {
              Some(Geometry::Polygon(coords, metadata.clone()))
            } else {
              None
            }
          } else {
            None
          }
        } else {
          None
        }
      }
      "MultiPoint" => {
        if let Some(points) = coordinates.as_array() {
          let geometries = points
            .iter()
            .filter_map(|point| {
              let coord = Self::parse_coordinate(point)?;
              Some(Geometry::Point(coord, Metadata::default()))
            })
            .collect();
          Some(Geometry::GeometryCollection(geometries, metadata.clone()))
        } else {
          None
        }
      }
      "MultiLineString" => {
        if let Some(lines) = coordinates.as_array() {
          let geometries = lines
            .iter()
            .filter_map(|line| {
              let coords = Self::parse_coordinate_array(line)?;
              if coords.len() >= 2 {
                Some(Geometry::LineString(coords, Metadata::default()))
              } else {
                None
              }
            })
            .collect();
          Some(Geometry::GeometryCollection(geometries, metadata.clone()))
        } else {
          None
        }
      }
      "MultiPolygon" => {
        if let Some(polygons) = coordinates.as_array() {
          let geometries = polygons
            .iter()
            .filter_map(|polygon| {
              if let Some(rings) = polygon.as_array() {
                if let Some(exterior_ring) = rings.first() {
                  let coords = Self::parse_coordinate_array(exterior_ring)?;
                  if coords.len() >= 3 {
                    Some(Geometry::Polygon(coords, Metadata::default()))
                  } else {
                    None
                  }
                } else {
                  None
                }
              } else {
                None
              }
            })
            .collect();
          Some(Geometry::GeometryCollection(geometries, metadata.clone()))
        } else {
          None
        }
      }
      "GeometryCollection" => {
        if let Some(geometries_array) = obj.get("geometries").and_then(Value::as_array) {
          let geometries = geometries_array
            .iter()
            .filter_map(|geom| self.parse_geometry(geom, &Metadata::default()))
            .collect();
          Some(Geometry::GeometryCollection(geometries, metadata.clone()))
        } else {
          None
        }
      }
      _ => None,
    }
  }

  /// Parse a single coordinate [lon, lat] or [lon, lat, elevation]
  #[allow(clippy::cast_possible_truncation)]
  fn parse_coordinate(coord: &Value) -> Option<PixelCoordinate> {
    if let Some(array) = coord.as_array()
      && array.len() >= 2
    {
      let lon = array[0].as_f64()? as f32;
      let lat = array[1].as_f64()? as f32;
      return Some(PixelCoordinate::from(WGS84Coordinate::new(lat, lon)));
    }
    None
  }

  /// Parse an array of coordinates [[lon, lat], [lon, lat], ...]
  fn parse_coordinate_array(coords: &Value) -> Option<Vec<PixelCoordinate>> {
    if let Some(array) = coords.as_array() {
      let coordinates = array.iter().filter_map(Self::parse_coordinate).collect();
      Some(coordinates)
    } else {
      None
    }
  }
}

impl Parser for GeoJsonParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    profile_scope!("GeoJsonParser::parse_line");
    let trimmed = line.trim();

    if trimmed.is_empty() {
      return None;
    }

    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed)
      && self.parse_geojson(&parsed).is_ok()
    {
      return None;
    }

    self.data.push_str(line);
    None
  }

  fn finalize(&mut self) -> Option<MapEvent> {
    profile_scope!("GeoJsonParser::finalize");
    if !self.data.is_empty()
      && let Ok(parsed) = serde_json::from_str::<Value>(&self.data)
      && let Err(e) = self.parse_geojson(&parsed)
    {
      log::warn!("GeoJSON document parsing failed: {e}");
    }

    if !self.geometry.is_empty() {
      let mut layer = Layer::new(self.layer_name.clone());
      layer.geometries.clone_from(&self.geometry);
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
    geometry_collection::{Geometry, Metadata, Style},
    map_event::MapEvent,
  };
  use crate::parser::{FileParser, test_utils::*};
  use std::fs::File;
  use std::io::BufReader;

  #[test]
  fn test_geojson_point_from_file() {
    let events = load_and_parse::<GeoJsonParser>("simple_point.geojson");
    let actual_layer = extract_layer(&events, "geojson");

    let expected_layer = LayerBuilder::new("geojson")
      .with_geometry(GeometryBuilder::point(52.0, 10.0))
      .build();

    assert_layer_eq(actual_layer, &expected_layer);
  }

  #[test]
  fn test_geojson_linestring_from_file() {
    let events = load_and_parse::<GeoJsonParser>("simple_linestring.geojson");
    let actual_layer = extract_layer(&events, "geojson");

    let expected_layer = LayerBuilder::new("geojson")
      .with_geometry(GeometryBuilder::linestring(&[
        (52.0, 10.0),
        (52.1, 10.1),
        (52.2, 10.2),
      ]))
      .build();

    assert_layer_eq(actual_layer, &expected_layer);
  }

  #[test]
  fn test_geojson_polygon_from_file() {
    let geojson_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/simple_polygon.geojson"
    );
    let file = File::open(geojson_path).expect("Could not open simple polygon GeoJSON file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = super::GeoJsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GeoJSON parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "geojson", "Layer should be named 'geojson'");
      assert_eq!(layer.geometries.len(), 1, "Should have 1 polygon geometry");

      let expected_coords = vec![
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.0, 10.0),
        ),
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.0, 10.1),
        ),
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.1, 10.1),
        ),
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.1, 10.0),
        ),
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.0, 10.0),
        ),
      ];
      let expected_geometry = Geometry::Polygon(expected_coords, Metadata::default());

      assert_eq!(
        layer.geometries[0], expected_geometry,
        "Polygon geometry should match expected"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_geojson_styled_feature_from_file() {
    let geojson_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/styled_feature.geojson"
    );
    let file = File::open(geojson_path).expect("Could not open styled feature GeoJSON file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = super::GeoJsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GeoJSON parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "geojson", "Layer should be named 'geojson'");
      assert_eq!(layer.geometries.len(), 1, "Should have 1 styled geometry");

      let expected_coord = PixelCoordinate::from(WGS84Coordinate::new(52.0, 10.0));
      let expected_style = Style::default()
        .with_color(egui::Color32::RED)
        .with_fill_color(egui::Color32::GREEN);
      let expected_metadata = Metadata::default()
        .with_label("Test Feature".to_string())
        .with_style(expected_style);
      let expected_geometry = Geometry::Point(expected_coord, expected_metadata);

      assert_eq!(
        layer.geometries[0], expected_geometry,
        "Styled geometry should match expected"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_geojson_feature_collection_from_file() {
    let geojson_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/feature_collection.geojson"
    );
    let file = File::open(geojson_path).expect("Could not open feature collection GeoJSON file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = super::GeoJsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GeoJSON parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "geojson", "Layer should be named 'geojson'");
      assert_eq!(
        layer.geometries.len(),
        3,
        "Should have 3 geometries from feature collection"
      );

      let mut has_point = false;
      let mut has_linestring = false;
      let mut has_polygon = false;
      let mut has_labels = 0;
      let mut has_styles = 0;

      for geometry in &layer.geometries {
        match geometry {
          Geometry::Point(_, metadata) => {
            has_point = true;
            if metadata.label.is_some() {
              has_labels += 1;
            }
            if metadata.style.is_some() {
              has_styles += 1;
            }
          }
          Geometry::LineString(_, metadata) => {
            has_linestring = true;
            if metadata.label.is_some() {
              has_labels += 1;
            }
            if metadata.style.is_some() {
              has_styles += 1;
            }
          }
          Geometry::Polygon(_, metadata) => {
            has_polygon = true;
            if metadata.label.is_some() {
              has_labels += 1;
            }
            if metadata.style.is_some() {
              has_styles += 1;
            }
          }
          Geometry::GeometryCollection(..) => {}
        }
      }

      assert!(has_point, "Should have point geometry");
      assert!(has_linestring, "Should have linestring geometry");
      assert!(has_polygon, "Should have polygon geometry");
      assert_eq!(has_labels, 3, "All geometries should have labels");
      assert_eq!(has_styles, 3, "All geometries should have styles");
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_geojson_multigeometry_from_file() {
    let geojson_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/multigeometry.geojson"
    );
    let file = File::open(geojson_path).expect("Could not open multigeometry GeoJSON file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = super::GeoJsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GeoJSON parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "geojson", "Layer should be named 'geojson'");
      assert_eq!(layer.geometries.len(), 3, "Should have 3 multi-geometries");

      for geometry in &layer.geometries {
        match geometry {
          Geometry::GeometryCollection(_, metadata) => {
            assert!(metadata.label.is_some(), "Multi-geometry should have label");
            assert!(metadata.style.is_some(), "Multi-geometry should have style");
          }
          _ => panic!("Multi-geometries should be converted to GeometryCollection"),
        }
      }
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_geojson_color_formats_from_file() {
    let geojson_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/color_formats.geojson"
    );
    let file = File::open(geojson_path).expect("Could not open color formats GeoJSON file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = super::GeoJsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GeoJSON parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "geojson", "Layer should be named 'geojson'");
      assert_eq!(
        layer.geometries.len(),
        5,
        "Should have 5 geometries with different color formats"
      );

      let mut parsed_colors = Vec::new();
      for geometry in &layer.geometries {
        if let Geometry::Point(_, metadata) = geometry
          && let Some(style) = &metadata.style
        {
          parsed_colors.push(style.color());
        }
      }

      assert_eq!(
        parsed_colors.len(),
        5,
        "All geometries should have parsed colors"
      );

      assert_eq!(parsed_colors[0], egui::Color32::RED);
      assert_eq!(parsed_colors[1], egui::Color32::RED);
      assert_eq!(parsed_colors[2], egui::Color32::GREEN);
      assert_eq!(parsed_colors[3], egui::Color32::BLUE);
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_geojson_linewise_from_file() {
    let events = load_and_parse::<GeoJsonParser>("linewise.geojson");
    let actual_layer = extract_layer(&events, "geojson");

    assert_eq!(
      actual_layer.geometries.len(),
      3,
      "Should have 3 geometries from linewise parsing"
    );
    let expected_point1 = GeometryBuilder::point(52.0, 10.0);
    assert_eq!(
      actual_layer.geometries[0], expected_point1,
      "First point should match"
    );

    let expected_style = StyleBuilder::with_color(egui::Color32::RED);
    let expected_metadata = MetadataBuilder::with_label_and_style("Feature Point", expected_style);
    let expected_point2 = GeometryBuilder::point_with_metadata(52.1, 10.1, expected_metadata);
    assert_eq!(
      actual_layer.geometries[1], expected_point2,
      "Second point should match with styling"
    );
    let expected_linestring =
      GeometryBuilder::linestring(&[(52.2, 10.2), (52.3, 10.3), (52.4, 10.4)]);
    assert_eq!(
      actual_layer.geometries[2], expected_linestring,
      "LineString should match"
    );
  }

  #[test]
  fn test_geojson_mixed_linewise_from_file() {
    let events = load_and_parse::<GeoJsonParser>("mixed_linewise.geojson");
    let actual_layer = extract_layer(&events, "geojson");

    assert_eq!(
      actual_layer.geometries.len(),
      3,
      "Should have 3 geometries from mixed linewise parsing"
    );

    let expected_point1 = GeometryBuilder::point(52.0, 10.0);
    assert_eq!(
      actual_layer.geometries[0], expected_point1,
      "First point should match"
    );

    let expected_metadata = MetadataBuilder::with_label("Test Point");
    let expected_point2 = GeometryBuilder::point_with_metadata(52.1, 10.1, expected_metadata);
    assert_eq!(
      actual_layer.geometries[1], expected_point2,
      "Feature point should match with label"
    );
    let expected_style = StyleBuilder::with_color(egui::Color32::GREEN);
    let expected_metadata = MetadataBuilder::with_style(expected_style);
    let expected_point3 = GeometryBuilder::point_with_metadata(52.2, 10.2, expected_metadata);
    assert_eq!(
      actual_layer.geometries[2], expected_point3,
      "FeatureCollection point should match with styling"
    );
  }

  #[test]
  fn test_geojson_multiline_document_fallback() {
    let events = load_and_parse::<GeoJsonParser>("multiline_document.geojson");
    let actual_layer = extract_layer(&events, "geojson");

    assert_eq!(
      actual_layer.geometries.len(),
      1,
      "Should have 1 geometry from document parsing"
    );

    let expected_metadata = MetadataBuilder::with_label("Multiline Point");
    let expected_point = GeometryBuilder::point_with_metadata(52.0, 10.0, expected_metadata);
    assert_eq!(
      actual_layer.geometries[0], expected_point,
      "Multiline document point should match"
    );
  }

  #[test]
  fn test_geojson_point_with_heading() {
    let events = load_and_parse::<GeoJsonParser>("point_with_heading.geojson");
    let actual_layer = extract_layer(&events, "geojson");

    assert_eq!(
      actual_layer.geometries.len(),
      4,
      "Should have 4 points with different heading properties"
    );

    // Test point with "heading" property
    if let Geometry::Point(_, metadata) = &actual_layer.geometries[0] {
      assert_eq!(
        metadata.label.as_ref().map(|l| &l.name),
        Some(&"Aircraft".to_string())
      );
      assert_eq!(metadata.heading, Some(45.0));
      assert!(metadata.style.is_some());
    } else {
      panic!("First geometry should be a Point");
    }

    // Test point with "bearing" property
    if let Geometry::Point(_, metadata) = &actual_layer.geometries[1] {
      assert_eq!(
        metadata.label.as_ref().map(|l| &l.name),
        Some(&"Ship".to_string())
      );
      assert_eq!(metadata.heading, Some(180.0));
    } else {
      panic!("Second geometry should be a Point");
    }

    // Test point with "direction" property
    if let Geometry::Point(_, metadata) = &actual_layer.geometries[2] {
      assert_eq!(
        metadata.label.as_ref().map(|l| &l.name),
        Some(&"Vehicle".to_string())
      );
      assert_eq!(metadata.heading, Some(270.0));
    } else {
      panic!("Third geometry should be a Point");
    }

    // Test point with "course" property
    if let Geometry::Point(_, metadata) = &actual_layer.geometries[3] {
      assert_eq!(
        metadata.label.as_ref().map(|l| &l.name),
        Some(&"Compass".to_string())
      );
      assert_eq!(metadata.heading, Some(90.0));
    } else {
      panic!("Fourth geometry should be a Point");
    }
  }

  #[test]
  fn test_geojson_temporal_features() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let geojson_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/temporal_features.geojson"
    );
    let file = File::open(geojson_path).expect("Could not open temporal features GeoJSON file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = GeoJsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GeoJSON parser should produce events");

    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "geojson", "Layer should be named 'geojson'");
      assert_eq!(layer.geometries.len(), 5, "Should have 5 temporal features");

      let mut features_with_timestamps = 0;
      let mut features_with_time_spans = 0;

      for geometry in &layer.geometries {
        match geometry {
          Geometry::Point(_, metadata) => {
            if metadata
              .time_data
              .as_ref()
              .is_some_and(|td| td.timestamp.is_some())
            {
              features_with_timestamps += 1;
            }
          }
          Geometry::LineString(_, metadata) => {
            if metadata
              .time_data
              .as_ref()
              .is_some_and(|td| td.time_span.is_some())
            {
              features_with_time_spans += 1;
            }
          }
          _ => {}
        }
      }

      assert_eq!(
        features_with_timestamps, 4,
        "Should have 4 point features with timestamps"
      );
      assert_eq!(
        features_with_time_spans, 1,
        "Should have 1 linestring feature with time span"
      );

      // Test specific timestamp parsing
      let berlin_point = &layer.geometries[0];
      if let Geometry::Point(_, metadata) = berlin_point {
        assert_eq!(
          metadata.label.as_ref().map(|l| &l.name),
          Some(&"Berlin Event".to_string())
        );
        assert!(
          metadata.time_data.is_some(),
          "Berlin event should have temporal data"
        );
        if let Some(time_data) = &metadata.time_data {
          assert!(
            time_data.timestamp.is_some(),
            "Berlin event should have timestamp"
          );
        }
      } else {
        panic!("First geometry should be Berlin point");
      }

      // Test time span parsing
      let journey_linestring = &layer.geometries[2];
      if let Geometry::LineString(_, metadata) = journey_linestring {
        assert_eq!(
          metadata.label.as_ref().map(|l| &l.name),
          Some(&"Journey Route".to_string())
        );
        assert!(
          metadata.time_data.is_some(),
          "Journey route should have temporal data"
        );
        if let Some(time_data) = &metadata.time_data {
          assert!(
            time_data.time_span.is_some(),
            "Journey route should have time span"
          );
          if let Some(time_span) = &time_data.time_span {
            assert!(
              time_span.begin.is_some(),
              "Journey route should have start time"
            );
            assert!(
              time_span.end.is_some(),
              "Journey route should have end time"
            );
          }
        }
      } else {
        panic!("Third geometry should be Journey linestring");
      }
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  #[allow(clippy::too_many_lines)]
  fn test_temporal_geojson_timestamp_parsing() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let geojson_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/temporal.geojson"
    );
    let file = File::open(geojson_path).expect("Could not open temporal.geojson file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = GeoJsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "GeoJSON parser should produce events");

    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "geojson", "Layer should be named 'geojson'");
      assert_eq!(
        layer.geometries.len(),
        7,
        "Should have 7 features from temporal.geojson"
      );

      let mut parsed_timestamps = 0;
      let mut detailed_results = Vec::new();

      for (i, geometry) in layer.geometries.iter().enumerate() {
        if let Geometry::Point(_, metadata) = geometry {
          let has_time_data = metadata.time_data.is_some();
          let format_info = match i {
            0 => "ISO 8601 UTC",
            1 => "ISO 8601 with offset",
            2 => "ISO 8601 date only",
            3 => "Unix epoch (seconds) - as number",
            4 => "Unix epoch (milliseconds) - as number",
            5 => "Time interval (time_range array)",
            6 => "GeoJSON-T timespan (when object)",
            _ => "Unknown",
          };

          detailed_results.push((i, format_info, has_time_data));

          if has_time_data {
            parsed_timestamps += 1;
          }
        }
      }

      // Print detailed results for debugging
      for (idx, format, parsed) in detailed_results {
        println!(
          "Feature {}: {} -> {}",
          idx,
          format,
          if parsed { "PARSED" } else { "NOT PARSED" }
        );
      }

      // All 7 timestamp formats should parse successfully:
      // - ISO 8601 UTC (index 0) -> timestamp
      // - ISO 8601 with offset (index 1) -> timestamp
      // - ISO 8601 date only (index 2) -> timestamp
      // - Unix epoch seconds (index 3) -> timestamp
      // - Unix epoch milliseconds (index 4) -> timestamp
      // - Time interval array (index 5) -> time span
      // - GeoJSON-T when object (index 6) -> time span
      assert_eq!(
        parsed_timestamps, 7,
        "All 7 timestamp formats should parse successfully, got {parsed_timestamps}"
      );

      // Verify specific parsing of ISO 8601 formats
      let iso_utc = &layer.geometries[0];
      if let Geometry::Point(_, metadata) = iso_utc {
        assert!(
          metadata.time_data.is_some(),
          "ISO 8601 UTC timestamp should parse"
        );
      }

      let iso_offset = &layer.geometries[1];
      if let Geometry::Point(_, metadata) = iso_offset {
        assert!(
          metadata.time_data.is_some(),
          "ISO 8601 with offset timestamp should parse"
        );
      }

      let iso_date = &layer.geometries[2];
      if let Geometry::Point(_, metadata) = iso_date {
        assert!(
          metadata.time_data.is_some(),
          "ISO 8601 date only should parse"
        );
      }

      // Verify Unix timestamp parsing
      let unix_seconds = &layer.geometries[3];
      if let Geometry::Point(_, metadata) = unix_seconds {
        assert!(
          metadata.time_data.is_some(),
          "Unix epoch (seconds) should parse"
        );
        if let Some(time_data) = &metadata.time_data {
          assert!(
            time_data.timestamp.is_some(),
            "Unix seconds should have timestamp"
          );
        }
      }

      let unix_millis = &layer.geometries[4];
      if let Geometry::Point(_, metadata) = unix_millis {
        assert!(
          metadata.time_data.is_some(),
          "Unix epoch (milliseconds) should parse"
        );
        if let Some(time_data) = &metadata.time_data {
          assert!(
            time_data.timestamp.is_some(),
            "Unix milliseconds should have timestamp"
          );
        }
      }

      // Verify time_range array parsing
      let time_range = &layer.geometries[5];
      if let Geometry::Point(_, metadata) = time_range {
        assert!(
          metadata.time_data.is_some(),
          "time_range array should parse"
        );
        if let Some(time_data) = &metadata.time_data {
          assert!(
            time_data.time_span.is_some(),
            "time_range should have time span"
          );
          if let Some(time_span) = &time_data.time_span {
            assert!(
              time_span.begin.is_some(),
              "time_range should have begin time"
            );
            assert!(time_span.end.is_some(), "time_range should have end time");
          }
        }
      }

      // Verify GeoJSON-T when object parsing
      let geojson_t = &layer.geometries[6];
      if let Geometry::Point(_, metadata) = geojson_t {
        assert!(
          metadata.time_data.is_some(),
          "GeoJSON-T when object should parse"
        );
        if let Some(time_data) = &metadata.time_data {
          assert!(
            time_data.time_span.is_some(),
            "GeoJSON-T when should have time span"
          );
          if let Some(time_span) = &time_data.time_span {
            assert!(
              time_span.begin.is_some(),
              "GeoJSON-T when should have begin time"
            );
            assert!(
              time_span.end.is_some(),
              "GeoJSON-T when should have end time"
            );
          }
        }
      }
    } else {
      panic!("First event should be a Layer event");
    }
  }
}
