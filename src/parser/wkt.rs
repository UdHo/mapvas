use log::debug;
use regex::{Regex, RegexBuilder};

use crate::map::{
  coordinates::Coordinate,
  map_event::{Color, Layer, MapEvent, Shape},
};

use super::Parser;

#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug)]
pub struct Wkt {
  color: Color,
  point_re: Regex,
  polyline_re: Regex,
  coord_re: Regex,
}

impl Parser for Wkt {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    let mut layer = Layer::new("wkt".to_string());
    for (_, [cont]) in self.point_re.captures_iter(line).map(|c| c.extract()) {
      for (_, [lon, lat]) in self.coord_re.captures_iter(cont).map(|c| c.extract()) {
        if let Some(coord) = Self::parse_coordinate(lat, lon) {
          layer.shapes.push(Shape::new(vec![coord]));
        }
      }
    }
    for (_, [cont]) in self.polyline_re.captures_iter(line).map(|c| c.extract()) {
      let mut coordinates = vec![];
      for (_, [lon, lat]) in self.coord_re.captures_iter(cont).map(|c| c.extract()) {
        if let Some(coord) = Self::parse_coordinate(lat, lon) {
          coordinates.push(coord);
        }
      }
      layer.shapes.push(Shape::new(coordinates));
    }

    (!layer.shapes.is_empty()).then_some(MapEvent::Layer(layer))
  }

  fn set_color(&mut self, color: Color) {
    self.color = color;
  }
}

impl Wkt {
  /// Creates a new [`Wkt`] parser.
  ///
  /// # Panics
  ///
  /// Panics if regex got screwed up.
  #[must_use]
  pub fn new() -> Self {
    let point_re = RegexBuilder::new(r"POINT\s*\(\s*(.*)\s*\)")
      .build()
      .unwrap();

    let polyline_re = RegexBuilder::new(r"LINESTRING\s*\(\s*(.*)\s*\)")
      .build()
      .unwrap();

    let coord_re = Regex::new(r"(-?\d*\.\d*) ?(-?\d*\.\d*)").unwrap();

    Self {
      color: Color::default(),
      point_re,
      polyline_re,
      coord_re,
    }
  }

  fn parse_coordinate(x: &str, y: &str) -> Option<Coordinate> {
    let lat = match x.parse::<f32>() {
      Ok(v) => v,
      Err(e) => {
        debug!("Could not parse {} {:?}.", x, e);
        return None;
      }
    };
    let lon = match y.parse::<f32>() {
      Ok(v) => v,
      Err(e) => {
        debug!("Could not parse {} {:?}", y, e);
        return None;
      }
    };
    let coordinates = Coordinate { lat, lon };
    coordinates.is_valid().then_some(coordinates)
  }
}

impl Default for Wkt {
  fn default() -> Self {
    Self::new()
  }
}
