use egui::Color32;
use serde_json::{Map, Value};

use crate::map::geometry_collection::{Metadata, Style};

/// Shared style parsing utilities for JSON-based parsers
pub struct StyleParser;

impl StyleParser {
  /// Extract metadata and style from JSON properties/object
  pub fn extract_metadata_from_json(properties: Option<&Value>) -> Metadata {
    let mut metadata = Metadata::default();

    if let Some(Value::Object(props)) = properties {
      // Extract name/title/label for metadata
      let label = props
        .get("name")
        .or_else(|| props.get("title"))
        .or_else(|| props.get("label"))
        .and_then(Value::as_str)
        .map(String::from);

      if let Some(label) = label {
        metadata = metadata.with_label(label);
      }

      // Extract style information
      let style = Self::extract_style_from_properties(props);
      if let Some(style) = style {
        metadata = metadata.with_style(style);
      }
    }

    metadata
  }

  /// Extract style information from JSON properties
  pub fn extract_style_from_properties(props: &Map<String, Value>) -> Option<Style> {
    let mut style = Style::default();
    let mut has_style = false;

    // Check for Mapbox/Leaflet style properties first (lower precedence)
    if let Some(color) = props.get("color").and_then(Value::as_str) {
      if let Some(parsed_color) = Self::parse_color(color) {
        style = style.with_color(parsed_color);
        has_style = true;
      }
    }

    if let Some(fill_color) = props.get("fillColor").and_then(Value::as_str) {
      if let Some(parsed_color) = Self::parse_color(fill_color) {
        style = style.with_fill_color(parsed_color);
        has_style = true;
      }
    }

    // Check for common GeoJSON style properties (higher precedence)
    if let Some(stroke) = props.get("stroke").and_then(Value::as_str) {
      if let Some(color) = Self::parse_color(stroke) {
        style = style.with_color(color);
        has_style = true;
      }
    }

    if let Some(fill) = props.get("fill").and_then(Value::as_str) {
      if let Some(color) = Self::parse_color(fill) {
        style = style.with_fill_color(color);
        has_style = true;
      }
    }

    // Check for stroke-width, stroke-opacity, fill-opacity
    if props.contains_key("stroke-width")
      || props.contains_key("stroke-opacity")
      || props.contains_key("fill-opacity")
    {
      has_style = true;
    }

    if has_style { Some(style) } else { None }
  }

  /// Parse color string (hex, rgb, named colors)
  pub fn parse_color(color_str: &str) -> Option<Color32> {
    let color_str = color_str.trim();

    // Handle hex colors
    if let Some(hex) = color_str.strip_prefix('#') {
      return Self::parse_hex_color(hex);
    }

    // Handle rgb() colors
    if color_str.starts_with("rgb(") && color_str.ends_with(')') {
      return Self::parse_rgb_color(&color_str[4..color_str.len() - 1]);
    }

    // Handle named colors
    match color_str.to_lowercase().as_str() {
      "red" => Some(Color32::RED),
      "green" => Some(Color32::GREEN),
      "blue" => Some(Color32::BLUE),
      "yellow" => Some(Color32::YELLOW),
      "black" => Some(Color32::BLACK),
      "white" => Some(Color32::WHITE),
      "gray" | "grey" => Some(Color32::GRAY),
      _ => None,
    }
  }

  /// Parse hex color string
  pub fn parse_hex_color(hex: &str) -> Option<Color32> {
    match hex.len() {
      3 => {
        // Short hex: #RGB -> #RRGGBB
        let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
        let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
        let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
        Some(Color32::from_rgb(r, g, b))
      }
      6 => {
        // Full hex: #RRGGBB
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color32::from_rgb(r, g, b))
      }
      8 => {
        // Full hex with alpha: #RRGGBBAA
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
        Some(Color32::from_rgba_unmultiplied(r, g, b, a))
      }
      _ => None,
    }
  }

  /// Parse `rgb()` color string
  pub fn parse_rgb_color(rgb: &str) -> Option<Color32> {
    let parts: Vec<&str> = rgb.split(',').map(str::trim).collect();
    if parts.len() >= 3 {
      let r = parts[0].parse::<u8>().ok()?;
      let g = parts[1].parse::<u8>().ok()?;
      let b = parts[2].parse::<u8>().ok()?;
      Some(Color32::from_rgb(r, g, b))
    } else {
      None
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn test_color_parsing() {
    // Test hex colors
    assert_eq!(StyleParser::parse_color("#ff0000"), Some(Color32::RED));
    assert_eq!(StyleParser::parse_color("#f00"), Some(Color32::RED));
    assert_eq!(
      StyleParser::parse_color("#ff000080"),
      Some(Color32::from_rgba_unmultiplied(255, 0, 0, 128))
    );

    // Test RGB colors
    assert_eq!(
      StyleParser::parse_color("rgb(255, 0, 0)"),
      Some(Color32::RED)
    );
    assert_eq!(
      StyleParser::parse_color("rgb(0, 255, 0)"),
      Some(Color32::GREEN)
    );

    // Test named colors
    assert_eq!(StyleParser::parse_color("red"), Some(Color32::RED));
    assert_eq!(StyleParser::parse_color("blue"), Some(Color32::BLUE));
    assert_eq!(StyleParser::parse_color("green"), Some(Color32::GREEN));

    // Test invalid colors
    assert_eq!(StyleParser::parse_color("invalid"), None);
    assert_eq!(StyleParser::parse_color("#gg0000"), None);
  }

  #[test]
  fn test_style_extraction() {
    let data = json!({
      "name": "Test Point",
      "stroke": "#ff0000",
      "fill": "#00ff00",
      "color": "blue"
    });

    if let Value::Object(props) = data {
      let style = StyleParser::extract_style_from_properties(&props);
      assert!(style.is_some());

      if let Some(style) = style {
        // stroke should override color
        assert_eq!(style.color(), Color32::RED);
        assert_eq!(style.fill_color(), Color32::GREEN);
      }
    }
  }

  #[test]
  fn test_metadata_extraction() {
    let data = json!({
      "name": "Test Feature",
      "stroke": "#ff0000"
    });

    let metadata = StyleParser::extract_metadata_from_json(Some(&data));
    assert_eq!(metadata.label, Some("Test Feature".to_string()));
    assert!(metadata.style.is_some());

    if let Some(style) = &metadata.style {
      assert_eq!(style.color(), Color32::RED);
    }
  }
}
