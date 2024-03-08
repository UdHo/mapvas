use log::error;
use serde::Deserialize;

use crate::{
  map::{
    coordinates::Coordinate,
    map_event::{Color, FillStyle, Layer, Shape},
  },
  MapEvent,
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

  fn convert_routes(&self, routes: &[Route]) -> crate::MapEvent {
    let colors = Color::all();
    let color_offset = colors.iter().position(|&c| c == self.color).unwrap_or(0);
    let mut shapes: Vec<Shape> = vec![];
    for (r_index, route) in routes.iter().enumerate() {
      for (l_index, leg) in route.legs.iter().enumerate() {
        shapes.push(
          Shape::new(leg.points.clone())
            .with_color(colors[(color_offset + 2 * r_index + (l_index % 2)) % colors.len()]),
        );
      }
    }

    let mut layer = Layer::new("Routes".to_string());
    layer.shapes = shapes;
    MapEvent::Layer(layer)
  }

  fn convert_range(&self, center: Option<Coordinate>, boundary: Vec<Coordinate>) -> MapEvent {
    let mut shapes = vec![Shape::new(boundary)
      .with_color(self.color)
      .with_fill(FillStyle::Transparent)];
    if let Some(c) = center {
      shapes.push(
        Shape::new(vec![c])
          .with_color(self.color)
          .with_fill(FillStyle::Solid),
      );
    }
    let mut layer = Layer::new("Range".to_string());
    layer.shapes = shapes;
    MapEvent::Layer(layer)
  }
}

#[derive(Deserialize, Debug)]
struct Leg {
  points: Vec<Coordinate>,
}

#[derive(Deserialize, Debug)]
struct Route {
  legs: Vec<Leg>,
}

#[derive(Deserialize, Debug)]
struct Polygon {
  center: Option<Coordinate>,
  boundary: Vec<Coordinate>,
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
  fn parse_line(&mut self, line: &str) -> Option<crate::MapEvent> {
    self.data += line;
    None
  }
  fn finalize(&self) -> Option<crate::MapEvent> {
    let routes: Result<JsonResponse, _> = serde_json::from_str(&self.data);
    match routes {
      Err(e) => {
        error!("{:?}", e);
        None
      }
      Ok(JsonResponse::Routes { routes }) => Some(self.convert_routes(&routes)),
      Ok(JsonResponse::Range { polygon }) => {
        Some(self.convert_range(polygon.center, polygon.boundary))
      }
    }
  }
}
