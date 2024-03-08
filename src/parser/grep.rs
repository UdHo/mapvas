use std::str::FromStr;

use log::{debug, error};
use regex::{Regex, RegexBuilder};

use crate::{
  map::{
    coordinates::Coordinate,
    map_event::{Color, FillStyle, Layer, Shape},
  },
  MapEvent,
};

use super::Parser;

#[allow(clippy::module_name_repetitions)]
#[derive(Default, Copy, Clone, Debug)]
pub struct GrepParser {
  invert_coordinates: bool,
  color: Color,
  fill: FillStyle,
}

impl Parser for GrepParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    if let Some(event) = Self::parse_clear(line) {
      return Some(event);
    }

    let mut layer = Layer::new("test".to_string());

    for l in line.split('\n') {
      self.parse_color(l);
      self.parse_fill(l);
      let coordinates = self.parse_shape(l);
      match coordinates.len() {
        0 => (),
        1 => {
          layer.shapes.push(
            Shape::new(coordinates)
              .with_color(self.color)
              .with_fill(FillStyle::Solid),
          );
        }
        _ => {
          layer.shapes.push(
            Shape::new(coordinates)
              .with_color(self.color)
              .with_fill(self.fill),
          );
        }
      }
    }

    if layer.shapes.is_empty() {
      None
    } else {
      Some(MapEvent::Layer(layer))
    }
  }
}

impl GrepParser {
  #[must_use]
  pub fn new(invert_coordinates: bool) -> Self {
    Self {
      invert_coordinates,
      color: Color::default(),
      fill: FillStyle::default(),
    }
  }

  #[must_use]
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  fn parse_clear(line: &str) -> Option<MapEvent> {
    RegexBuilder::new("clear")
      .case_insensitive(true)
      .build()
      .unwrap()
      .is_match(line)
      .then_some(MapEvent::Clear)
  }

  fn parse_color(&mut self, line: &str) {
    let color_re =
      RegexBuilder::new(r"\b(?:)?(darkBlue|blue|darkRed|red|darkGreen|green|darkYellow|yellow|Black|White|darkGrey|dark|Brown)\b")
        .case_insensitive(true)
        .build()
        .unwrap();
    for (_, [color]) in color_re.captures_iter(line).map(|c| c.extract()) {
      let _ = Color::from_str(color)
        .map(|parsed_color| self.color = parsed_color)
        .map_err(|_| error!("Failed parsing {}", color));
    }
  }

  fn parse_fill(&mut self, line: &str) {
    let fill_re = RegexBuilder::new(r"(solid|transparent|nofill)")
      .case_insensitive(true)
      .build()
      .unwrap();
    for (_, [fill]) in fill_re.captures_iter(line).map(|c| c.extract()) {
      let _ = FillStyle::from_str(fill)
        .map(|parsed_fill| self.fill = parsed_fill)
        .map_err(|_| error!("Failed parsing {}", fill));
    }
  }

  fn parse_shape(&self, line: &str) -> Vec<Coordinate> {
    let coord_re = Regex::new(r"(-?\d*\.\d*), ?(-?\d*\.\d*)").unwrap();

    let mut coordinates = vec![];
    for (_, [lat, lon]) in coord_re.captures_iter(line).map(|c| c.extract()) {
      if let Some(coord) = self.parse_coordinate(lat, lon) {
        coordinates.push(coord)
      }
    }
    coordinates
  }

  fn parse_coordinate(&self, x: &str, y: &str) -> Option<Coordinate> {
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
    let mut coordinates = Coordinate { lat, lon };
    if self.invert_coordinates {
      std::mem::swap(&mut coordinates.lat, &mut coordinates.lon);
    }
    match coordinates.is_valid() {
      true => Some(coordinates),
      false => None,
    }
  }
}
