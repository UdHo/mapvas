use super::{
  coordinates::{PixelPosition, WGS84Coordinate},
  geometry_collection::Geometry,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};

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
  Color32(u8, u8, u8, u8),
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

impl From<Color> for egui::Color32 {
  fn from(value: Color) -> Self {
    let alpha = 255;
    match value {
      Color::Blue => egui::Color32::from_rgba_premultiplied(0, 0, 255, alpha),
      Color::DarkBlue => egui::Color32::from_rgba_premultiplied(0, 0, 150, alpha),
      Color::Red => egui::Color32::from_rgba_premultiplied(255, 0, 0, alpha),
      Color::DarkRed => egui::Color32::from_rgba_premultiplied(150, 0, 0, alpha),
      Color::Green => egui::Color32::from_rgba_premultiplied(0, 255, 0, alpha),
      Color::DarkGreen => egui::Color32::from_rgba_premultiplied(0, 150, 0, alpha),
      Color::Yellow => egui::Color32::from_rgba_premultiplied(255, 255, 0, alpha),
      Color::DarkYellow => egui::Color32::from_rgba_premultiplied(150, 150, 0, alpha),
      Color::Black => egui::Color32::from_rgba_premultiplied(0, 0, 0, alpha),
      Color::White => egui::Color32::from_rgba_premultiplied(255, 255, 255, alpha),
      Color::Grey => egui::Color32::from_rgba_premultiplied(127, 127, 127, alpha),
      Color::Brown => egui::Color32::from_rgba_premultiplied(153, 76, 0, alpha),
      Color::Color32(r, g, b, a) => egui::Color32::from_rgba_premultiplied(r, g, b, a),
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
  pub coordinates: Vec<WGS84Coordinate>,
  pub style: Style,
  pub visible: bool,
  pub label: Option<String>,
}

impl Shape {
  #[must_use]
  pub fn new(coordinates: Vec<WGS84Coordinate>) -> Self {
    Shape {
      coordinates,
      visible: true,
      ..Default::default()
    }
  }

  #[must_use]
  pub fn with_color(mut self, color: Color) -> Self {
    self.style.color = color;
    self
  }

  #[must_use]
  pub fn with_fill(mut self, fill: FillStyle) -> Self {
    self.style.fill = fill;
    self
  }

  #[must_use]
  pub fn with_label(mut self, label: Option<String>) -> Self {
    self.label = label;
    self
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
  pub id: String,
  pub geometries: Vec<Geometry<PixelPosition>>,
}

impl Layer {
  #[must_use]
  pub fn new(id: String) -> Self {
    Layer {
      id,
      geometries: vec![],
    }
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Focus {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MapEvent {
  Shutdown,
  Clear,
  Layer(Layer),
  Focus,
  Screenshot(PathBuf),
}
