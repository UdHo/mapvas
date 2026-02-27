use egui::Color32;
use log::error;
use serde::{Deserialize, Serialize};

use crate::map::{
  coordinates::{Coordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata, Style},
  map_event::{Color, Layer, MapEvent},
};

use super::Parser;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TTJsonParser {
  #[serde(skip_serializing, default)]
  data: String,
  #[serde(default)]
  color: Color,
  #[serde(skip)]
  layer_name: String,
}

impl Default for TTJsonParser {
  fn default() -> Self {
    Self::new()
  }
}

impl TTJsonParser {
  #[must_use]
  pub fn new() -> Self {
    Self {
      data: String::new(),
      color: Color::default(),
      layer_name: "ttjson".to_string(),
    }
  }

  #[must_use]
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  fn convert_routes(&self, routes: &[Route]) -> MapEvent {
    let mut res = vec![];
    for route in routes {
      res.push(Geometry::GeometryCollection(
        route
          .legs
          .iter()
          .map(|leg| {
            leg
              .points
              .iter()
              .map(Coordinate::as_pixel_coordinate)
              .collect()
          })
          .enumerate()
          .map(|(i, points)| {
            Geometry::LineString(
              points,
              Metadata {
                label: Some(format!("Leg {i}").into()),
                style: Some(Style::default().with_color(Into::<Color32>::into(self.color))),
                heading: None,
                time_data: None,
              },
            )
          })
          .collect(),
        Metadata::default(),
      ));
    }

    let mut layer = Layer::new(self.layer_name.clone());
    layer.geometries = vec![Geometry::GeometryCollection(res, Metadata::default())];
    MapEvent::Layer(layer)
  }

  fn convert_range(
    &self,
    center: Option<WGS84Coordinate>,
    boundary: &[WGS84Coordinate],
  ) -> MapEvent {
    let mut geometries = vec![Geometry::Polygon(
      boundary
        .iter()
        .map(Coordinate::as_pixel_coordinate)
        .collect(),
      Metadata {
        label: Some("Range boundary".into()),
        style: Some(
          Style::default()
            .with_color(Into::<Color32>::into(self.color))
            .with_fill_color(Into::<Color32>::into(self.color).gamma_multiply(0.4)),
        ),
        heading: None,
        time_data: None,
      },
    )];
    if let Some(c) = center {
      geometries.push(Geometry::Polygon(
        vec![c.as_pixel_coordinate()],
        Metadata {
          label: Some("Range center".into()),
          style: Some(
            Style::default()
              .with_color(Into::<Color32>::into(self.color))
              .with_fill_color(Into::<Color32>::into(self.color).gamma_multiply(0.4)),
          ),
          heading: None,
          time_data: None,
        },
      ));
    }
    let mut layer = Layer::new(self.layer_name.clone());
    layer.geometries = geometries;
    MapEvent::Layer(layer)
  }
}

#[derive(Deserialize, Debug)]
struct Leg {
  points: Vec<WGS84Coordinate>,
}

#[derive(Deserialize, Debug)]
struct Route {
  legs: Vec<Leg>,
}

#[derive(Deserialize, Debug)]
struct Polygon {
  center: Option<WGS84Coordinate>,
  boundary: Vec<WGS84Coordinate>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum JsonResponse {
  Routes {
    routes: Vec<Route>,
  },
  Range {
    #[serde(alias = "reachableRange")]
    polygon: Polygon,
  },
}

impl Parser for TTJsonParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    self.data += line;
    None
  }
  fn finalize(&mut self) -> Option<MapEvent> {
    let routes: Result<JsonResponse, _> = serde_json::from_str(&self.data);
    match routes {
      Err(e) => {
        error!("{e:?}");
        None
      }
      Ok(JsonResponse::Routes { routes }) => Some(self.convert_routes(&routes)),
      Ok(JsonResponse::Range { polygon }) => {
        Some(self.convert_range(polygon.center, &polygon.boundary))
      }
    }
  }

  fn set_layer_name(&mut self, layer_name: String) {
    self.layer_name = layer_name;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parse_json(json: &str) -> Option<MapEvent> {
    let mut parser = TTJsonParser::new();
    for line in json.lines() {
      parser.parse_line(line);
    }
    parser.finalize()
  }

  #[test]
  fn parse_line_returns_none_accumulates_data() {
    let mut parser = TTJsonParser::new();
    assert!(parser.parse_line("{").is_none());
    assert!(!parser.data.is_empty());
  }

  #[test]
  fn route_single_leg() {
    let json = std::fs::read_to_string(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/tt_route.json"
    ))
    .unwrap();
    let event = parse_json(&json).unwrap();
    let MapEvent::Layer(layer) = event else {
      panic!("Expected Layer event");
    };
    assert_eq!(layer.geometries.len(), 1);
    if let Geometry::GeometryCollection(routes, _) = &layer.geometries[0] {
      assert_eq!(routes.len(), 1);
      if let Geometry::GeometryCollection(legs, _) = &routes[0] {
        assert_eq!(legs.len(), 1);
      } else {
        panic!("Expected inner GeometryCollection");
      }
    } else {
      panic!("Expected GeometryCollection");
    }
  }

  #[test]
  fn route_multiple_legs() {
    let json = std::fs::read_to_string(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/tt_route_multi_leg.json"
    ))
    .unwrap();
    let event = parse_json(&json).unwrap();
    let MapEvent::Layer(layer) = event else {
      panic!("Expected Layer event");
    };
    if let Geometry::GeometryCollection(routes, _) = &layer.geometries[0] {
      if let Geometry::GeometryCollection(legs, _) = &routes[0] {
        assert_eq!(legs.len(), 2);
        for leg in legs {
          assert!(matches!(leg, Geometry::LineString(_, _)));
        }
      } else {
        panic!("Expected inner GeometryCollection");
      }
    }
  }

  #[test]
  fn range_with_center() {
    let json = std::fs::read_to_string(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/tt_range.json"
    ))
    .unwrap();
    let event = parse_json(&json).unwrap();
    let MapEvent::Layer(layer) = event else {
      panic!("Expected Layer event");
    };
    // boundary polygon + center point
    assert_eq!(layer.geometries.len(), 2);
    assert!(matches!(layer.geometries[0], Geometry::Polygon(_, _)));
    assert!(matches!(layer.geometries[1], Geometry::Polygon(_, _)));
  }

  #[test]
  fn range_without_center() {
    let json = std::fs::read_to_string(concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/tt_range_no_center.json"
    ))
    .unwrap();
    let event = parse_json(&json).unwrap();
    let MapEvent::Layer(layer) = event else {
      panic!("Expected Layer event");
    };
    // Only boundary polygon, no center
    assert_eq!(layer.geometries.len(), 1);
    assert!(matches!(layer.geometries[0], Geometry::Polygon(_, _)));
  }

  #[test]
  fn invalid_json_returns_none() {
    let event = parse_json("this is not json at all");
    assert!(event.is_none());
  }

  #[test]
  fn with_color_builder() {
    let parser = TTJsonParser::new().with_color(Color::from(egui::Color32::RED));
    assert_eq!(parser.color, Color::from(egui::Color32::RED));
  }
}
