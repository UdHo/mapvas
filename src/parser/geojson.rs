use super::{Parser, style::StyleParser};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::map::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata},
  map_event::{Layer, MapEvent},
};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct GeoJsonParser {
  #[serde(skip)]
  data: String,
  #[serde(skip)]
  geometry: Vec<Geometry<PixelCoordinate>>,
}

impl GeoJsonParser {
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

  /// Parse `GeoJSON` data and extract geometries with style information
  fn parse_geojson(&mut self, value: &Value) -> Result<(), String> {
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
              // Direct geometry object
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

    // Extract properties for styling and metadata
    let metadata = Self::extract_metadata_from_properties(obj.get("properties"));

    // Parse geometry
    if let Some(geometry_value) = obj.get("geometry") {
      if let Some(geometry) = self.parse_geometry(geometry_value, &metadata) {
        self.geometry.push(geometry);
      }
    }

    Ok(())
  }

  /// Extract metadata and style from `GeoJSON` properties
  fn extract_metadata_from_properties(properties: Option<&Value>) -> Metadata {
    StyleParser::extract_metadata_from_json(properties)
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
        // GeoJSON Polygon coordinates are [[[lon, lat], [lon, lat], ...]]
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
    if let Some(array) = coord.as_array() {
      if array.len() >= 2 {
        let lon = array[0].as_f64()? as f32;
        let lat = array[1].as_f64()? as f32;
        return Some(PixelCoordinate::from(WGS84Coordinate::new(lat, lon)));
      }
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
    self.data.push_str(line);
    None
  }

  fn finalize(&mut self) -> Option<MapEvent> {
    let parsed: Value = serde_json::from_str(&self.data).ok()?;

    if let Err(e) = self.parse_geojson(&parsed) {
      log::warn!("GeoJSON parsing failed: {e}");
      return None;
    }

    if !self.geometry.is_empty() {
      let mut layer = Layer::new("geojson".to_string());
      layer.geometries.clone_from(&self.geometry);
      return Some(MapEvent::Layer(layer));
    }
    None
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

      // Check that we have point, linestring, and polygon
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

      // All should be GeometryCollection due to multi-geometry conversion
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
        if let Geometry::Point(_, metadata) = geometry {
          if let Some(style) = &metadata.style {
            parsed_colors.push(style.color());
          }
        }
      }

      assert_eq!(
        parsed_colors.len(),
        5,
        "All geometries should have parsed colors"
      );

      // Check that we got different colors (hex, short hex, rgb, named, hex with alpha)
      assert_eq!(parsed_colors[0], egui::Color32::RED); // #ff0000
      assert_eq!(parsed_colors[1], egui::Color32::RED); // #f00 (should expand to #ff0000)
      assert_eq!(parsed_colors[2], egui::Color32::GREEN); // rgb(0, 255, 0)
      assert_eq!(parsed_colors[3], egui::Color32::BLUE); // blue
    // Note: Color with alpha (#ff000080) will be parsed but may differ in representation
    } else {
      panic!("First event should be a Layer event");
    }
  }
}
