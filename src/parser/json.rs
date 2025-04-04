use super::Parser;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::map::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::Geometry,
  map_event::{Layer, MapEvent},
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

  fn find_coordinates(&mut self, v: &Value) -> Option<PixelCoordinate> {
    match v {
      Value::Array(vec) => {
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
    self.data.push_str(line);
    None
  }

  fn finalize(&mut self) -> Option<MapEvent> {
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
  use crate::parser::Parser as _;

  #[test]
  fn test_json_parser() {
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
}
