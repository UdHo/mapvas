use super::coordinates::{Coordinate, Tile};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};

static ALL_COLORS: [Color; 11] = [
  Color::Blue,
  Color::DarkBlue,
  Color::Red,
  Color::DarkRed,
  Color::Green,
  Color::DarkGreen,
  Color::Black,
  Color::Grey,
  Color::Yellow,
  Color::White,
  Color::Brown,
];

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum Color {
  #[default]
  Blue,
  DarkBlue,
  Red,
  DarkRed,
  Green,
  DarkGreen,
  Yellow,
  DarkYellow,
  Black,
  Grey,
  White,
  Brown,
}

impl Color {
  pub fn to_rgba(self, alpha: u8) -> femtovg::Color {
    match self {
      Color::Blue => femtovg::Color::rgba(0, 0, 255, alpha),
      Color::DarkBlue => femtovg::Color::rgba(0, 0, 150, alpha),
      Color::Red => femtovg::Color::rgba(255, 0, 0, alpha),
      Color::DarkRed => femtovg::Color::rgba(150, 0, 0, alpha),
      Color::Green => femtovg::Color::rgba(0, 255, 0, alpha),
      Color::DarkGreen => femtovg::Color::rgba(0, 150, 0, alpha),
      Color::Yellow => femtovg::Color::rgba(255, 255, 0, alpha),
      Color::DarkYellow => femtovg::Color::rgba(150, 150, 0, alpha),
      Color::Black => femtovg::Color::rgba(0, 0, 0, alpha),
      Color::White => femtovg::Color::rgba(255, 255, 255, alpha),
      Color::Grey => femtovg::Color::rgba(127, 127, 127, alpha),
      Color::Brown => femtovg::Color::rgba(153, 76, 0, alpha),
    }
  }

  pub fn to_rgb(self) -> femtovg::Color {
    self.to_rgba(255)
  }

  pub fn all() -> &'static [Color] {
    &ALL_COLORS
  }
}

impl FromStr for Color {
  type Err = ();
  fn from_str(input: &str) -> Result<Color, Self::Err> {
    let lowercase = input.to_lowercase();
    match lowercase.as_str() {
      "blue" => Ok(Color::Blue),
      "darkblue" => Ok(Color::DarkBlue),
      "red" => Ok(Color::Red),
      "darkred" => Ok(Color::DarkRed),
      "green" => Ok(Color::Green),
      "darkgreen" => Ok(Color::DarkGreen),
      "yellow" => Ok(Color::Yellow),
      "darkyellow" => Ok(Color::DarkYellow),
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

impl FromStr for FillStyle {
  type Err = ();
  fn from_str(input: &str) -> Result<FillStyle, Self::Err> {
    let lowercase = input.to_lowercase();
    match lowercase.as_str() {
      "solid" => Ok(FillStyle::Solid),
      "transparent" => Ok(FillStyle::Transparent),
      "nofill" => Ok(FillStyle::NoFill),
      _ => Err(()),
    }
  }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
  pub coordinates: Vec<Coordinate>,
  pub style: Style,
  pub visible: bool,
  pub label: Option<String>,
}

impl Shape {
  pub fn new(coordinates: Vec<Coordinate>) -> Self {
    Shape {
      coordinates,
      visible: true,
      ..Default::default()
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

  pub fn with_label(mut self, label: Option<String>) -> Self {
    self.label = label;
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
pub struct Focus {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MapEvent {
  Shutdown,
  Clear,
  TileDataArrived { tile: Tile, data: Vec<u8> },
  Layer(Layer),
  Focus,
  Screenshot(PathBuf),
}
