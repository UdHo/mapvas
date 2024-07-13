use std::collections::BTreeMap;
use std::iter::{empty, once};

use serde::{Deserialize, Serialize};

type GeojsonCoordinate = [f64; 2];

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum Geometry {
  Point {
    coordinates: GeojsonCoordinate,
  },
  LineString {
    coordinates: Vec<GeojsonCoordinate>,
  },
  Polygon {
    coordinates: Vec<Vec<GeojsonCoordinate>>,
  },
  MultiPoint {
    coordinates: Vec<GeojsonCoordinate>,
  },
  MultiLineString {
    coordinates: Vec<Vec<GeojsonCoordinate>>,
  },
  MultiPolygon {
    coordinates: Vec<Vec<Vec<GeojsonCoordinate>>>,
  },
  GeometryCollection {
    geometries: Vec<Geometry>,
  },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(untagged)]
enum Value {
  Float(f64),
  Int(i64),
  String(String),
  Map(BTreeMap<String, Value>),
  List(Vec<Value>),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct Properties {
  #[serde(skip_serializing_if = "Option::is_none")]
  id: Option<String>,
  #[serde(flatten, skip_serializing_if = "BTreeMap::is_empty")]
  other: BTreeMap<String, Value>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default, Copy, Clone)]
#[serde(rename_all = "lowercase")]
pub enum FillRule {
  #[default]
  NonZero,
  EvenOdd,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Style {
  #[serde(skip_serializing_if = "Option::is_none")]
  stroke: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  stroke_width: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  fill: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  fill_opacity: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  fill_rule: Option<FillRule>,
}

impl Style {
  pub fn merge_base_style(mut self, base_style: &Style) {
    self.stroke = self.stroke.or(base_style.stroke.clone());
    self.stroke_width = self.stroke_width.or(base_style.stroke_width.clone());
    self.fill = self.fill.or(base_style.fill.clone());
    self.fill_opacity = self.fill_opacity.or(base_style.fill_opacity);
    self.fill_rule = self.fill_rule.or(base_style.fill_rule);
  }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub struct Feature {
  geometry: Geometry,
  #[serde(default)]
  properties: Properties,
  #[serde(skip_serializing_if = "Option::is_none")]
  style: Option<Style>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct FeatureCollection {
  features: Vec<Feature>,
  #[serde(skip_serializing_if = "Option::is_none")]
  style: Option<Style>,
}

impl FeatureCollection {
  pub fn features(&self) -> impl Iterator<Item = &Feature> {
    self.features.iter()
  }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum GeoJson {
  Feature(Feature),
  FeatureCollection(FeatureCollection),
  Geometry(Geometry),
}

impl GeoJson {
  #[must_use]
  pub fn features(&self) -> Box<dyn Iterator<Item = &Feature> + '_> {
    match self {
      Self::Feature(f) => Box::new(once(f)),
      Self::FeatureCollection(fc) => Box::new(fc.features()),
      Self::Geometry(_) => Box::new(empty()),
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use rstest::rstest;
  use std::{fs, path::PathBuf};

  #[test]
  #[expect(clippy::float_cmp)]
  fn test_coordinate() {
    let c = [1.0, 2.0];
    let serialized = serde_json::to_string(&c).expect("Serialization failed.");
    assert_eq!(serialized, "[1.0,2.0]");
    let deserialized: GeojsonCoordinate =
      serde_json::from_str(&serialized).expect("Deserialization failed.");
    assert_eq!(c, deserialized);
  }
  #[test]
  fn test_polygon() {
    let p = Geometry::Polygon {
      coordinates: vec![vec![[1.0, 2.0], [3.0, 4.0]]],
    };
    let serialized = serde_json::to_string(&p).expect("Serialization failed.");
    println!("{serialized}");
    assert_eq!(
      serialized,
      r#"{"type":"Polygon","coordinates":[[[1.0,2.0],[3.0,4.0]]]}"#
    );

    let deserialized: Geometry =
      serde_json::from_str(&serialized).expect("Deserialization failed.");
    assert_eq!(p, deserialized);

    let int_deserialized: Geometry =
      serde_json::from_str(r#"{"type":"Polygon","coordinates":[[[1.0,2.0],[3.0,4.0]]]}"#)
        .expect("Deserialization failed.");
    assert_eq!(p, int_deserialized);
  }

  #[test]
  fn test_feature() {
    let ser = r#"{
      "type": "Feature",
      "geometry": {
        "type": "LineString",
        "coordinates": [
          [102.0, 0.0],
          [103.0, 1.0],
          [104.0, 0.0],
          [105.0, 1.0]
        ]
      },
      "properties": {
        "prop0": "value0"
      }
    }"#;
    let f: Feature = serde_json::from_str(ser).expect("Cannot parse.");
    assert_eq!(
      f,
      Feature {
        geometry: Geometry::LineString {
          coordinates: vec![[102.0, 0.0], [103.0, 1.0], [104.0, 0.0], [105.0, 1.0]]
        },
        properties: Properties {
          id: None,
          other: [("prop0".to_string(), Value::String("value0".to_string()))].into()
        },
        style: None
      }
    );
  }

  #[rstest]
  #[case(Geometry::Point{coordinates:[1.0,2.0]}, r#"{"type":"Point","coordinates":[1.0,2.0]}"#)]
  #[case(Geometry::LineString{coordinates: vec![[1.0, 2.0], [3.0, 4.0]]}, r#"{"type":"LineString","coordinates":[[1.0,2.0],[3.0,4.0]]}"#)]
  #[case(Geometry::Polygon{coordinates: vec![vec![[1.0, 2.0], [3.0, 4.0], [5.0,6.0]]]},r#"{"type":"Polygon","coordinates":[[[1.0,2.0],[3.0,4.0],[5.0,6.0]]]}"#)]
  #[case(Geometry::MultiPoint{coordinates: vec![[1.0, 2.0], [3.0, 4.0]]}, r#"{"type":"MultiPoint","coordinates":[[1.0,2.0],[3.0,4.0]]}"#)]
  #[case(Geometry::MultiLineString{coordinates:vec![vec![[1.0,2.0],[3.0,4.0]],vec![[0.0,1.0],[1.0,2.0]]]}, r#"{"type":"MultiLineString","coordinates":[[[1.0,2.0],[3.0,4.0]],[[0.0,1.0],[1.0,2.0]]]}"#)]
  #[case(Geometry::MultiPolygon{coordinates:vec![vec![vec![[1.0,2.0],[3.0,4.0]]],vec![vec![[0.0,1.0],[1.0,2.0]]]]}, r#"{"type":"MultiPolygon","coordinates":[[[[1.0,2.0],[3.0,4.0]]],[[[0.0,1.0],[1.0,2.0]]]]}"#)]
  #[case(Geometry::GeometryCollection { geometries: vec![
                                   Geometry::Point{coordinates:[1.0,2.0]},
                                   Geometry::LineString{coordinates: vec![[1.0, 2.0], [3.0, 4.0]]},
                                   Geometry::Polygon{coordinates: vec![vec![[1.0, 2.0], [3.0, 4.0], [5.0,6.0]]]}]},
     r#"{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1.0,2.0]},{"type":"LineString","coordinates":[[1.0,2.0],[3.0,4.0]]},{"type":"Polygon","coordinates":[[[1.0,2.0],[3.0,4.0],[5.0,6.0]]]}]}"#)]
  fn geometry_geojson_test(#[case] geometry: Geometry, #[case] serialized: &str) {
    let serialized_geometry = serde_json::to_string(&geometry).expect("Serialization failed.");
    println!("{serialized_geometry}");
    assert_eq!(serialized_geometry, serialized);
    let deserialized: Geometry =
      serde_json::from_str(&serialized_geometry).expect("Deserialization failed.");
    assert_eq!(geometry, deserialized);
  }
  #[test]
  fn geojson_test2() {
    let geojson = GeoJson::FeatureCollection(FeatureCollection {
      features: vec![Feature {
        geometry: Geometry::Point {
          coordinates: [1.0, 2.0],
        },
        properties: Properties {
          id: Some("1".into()),
          ..Default::default()
        },
        style: None,
      }],
      style: None,
    });
    let serialized = serde_json::to_string(&geojson).expect("Serialization failure.");
    println!("{serialized}");
    assert_eq!(
      serialized,
      r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":{"id":"1"}}]}"#
    );
  }

  #[test]
  fn geojson_deserialize_test() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("resources/test/geojson_test.geojson");

    let geojson_source = fs::read_to_string(path).expect("Test resource not available.");
    let parsed: GeoJson = serde_json::from_str(&geojson_source).expect("Cannot parse geojson.");

    let should_be = GeoJson::FeatureCollection(FeatureCollection {
      features: vec![
        Feature {
          geometry: Geometry::Point {
            coordinates: [102.0, 0.5],
          },
          properties: Properties {
            id: None,
            other: [("prop0".into(), Value::String("value0".into()))].into(),
          },
          style: None,
        },
        Feature {
          geometry: Geometry::LineString {
            coordinates: vec![[102.0, 0.0], [103.0, 1.0], [104.0, 0.0], [105.0, 1.0]],
          },
          properties: Properties {
            id: None,
            other: [
              ("prop0".to_string(), Value::String("value0".to_string())),
              ("prop1".to_string(), Value::Float(0.0)),
            ]
            .into(),
          },
          style: None,
        },
        Feature {
          geometry: Geometry::Polygon {
            coordinates: vec![vec![
              [100.0, 0.0],
              [101.0, 0.0],
              [101.0, 1.0],
              [100.0, 1.0],
              [100.0, 0.0],
            ]],
          },
          properties: Properties {
            id: None,
            other: [
              ("prop0".to_string(), Value::String("value0".into())),
              (
                "prop1".to_string(),
                Value::Map([("this".into(), Value::String("that".into()))].into()),
              ),
            ]
            .into(),
          },
          style: None,
        },
      ],
      style: None,
    });

    assert_eq!(parsed, should_be);
  }

  #[test]
  fn geojson_serialization_test() {
    let geojson = GeoJson::FeatureCollection(FeatureCollection {
      features: vec![Feature {
        geometry: Geometry::Point {
          coordinates: [102.0, 0.5],
        },
        properties: Properties {
          id: Some("a".into()),
          ..Default::default()
        },
        style: None,
      }],
      style: None,
    });
    let serialized = serde_json::to_string(&geojson).expect("Can serialize.");
    println!("{serialized}");
    assert_eq!(
      serialized,
      r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[102.0,0.5]},"properties":{"id":"a"}}]}"#
    );
  }

  #[test]
  fn geojson_style_deserialize_test() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("resources/test/geojson_style_test.geojson");

    let geojson_source = fs::read_to_string(path).expect("Test resource not available.");
    let parsed: GeoJson = serde_json::from_str(&geojson_source).expect("Cannot parse geojson.");
    let features: Vec<_> = parsed.features().collect();
    assert_eq!(
      features[0].style.as_ref().unwrap().stroke,
      Some("green".to_string())
    );
  }
}
