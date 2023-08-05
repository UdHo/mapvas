use std::str::FromStr;

use log::debug;
use mapvas::{
  map::{
    coordinates::Coordinate,
    map_event::{Color, Layer, Shape},
  },
  MapEvent,
};
use regex::{Regex, RegexBuilder};
use single_instance::SingleInstance;

#[derive(Default, Copy, Clone)]
pub struct GrepParser {
  invert_coordinates: bool,
  color: Color,
}

impl GrepParser {
  pub fn parse_line(&mut self, line: &String) -> Vec<Coordinate> {
    self.parse_color(line);
    self.parse_shape(line)
  }

  fn parse_color(&mut self, line: &String) {
    let color_re = RegexBuilder::new(r"(Blue|Red|Green|Yellow|Black|White|Grey|Brown)")
      .case_insensitive(true)
      .build()
      .unwrap();
    for (_, [color]) in color_re.captures_iter(&line).map(|c| c.extract()) {
      let _ = Color::from_str(color).map(|parsed_color| self.color = parsed_color);
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

async fn send_path(coordinates: Vec<Coordinate>, color: Color) {
  if coordinates.is_empty() {
    return;
  }

  let mut layer = Layer::new("test".to_string());
  layer.shapes.push(Shape::new(coordinates).with_color(color));

  let event = MapEvent::Layer(layer);
  let _ = surf::post("http://localhost:8080/")
    .body_json(&event)
    .expect("Cannot serialize json")
    .await;
}

async fn spawn_mapvas_if_needed() {
  let check = SingleInstance::new("MapVas").unwrap();
  if !check.is_single() {
    return;
  }
  drop(check);
  let _ = std::process::Command::new("mapvas").spawn();
  while let Err(_) = surf::get("http://localhost:8080/healthcheck").await {}
}

#[tokio::main]
async fn main() {
  env_logger::init();

  spawn_mapvas_if_needed().await;

  let mut tasks = tokio::task::JoinSet::new();
  let mut parser = GrepParser::default();
  let stdin = async_std::io::stdin();
  let mut line = String::new();
  while let Ok(res) = stdin.read_line(&mut line).await {
    if res == 0 {
      break;
    }
    let coords = parser.parse_line(&line);
    line.clear();
    tasks.spawn(async move {
      if !coords.is_empty() {
        send_path(coords, parser.color).await;
      }
    });
  }
  // Waiting for all tasks to finish.
  while let Some(_) = tasks.join_next().await {}
}
