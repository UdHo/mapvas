use super::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::Geometry,
};
use egui::Color32;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct Color(pub Color32);

impl From<Color32> for Color {
  fn from(value: egui::Color32) -> Self {
    Color(value)
  }
}

impl Default for Color {
  fn default() -> Self {
    Color(egui::Color32::BLUE)
  }
}

impl FromStr for Color {
  type Err = ();
  fn from_str(input: &str) -> Result<Color, Self::Err> {
    let lowercase = input.to_lowercase();
    match lowercase.as_str() {
      "blue" => Ok(Color32::BLUE.into()),
      "darkblue" => Ok(Color32::DARK_BLUE.into()),
      "red" => Ok(Color32::RED.into()),
      "darkred" => Ok(Color32::DARK_RED.into()),
      "green" => Ok(Color32::GREEN.into()),
      "darkgreen" => Ok(Color32::DARK_GREEN.into()),
      "yellow" => Ok(Color32::YELLOW.into()),
      "black" => Ok(Color32::BLACK.into()),
      "white" => Ok(Color32::WHITE.into()),
      "grey" | "gray" => Ok(Color32::GRAY.into()),
      "brown" => Ok(Color32::BROWN.into()),
      _ => Err(()),
    }
  }
}

impl From<Color> for egui::Color32 {
  fn from(value: Color) -> Self {
    value.0
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
  pub id: String,
  pub geometries: Vec<Geometry<PixelCoordinate>>,
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
pub enum MapEvent {
  /// Ends mapvas.
  Shutdown,
  /// Clears the layers.
  Clear,
  /// Adds to the geometry layer.
  Layer(Layer),
  /// Shows the bounding box of everything on the map.
  Focus,
  /// Focus on a specific coordinate with zoom level
  FocusOn {
    coordinate: WGS84Coordinate,
    zoom_level: Option<u8>,
  },
  /// Create a screenshot and store it to the given path.
  Screenshot(PathBuf),
}
