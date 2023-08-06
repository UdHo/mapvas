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

#[derive(Default, Copy, Clone, Debug)]
pub struct GrepParser {
  invert_coordinates: bool,
  color: Color,
  fill: FillStyle,
}

impl Parser for GrepParser {
  fn parse_line(&mut self, line: &String) -> Option<MapEvent> {
    self.parse_color(line);
    self.parse_fill(line);
    let coordinates = self.parse_shape(line);
    match coordinates.len() {
      0 => None,
      1 => {
        let mut layer = Layer::new("test".to_string());
        layer.shapes.push(
          Shape::new(coordinates)
            .with_color(self.color)
            .with_fill(FillStyle::Solid),
        );
        Some(MapEvent::Layer(layer))
      }
      _ => {
        let mut layer = Layer::new("test".to_string());
        layer.shapes.push(
          Shape::new(coordinates)
            .with_color(self.color)
            .with_fill(self.fill),
        );
        Some(MapEvent::Layer(layer))
      }
    }
  }
}

impl GrepParser {
  pub fn new(invert_coordinates: bool) -> Self {
    Self {
      invert_coordinates,
      color: Color::default(),
      fill: FillStyle::default(),
    }
  }
  fn parse_color(&mut self, line: &String) {
    let color_re = RegexBuilder::new(r"(Blue|Red|Green|Yellow|Black|White|Grey|Brown)")
      .case_insensitive(true)
      .build()
      .unwrap();
    for (_, [color]) in color_re.captures_iter(&line).map(|c| c.extract()) {
      let _ = Color::from_str(color)
        .map(|parsed_color| self.color = parsed_color)
        .map_err(|_| error!("Failed parsing {}", color));
    }
  }

  fn parse_fill(&mut self, line: &String) {
    let fill_re = RegexBuilder::new(r"(solid|transparent|nofill)")
      .case_insensitive(true)
      .build()
      .unwrap();
    for (_, [fill]) in fill_re.captures_iter(&line).map(|c| c.extract()) {
      let _ = FillStyle::from_str(fill)
        .map(|parsed_fill| self.fill = parsed_fill)
        .map_err(|_| error!("Failed parsing {}", fill));
    }
  }

  fn parse_shape(&self, line: &String) -> Vec<Coordinate> {
    let coord_re = Regex::new(r"(-?\d*\.\d*), ?(-?\d*\.\d*)").unwrap();

    let mut coordinates = vec![];
    for (_, [lat, lon]) in coord_re.captures_iter(&line).map(|c| c.extract()) {
      match self.parse_coordinate(lat, lon) {
        Some(coord) => coordinates.push(coord),
        None => (),
      }
    }
    return coordinates;
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
