use egui::Color32;
use log::error;
use serde::Deserialize;

use crate::map::{
  coordinates::{Coordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata, Style},
  map_event::{Color, Layer, MapEvent},
};

use super::Parser;

#[derive(Debug)]
pub struct TTJsonParser {
  data: String,
  color: Color,
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
              .map(Coordinate::as_pixel_position)
              .collect()
          })
          .enumerate()
          .map(|(i, points)| {
            Geometry::LineString(
              points,
              Metadata {
                label: Some(format!("Leg {i}")),
                style: Some(Style::default().with_color(Into::<Color32>::into(self.color))),
              },
            )
          })
          .collect(),
        Metadata::default(),
      ));
    }

    let mut layer = Layer::new("Routes".to_string());
    layer.geometries = vec![Geometry::GeometryCollection(res, Metadata::default())];
    MapEvent::Layer(layer)
  }

  fn convert_range(
    &self,
    center: Option<WGS84Coordinate>,
    boundary: &[WGS84Coordinate],
  ) -> MapEvent {
    let mut geometries = vec![Geometry::Polygon(
      boundary.iter().map(Coordinate::as_pixel_position).collect(),
      Metadata {
        label: Some("Range boundary".into()),
        style: Some(
          Style::default()
            .with_color(Into::<Color32>::into(self.color))
            .with_fill_color(Into::<Color32>::into(self.color).gamma_multiply(0.4)),
        ),
      },
    )];
    if let Some(c) = center {
      geometries.push(Geometry::Polygon(
        vec![c.as_pixel_position()],
        Metadata {
          label: Some("Range center".into()),
          style: Some(
            Style::default()
              .with_color(Into::<Color32>::into(self.color))
              .with_fill_color(Into::<Color32>::into(self.color).gamma_multiply(0.4)),
          ),
        },
      ));
    }
    let mut layer = Layer::new("Range".to_string());
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
  fn finalize(&self) -> Option<MapEvent> {
    let routes: Result<JsonResponse, _> = serde_json::from_str(&self.data);
    match routes {
      Err(e) => {
        error!("{:?}", e);
        None
      }
      Ok(JsonResponse::Routes { routes }) => Some(self.convert_routes(&routes)),
      Ok(JsonResponse::Range { polygon }) => {
        Some(self.convert_range(polygon.center, &polygon.boundary))
      }
    }
  }
}
