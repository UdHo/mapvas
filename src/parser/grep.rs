use std::str::FromStr;

use log::{debug, error};
use regex::{Regex, RegexBuilder};

use crate::map::{
  coordinates::Coordinate,
  map_event::{Color, FillStyle, Layer, MapEvent, Shape},
};

use super::Parser;

#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug)]
pub struct GrepParser {
  invert_coordinates: bool,
  color: Color,
  fill: FillStyle,
  color_re: Regex,
  fill_re: Regex,
  coord_re: Regex,
  clear_re: Regex,
  label_re: Option<Regex>,
}

impl Parser for GrepParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    if let Some(event) = self.parse_clear(line) {
      return Some(event);
    }

    let mut layer = Layer::new("test".to_string());

    for l in line.split('\n') {
      self.parse_color(l);
      self.parse_fill(l);
      let label = self.parse_label(l);
      let coordinates = self.parse_shape(l);
      match coordinates.len() {
        0 => (),
        1 => {
          layer.shapes.push(
            Shape::new(coordinates)
              .with_color(self.color)
              .with_fill(FillStyle::Solid)
              .with_label(label),
          );
        }
        _ => {
          layer.shapes.push(
            Shape::new(coordinates)
              .with_color(self.color)
              .with_fill(self.fill)
              .with_label(label),
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
  /// # Panics
  /// If there is a typo in some regex.
  pub fn new(invert_coordinates: bool) -> Self {
    let color_re =
      RegexBuilder::new(r"\b(?:)?(darkBlue|blue|darkRed|red|darkGreen|green|darkYellow|yellow|Black|White|darkGrey|dark|Brown)\b")
        .case_insensitive(true)
        .build()
        .unwrap();
    let fill_re = RegexBuilder::new(r"(solid|transparent|nofill)")
      .case_insensitive(true)
      .build()
      .unwrap();
    let coord_re = Regex::new(r"(-?\d*\.\d*), ?(-?\d*\.\d*)").unwrap();
    let clear_re = RegexBuilder::new("clear")
      .case_insensitive(true)
      .build()
      .unwrap();

    Self {
      invert_coordinates,
      color: Color::default(),
      fill: FillStyle::default(),
      color_re,
      fill_re,
      coord_re,
      clear_re,
      label_re: None,
    }
  }

  #[must_use]
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  /// # Panics
  /// If the given regex is invalid.
  #[must_use]
  pub fn with_label_pattern(mut self, label_pattern: &str) -> Self {
    self.label_re = Some(Regex::new(label_pattern).expect("Cannot build label regex."));
    self
  }

  fn parse_clear(&self, line: &str) -> Option<MapEvent> {
    self.clear_re.is_match(line).then_some(MapEvent::Clear)
  }

  fn parse_color(&mut self, line: &str) {
    for (_, [color]) in self.color_re.captures_iter(line).map(|c| c.extract()) {
      let _ = Color::from_str(color)
        .map(|parsed_color| self.color = parsed_color)
        .map_err(|()| error!("Failed parsing {}", color));
    }
  }

  fn parse_fill(&mut self, line: &str) {
    for (_, [fill]) in self.fill_re.captures_iter(line).map(|c| c.extract()) {
      let _ = FillStyle::from_str(fill)
        .map(|parsed_fill| self.fill = parsed_fill)
        .map_err(|()| error!("Failed parsing {}", fill));
    }
  }

  fn parse_shape(&self, line: &str) -> Vec<Coordinate> {
    let mut coordinates = vec![];
    for (_, [lat, lon]) in self.coord_re.captures_iter(line).map(|c| c.extract()) {
      if let Some(coord) = self.parse_coordinate(lat, lon) {
        coordinates.push(coord);
      }
    }
    coordinates
  }

  fn parse_label(&self, line: &str) -> Option<String> {
    if let Some(re) = &self.label_re {
      let res: Vec<_> = re
        .captures_iter(line)
        .map(|c| c.extract::<1>().1[0])
        .collect();

      if res.is_empty() {
        None
      } else {
        Some(res.join(" | "))
      }
    } else {
      None
    }
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
    coordinates.is_valid().then_some(coordinates)
  }
}
