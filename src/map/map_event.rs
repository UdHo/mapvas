use super::coordinates::{Coordinate, Tile};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum Color {
  #[default]
  Blue,
  Red,
  Green,
  Yellow,
  Black,
  White,
  Grey,
  Brown,
}

impl Color {
  pub fn to_rgb(self) -> femtovg::Color {
    match self {
      Color::Blue => femtovg::Color::rgb(0, 0, 255),
      Color::Red => femtovg::Color::rgb(255, 0, 0),
      Color::Green => femtovg::Color::rgb(0, 255, 0),
      Color::Yellow => femtovg::Color::rgb(255, 255, 0),
      Color::Black => femtovg::Color::rgb(0, 0, 0),
      Color::White => femtovg::Color::rgb(255, 255, 255),
      Color::Grey => femtovg::Color::rgb(127, 127, 127),
      Color::Brown => femtovg::Color::rgb(153, 76, 0),
    }
  }
}

impl FromStr for Color {
  type Err = ();
  fn from_str(input: &str) -> Result<Color, Self::Err> {
    let lowercase = input.to_lowercase();
    match lowercase.as_str() {
      "blue" => Ok(Color::Blue),
      "red" => Ok(Color::Red),
      "green" => Ok(Color::Green),
      "yellow" => Ok(Color::Yellow),
      "black" => Ok(Color::Black),
      "white" => Ok(Color::White),
      "grey" => Ok(Color::Grey),
      "brown" => Ok(Color::Brown),
      _ => Err(()),
    }
  }
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum FillStyle {
  #[default]
  NoFill,
  Transparent,
  Solid,
}

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Style {
  pub color: Color,
  pub fill: FillStyle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
  pub coordinates: Vec<Coordinate>,
  pub style: Style,
  pub visible: bool,
}

impl Shape {
  pub fn new(coordinates: Vec<Coordinate>) -> Self {
    Shape {
      coordinates,
      style: Style::default(),
      visible: true,
    }
  }

  pub fn with_color(mut self, color: Color) -> Self {
    self.style.color = color;
    self
  }

  pub fn with_fill(mut self, fill: FillStyle) -> Self {
    self.style.fill = fill;
    self
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
  pub id: String,
  pub shapes: Vec<Shape>,
}

impl Layer {
  pub fn new(id: String) -> Self {
    Layer { id, shapes: vec![] }
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MapEvent {
  Shutdown,
  TileDataArrived { tile: Tile, data: Vec<u8> },
  Layer(Layer),
}
