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

impl TTJsonParser {
  pub fn new() -> Self {
    Self {
      data: String::new(),
      color: Color::default(),
    }
  }

  pub fn with_color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  fn convert_routes(&self, routes: &Vec<Route>) -> Option<crate::MapEvent> {
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
    Some(MapEvent::Layer(layer))
  }

  fn convert_range(
    &self,
    center: Option<Coordinate>,
    boundary: Vec<Coordinate>,
  ) -> Option<MapEvent> {
    let mut shapes = vec![Shape::new(boundary)
      .with_color(self.color)
      .with_fill(FillStyle::Transparent)];
    center.map(|c| {
      shapes.push(
        Shape::new(vec![c])
          .with_color(self.color)
          .with_fill(FillStyle::Solid),
      )
    });
    let mut layer = Layer::new("Range".to_string());
    layer.shapes = shapes;
    Some(MapEvent::Layer(layer))
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
  fn parse_line(&mut self, line: &String) -> Option<crate::MapEvent> {
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
      Ok(JsonResponse::Routes { routes }) => self.convert_routes(&routes),
      Ok(JsonResponse::Range { polygon }) => self.convert_range(polygon.center, polygon.boundary),
    }
  }
}
