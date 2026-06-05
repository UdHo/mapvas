use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Color([u8; 4]);

impl Color {
  pub const BLACK: Self = Self::from_rgb(0, 0, 0);
  pub const BLUE: Self = Self::from_rgb(0, 0, 255);
  pub const BROWN: Self = Self::from_rgb(165, 42, 42);
  pub const DARK_BLUE: Self = Self::from_rgb(0, 0, 0x8B);
  pub const DARK_GREEN: Self = Self::from_rgb(0, 0x64, 0);
  pub const DARK_RED: Self = Self::from_rgb(0x8B, 0, 0);
  pub const GRAY: Self = Self::from_rgb(160, 160, 160);
  pub const GREEN: Self = Self::from_rgb(0, 255, 0);
  pub const RED: Self = Self::from_rgb(255, 0, 0);
  pub const TRANSPARENT: Self = Self::from_rgba_unmultiplied(0, 0, 0, 0);
  pub const WHITE: Self = Self::from_rgb(255, 255, 255);
  pub const YELLOW: Self = Self::from_rgb(255, 255, 0);

  #[must_use]
  pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
    Self::from_rgba_unmultiplied(r, g, b, 255)
  }

  #[must_use]
  pub const fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
    Self([r, g, b, a])
  }

  #[must_use]
  pub const fn to_rgba_unmultiplied(self) -> [u8; 4] {
    self.0
  }

  #[must_use]
  pub const fn r(self) -> u8 {
    self.0[0]
  }

  #[must_use]
  pub const fn g(self) -> u8 {
    self.0[1]
  }

  #[must_use]
  pub const fn b(self) -> u8 {
    self.0[2]
  }

  #[must_use]
  pub const fn a(self) -> u8 {
    self.0[3]
  }

  #[must_use]
  pub fn gamma_multiply(self, factor: f32) -> Self {
    debug_assert!(
      0.0 <= factor && factor.is_finite(),
      "factor should be finite, but was {factor}"
    );
    Self([
      (f32::from(self.r()) * factor + 0.5) as u8,
      (f32::from(self.g()) * factor + 0.5) as u8,
      (f32::from(self.b()) * factor + 0.5) as u8,
      (f32::from(self.a()) * factor + 0.5) as u8,
    ])
  }
}

impl Default for Color {
  fn default() -> Self {
    Color::BLUE
  }
}

impl FromStr for Color {
  type Err = ();
  fn from_str(input: &str) -> Result<Color, Self::Err> {
    let lowercase = input.to_lowercase();
    match lowercase.as_str() {
      "blue" => Ok(Color::BLUE),
      "darkblue" => Ok(Color::DARK_BLUE),
      "red" => Ok(Color::RED),
      "darkred" => Ok(Color::DARK_RED),
      "green" => Ok(Color::GREEN),
      "darkgreen" => Ok(Color::DARK_GREEN),
      "yellow" => Ok(Color::YELLOW),
      "black" => Ok(Color::BLACK),
      "white" => Ok(Color::WHITE),
      "grey" | "gray" => Ok(Color::GRAY),
      "brown" => Ok(Color::BROWN),
      _ => Err(()),
    }
  }
}
