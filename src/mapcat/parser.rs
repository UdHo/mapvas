use std::str::FromStr;

use log::{debug, error};
use mapvas::{
  map::{
    coordinates::Coordinate,
    map_event::{Color, FillStyle, Layer, Shape},
  },
  MapEvent,
};
use rand::{rngs::ThreadRng, Rng};
use regex::{Regex, RegexBuilder};

/// An interface for input parsers.
pub trait Parser {
  /// Handles the next line of the input. Returns optionally a map event that can be send.
  /// * `line` - The next line to parse.
  fn parse_line(&mut self, line: &String) -> Option<MapEvent>;
  /// Is called when the complete input has been parsed.
  /// For parses that parse a complete document, e.g. a json parser it returns the result.
  fn finalize(&self) -> Option<MapEvent> {
    None
  }
}

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

pub struct RandomParser {
  rng: ThreadRng,
  coordinate: Coordinate,
}
impl RandomParser {
  pub fn new() -> Self {
    let mut rng = rand::thread_rng();
    let coordinate = Self::rand_coordinate(&mut rng);
    Self { rng, coordinate }
  }

  fn rand_coordinate(rng: &mut ThreadRng) -> Coordinate {
    Coordinate {
      lat: rng.gen_range(-80.0..80.0),
      lon: rng.gen_range(-179.0..179.0),
    }
  }

  fn rand_move(&mut self) -> Coordinate {
    let new_coord = Coordinate {
      lat: (self.coordinate.lat + self.rng.gen_range(-1.0..1.0)).clamp(-80., 80.),
      lon: (self.coordinate.lon + self.rng.gen_range(-1.0..1.0)).clamp(-179., 179.),
    };
    self.coordinate = new_coord;
    new_coord
  }
}
impl Parser for RandomParser {
  fn parse_line(&mut self, line: &String) -> Option<MapEvent> {
    let steps = match line.trim().parse::<u32>() {
      Ok(s) => s,
      Err(_) => 1,
    };
    let mut layer = Layer::new("test".to_string());

    for _ in 0..=steps {
      let length = self.rng.gen_range(0..10);
      let mut coordinates = vec![self.coordinate];
      for _ in 1..length {
        coordinates.push(self.rand_move());
      }
      let colors = vec![
        Color::Blue,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Black,
        Color::White,
        Color::Grey,
        Color::Brown,
      ];

      let color = colors[self.rng.gen_range(0..colors.len())];

      let fill = match self.rng.gen_range(0.0..1.0) {
        p if p < 0.8 => FillStyle::NoFill,
        _ => FillStyle::Transparent,
      };

      layer
        .shapes
        .push(Shape::new(coordinates).with_color(color).with_fill(fill));
    }
    Some(MapEvent::Layer(layer))
  }
}
