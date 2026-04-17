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
  /// Focus on a specific named sub-layer (as sent via `mapcat`).
  FocusLayer { id: String },
  /// Focus on a single shape within a sub-layer.
  FocusShape { layer_id: String, shape_idx: usize },
  /// Create a screenshot and store it to the given path.
  Screenshot(PathBuf),
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn color_from_str_all_variants() {
    let cases = [
      ("blue", Color32::BLUE),
      ("darkblue", Color32::DARK_BLUE),
      ("red", Color32::RED),
      ("darkred", Color32::DARK_RED),
      ("green", Color32::GREEN),
      ("darkgreen", Color32::DARK_GREEN),
      ("yellow", Color32::YELLOW),
      ("black", Color32::BLACK),
      ("white", Color32::WHITE),
      ("grey", Color32::GRAY),
      ("gray", Color32::GRAY),
      ("brown", Color32::BROWN),
    ];
    for (name, expected) in cases {
      let color = Color::from_str(name).unwrap();
      assert_eq!(color.0, expected, "Color '{name}' should match");
    }
  }

  #[test]
  fn color_from_str_case_insensitive() {
    assert_eq!(Color::from_str("Blue").unwrap().0, Color32::BLUE);
    assert_eq!(Color::from_str("DARKRED").unwrap().0, Color32::DARK_RED);
    assert_eq!(Color::from_str("GREEN").unwrap().0, Color32::GREEN);
  }

  #[test]
  fn color_from_str_invalid() {
    assert!(Color::from_str("purple").is_err());
    assert!(Color::from_str("").is_err());
    assert!(Color::from_str("notacolor").is_err());
  }

  #[test]
  fn color_default_is_blue() {
    assert_eq!(Color::default().0, Color32::BLUE);
  }

  #[test]
  fn fillstyle_from_str_all_variants() {
    assert_eq!(FillStyle::from_str("solid").unwrap(), FillStyle::Solid);
    assert_eq!(
      FillStyle::from_str("transparent").unwrap(),
      FillStyle::Transparent
    );
    assert_eq!(FillStyle::from_str("nofill").unwrap(), FillStyle::NoFill);
  }

  #[test]
  fn fillstyle_from_str_case_insensitive() {
    assert_eq!(FillStyle::from_str("Solid").unwrap(), FillStyle::Solid);
    assert_eq!(FillStyle::from_str("NOFILL").unwrap(), FillStyle::NoFill);
  }

  #[test]
  fn fillstyle_from_str_invalid() {
    assert!(FillStyle::from_str("hatched").is_err());
    assert!(FillStyle::from_str("").is_err());
  }

  #[test]
  fn fillstyle_default_is_nofill() {
    assert_eq!(FillStyle::default(), FillStyle::NoFill);
  }

  #[test]
  fn layer_new_constructor() {
    let layer = Layer::new("test_layer".to_string());
    assert_eq!(layer.id, "test_layer");
    assert!(layer.geometries.is_empty());
  }
}
