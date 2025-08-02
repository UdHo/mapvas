use super::{Parser, style::StyleParser};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
  map::{
    coordinates::{PixelCoordinate, WGS84Coordinate},
    geometry_collection::Geometry,
    map_event::{Layer, MapEvent},
  },
  profile_scope,
};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct JsonParser {
  #[serde(skip)]
  data: String,
  #[serde(skip)]
  geometry: Vec<Geometry<PixelCoordinate>>,
}

impl JsonParser {
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

  #[expect(clippy::cast_possible_truncation)]
  fn find_coordinates(&mut self, v: &Value) -> Option<PixelCoordinate> {
    profile_scope!("JsonParser::find_coordinates");
    match v {
      Value::Array(vec) => {
        if vec.len() == 2 {
          if let (Some(lon), Some(lat)) = (vec[0].as_f64(), vec[1].as_f64()) {
            return Some(PixelCoordinate::from(WGS84Coordinate::new(
              lat as f32, lon as f32,
            )));
          }
        }
        let coords = vec
          .iter()
          .filter_map(|v| self.find_coordinates(v))
          .collect::<Vec<_>>();
        Geometry::from_coords(coords)
          .into_iter()
          .for_each(|g| self.geometry.push(g));
      }
      Value::Object(map) => {
        if let Some(coord) = Self::try_get_coordinate_from_obj(map) {
          let metadata = StyleParser::extract_metadata_from_json(Some(v));
          let point = Geometry::Point(coord, metadata);
          self.geometry.push(point);
          return Some(coord);
        }
        for (_, v) in map {
          let _ = self.find_coordinates(v);
        }
      }
      _ => (),
    }

    None
  }

  #[allow(clippy::cast_possible_truncation)]
  fn try_get_coordinate_from_obj(obj: &Map<String, Value>) -> Option<PixelCoordinate> {
    let mut lat = None;
    let mut lon = None;
    for (k, v) in obj {
      if let Some(num) = v.as_f64() {
        if k.starts_with("lat") && (-180. ..=180.).contains(&num) {
          lat = Some(num);
        } else if k.starts_with("lon") && (-90. ..=90.).contains(&num) {
          lon = Some(num);
        }
      }
    }
    if let (Some(lat), Some(lon)) = (lat, lon) {
      return Some(WGS84Coordinate::new(lat as f32, lon as f32).into());
    }
    None
  }
}

impl Parser for JsonParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    profile_scope!("JsonParser::parse_line");
    self.data.push_str(line);
    None
  }

  fn finalize(&mut self) -> Option<MapEvent> {
    profile_scope!("JsonParser::finalize");
    let parsed: Value = serde_json::from_str(&self.data).ok()?;
    self.find_coordinates(&parsed);
    if !self.geometry.is_empty() {
      let mut layer = Layer::new("json".to_string());
      layer.geometries.clone_from(&self.geometry);
      return Some(MapEvent::Layer(layer));
    }
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::map::{geometry_collection::Geometry, map_event::MapEvent};
  use crate::parser::{FileParser, test_utils::*};
  use std::fs::File;
  use std::io::BufReader;

  #[test]
  fn test_json_simple_coordinates_from_file() {
    let events = load_and_parse::<JsonParser>("simple_coordinates.json");
    let actual_layer = extract_layer(&events, "json");

    let expected_layer = LayerBuilder::new("json")
      .with_geometry(GeometryBuilder::point(52.0, 10.0))
      .build();

    assert_layer_eq(actual_layer, &expected_layer);
  }

  #[test]
  fn test_json_styled_coordinates_from_file() {
    let events = load_and_parse::<JsonParser>("styled_coordinates.json");
    let actual_layer = extract_layer(&events, "json");

    let expected_style = StyleBuilder::with_colors(egui::Color32::RED, egui::Color32::GREEN);
    let expected_metadata = MetadataBuilder::with_label_and_style("Test Location", expected_style);
    let expected_layer = LayerBuilder::new("json")
      .with_geometry(GeometryBuilder::point_with_metadata(
        52.0,
        10.0,
        expected_metadata,
      ))
      .build();

    assert_layer_eq(actual_layer, &expected_layer);
  }

  #[test]
  fn test_json_multiple_coordinates_from_file() {
    let json_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/multiple_coordinates.json"
    );
    let file = File::open(json_path).expect("Could not open multiple coordinates JSON file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = super::JsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "JSON parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "json", "Layer should be named 'json'");
      assert!(
        layer.geometries.len() >= 4,
        "Should have at least 4 point geometries"
      );

      let mut styled_count = 0;
      let mut labeled_count = 0;

      for geometry in &layer.geometries {
        if let Geometry::Point(_, metadata) = geometry {
          if metadata.label.is_some() {
            labeled_count += 1;
          }
          if metadata.style.is_some() {
            styled_count += 1;
          }
        }
      }

      assert!(labeled_count >= 4, "Should have at least 4 labeled points");
      assert!(styled_count >= 4, "Should have at least 4 styled points");
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_json_color_variations_from_file() {
    let json_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/color_variations.json"
    );
    let file = File::open(json_path).expect("Could not open color variations JSON file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = super::JsonParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "JSON parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.id, "json", "Layer should be named 'json'");
      assert!(
        layer.geometries.len() >= 5,
        "Should have at least 5 geometries with different color formats"
      );

      let mut parsed_colors = Vec::new();
      for geometry in &layer.geometries {
        if let Geometry::Point(_, metadata) = geometry {
          if let Some(style) = &metadata.style {
            parsed_colors.push(style.color());
          }
        }
      }

      assert!(
        parsed_colors.len() >= 5,
        "Should have at least 5 parsed colors"
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
  fn test_legacy_json_parsing() {
    let data = r#"
{
  "x": {
    "y": [
      {
        "leg": {
          "blub": 456,
          "bla": 123,
          "geometry": [
            {
              "latitude": 53.55365,
              "longitude": 10.00512
            },
            {
              "latitude": 53.55365,
              "longitude": 10.00518
            },
            {
              "latitude": 53.55369,
              "longitude": 10.00519
            },
            {
              "latitude": 53.55372,
              "longitude": 10.00519
            }
          ]
        }
      }
    ]
  }
}"#
      .to_string();
    let mut parser = super::JsonParser::default();
    assert_eq!(parser.parse_line(&data), None);
    assert!(parser.finalize().is_some());
  }

  #[test]
  fn test_geojson_linestring_compatibility() {
    let data = r#"
{"type":"LineString","coordinates":[[10.0,53.0],[10.0,54.0],[10.0,53.5],[10.0,50.0]]}"#
      .to_string();
    let mut parser = super::JsonParser::default();
    assert_eq!(parser.parse_line(&data), None);
    assert!(parser.finalize().is_some());
  }
}
