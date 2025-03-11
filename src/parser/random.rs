use rand::{Rng, rngs::ThreadRng};

use crate::map::{
  coordinates::Coordinate,
  map_event::{Color, FillStyle, Layer, MapEvent, Shape},
};

use super::Parser;

#[allow(clippy::module_name_repetitions)]
pub struct RandomParser {
  rng: ThreadRng,
  coordinate: Coordinate,
}

impl Default for RandomParser {
  fn default() -> Self {
    Self::new()
  }
}

impl RandomParser {
  #[must_use]
  pub fn new() -> Self {
    let mut rng = rand::rng();
    let coordinate = Self::rand_coordinate(&mut rng);
    Self { rng, coordinate }
  }

  fn rand_coordinate(rng: &mut ThreadRng) -> Coordinate {
    Coordinate {
      lat: rng.random_range(-80.0..80.0),
      lon: rng.random_range(-179.0..179.0),
    }
  }

  fn rand_move(&mut self) -> Coordinate {
    let new_coord = Coordinate {
      lat: (self.coordinate.lat + self.rng.random_range(-1.0..1.0)).clamp(-80., 80.),
      lon: (self.coordinate.lon + self.rng.random_range(-1.0..1.0)).clamp(-179., 179.),
    };
    self.coordinate = new_coord;
    new_coord
  }
}
impl Parser for RandomParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    let steps = line.trim().parse::<u32>().unwrap_or(1);
    let mut layer = Layer::new("random".to_string());

    for _ in 0..=steps {
      let length = self.rng.random_range(0..10);
      let mut coordinates = vec![self.coordinate];
      for _ in 1..length {
        coordinates.push(self.rand_move());
      }
      let colors = Color::all();
      let color = colors[self.rng.random_range(0..colors.len())];

      let fill = match self.rng.random_range(0.0..1.0) {
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
