use egui::Color32;
use serde_json::{Map, Value};

use crate::map::geometry_collection::{Metadata, Style, TimeData};
use chrono::{DateTime, Utc};

/// Shared style parsing utilities for JSON-based parsers
pub struct StyleParser;

impl StyleParser {
  /// Extract metadata and style from JSON properties/object
  pub fn extract_metadata_from_json(properties: Option<&Value>) -> Metadata {
    let mut metadata = Metadata::default();

    if let Some(Value::Object(props)) = properties {
      let label = props
        .get("name")
        .or_else(|| props.get("title"))
        .or_else(|| props.get("label"))
        .and_then(Value::as_str)
        .map(String::from);

      if let Some(label) = label {
        metadata = metadata.with_label(label);
      }

      // Extract heading from properties
      let heading = props
        .get("heading")
        .or_else(|| props.get("bearing"))
        .or_else(|| props.get("direction"))
        .or_else(|| props.get("course"))
        .and_then(|v| {
          v.as_f64().map(|f| {
            #[allow(clippy::cast_possible_truncation)]
            {
              f as f32
            }
          })
        });

      if let Some(heading) = heading {
        metadata = metadata.with_heading(heading);
      }

      // Extract temporal data from properties
      let time_data = Self::extract_temporal_data_from_properties(props);
      if let Some(time_data) = time_data {
        metadata = metadata.with_time_data(time_data);
      }

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

    if let Some(color) = props.get("color").and_then(Value::as_str)
      && let Some(parsed_color) = Self::parse_color(color)
    {
      style = style.with_color(parsed_color);
      has_style = true;
    }

    if let Some(fill_color) = props.get("fillColor").and_then(Value::as_str)
      && let Some(parsed_color) = Self::parse_color(fill_color)
    {
      style = style.with_fill_color(parsed_color);
      has_style = true;
    }

    if let Some(stroke) = props.get("stroke").and_then(Value::as_str)
      && let Some(color) = Self::parse_color(stroke)
    {
      style = style.with_color(color);
      has_style = true;
    }

    if let Some(fill) = props.get("fill").and_then(Value::as_str)
      && let Some(color) = Self::parse_color(fill)
    {
      style = style.with_fill_color(color);
      has_style = true;
    }

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

    if let Some(hex) = color_str.strip_prefix('#') {
      return Self::parse_hex_color(hex);
    }

    if color_str.starts_with("rgb(") && color_str.ends_with(')') {
      return Self::parse_rgb_color(&color_str[4..color_str.len() - 1]);
    }

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
        let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
        let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
        let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
        Some(Color32::from_rgb(r, g, b))
      }
      6 => {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color32::from_rgb(r, g, b))
      }
      8 => {
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

  /// Extract temporal data from JSON properties
  pub fn extract_temporal_data_from_properties(props: &Map<String, Value>) -> Option<TimeData> {
    // Look for common timestamp property names (string or number)
    let timestamp = props
      .get("timestamp")
      .or_else(|| props.get("time"))
      .or_else(|| props.get("datetime"))
      .or_else(|| props.get("date"))
      .and_then(|v| {
        // Try as string first
        if let Some(s) = v.as_str() {
          Self::parse_timestamp(s)
        }
        // Try as number (Unix timestamp)
        else if let Some(n) = v.as_i64() {
          Self::parse_unix_timestamp(n)
        } else {
          None
        }
      });

    // Look for time span properties (string format)
    let begin_time = props
      .get("timeBegin")
      .or_else(|| props.get("time_begin"))
      .or_else(|| props.get("startTime"))
      .or_else(|| props.get("start_time"))
      .and_then(Value::as_str)
      .and_then(Self::parse_timestamp);

    let end_time = props
      .get("timeEnd")
      .or_else(|| props.get("time_end"))
      .or_else(|| props.get("endTime"))
      .or_else(|| props.get("end_time"))
      .and_then(Value::as_str)
      .and_then(Self::parse_timestamp);

    // Look for time_range array format
    let (range_begin, range_end) = if let Some(time_range) = props.get("time_range")
      && let Some(array) = time_range.as_array()
      && array.len() >= 2
    {
      let begin = array[0].as_str().and_then(Self::parse_timestamp);
      let end = array[1].as_str().and_then(Self::parse_timestamp);
      (begin, end)
    } else {
      (None, None)
    };

    // Combine time span sources (prefer explicit properties over range array)
    let final_begin = begin_time.or(range_begin);
    let final_end = end_time.or(range_end);

    // Create TimeData if we have any temporal information
    if timestamp.is_some() || final_begin.is_some() || final_end.is_some() {
      Some(TimeData {
        timestamp,
        time_span: if final_begin.is_some() || final_end.is_some() {
          Some(crate::map::geometry_collection::TimeSpan {
            begin: final_begin,
            end: final_end,
          })
        } else {
          None
        },
      })
    } else {
      None
    }
  }

  /// Parse timestamp string to `DateTime<Utc>`
  pub fn parse_timestamp(timestamp_str: &str) -> Option<DateTime<Utc>> {
    // Try ISO 8601 format first (most common)
    if let Ok(dt) = DateTime::parse_from_rfc3339(timestamp_str) {
      return Some(dt.with_timezone(&Utc));
    }

    // Try ISO 8601 without timezone (assume UTC)
    if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(timestamp_str, "%Y-%m-%dT%H:%M:%S")
    {
      return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc));
    }

    // Try date only format
    if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(timestamp_str, "%Y-%m-%d") {
      let naive_dt = naive_date.and_hms_opt(0, 0, 0)?;
      return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc));
    }

    // Try Unix timestamp (seconds)
    if let Ok(timestamp) = timestamp_str.parse::<i64>() {
      return DateTime::<Utc>::from_timestamp(timestamp, 0);
    }

    // Try Unix timestamp (milliseconds)
    if let Ok(timestamp_ms) = timestamp_str.parse::<i64>() {
      return DateTime::<Utc>::from_timestamp_millis(timestamp_ms);
    }

    None
  }

  /// Parse Unix timestamp (number) to `DateTime<Utc>`
  fn parse_unix_timestamp(timestamp: i64) -> Option<DateTime<Utc>> {
    // Try as seconds first (most common)
    if let Some(dt) = DateTime::<Utc>::from_timestamp(timestamp, 0) {
      return Some(dt);
    }

    // Try as milliseconds (if number is very large)
    if timestamp > 1_000_000_000_000 {
      return DateTime::<Utc>::from_timestamp_millis(timestamp);
    }

    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn test_color_parsing() {
    assert_eq!(StyleParser::parse_color("#ff0000"), Some(Color32::RED));
    assert_eq!(StyleParser::parse_color("#f00"), Some(Color32::RED));
    assert_eq!(
      StyleParser::parse_color("#ff000080"),
      Some(Color32::from_rgba_unmultiplied(255, 0, 0, 128))
    );

    assert_eq!(
      StyleParser::parse_color("rgb(255, 0, 0)"),
      Some(Color32::RED)
    );
    assert_eq!(
      StyleParser::parse_color("rgb(0, 255, 0)"),
      Some(Color32::GREEN)
    );

    assert_eq!(StyleParser::parse_color("red"), Some(Color32::RED));
    assert_eq!(StyleParser::parse_color("blue"), Some(Color32::BLUE));
    assert_eq!(StyleParser::parse_color("green"), Some(Color32::GREEN));

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
    assert_eq!(
      metadata.label.as_ref().map(|l| &l.name),
      Some(&"Test Feature".to_string())
    );
    assert!(metadata.style.is_some());

    if let Some(style) = &metadata.style {
      assert_eq!(style.color(), Color32::RED);
    }
  }

  #[test]
  fn test_heading_extraction() {
    let data = json!({
      "name": "Aircraft",
      "heading": 45.5,
      "stroke": "#0000ff"
    });

    let metadata = StyleParser::extract_metadata_from_json(Some(&data));
    assert_eq!(
      metadata.label.as_ref().map(|l| &l.name),
      Some(&"Aircraft".to_string())
    );
    assert_eq!(metadata.heading, Some(45.5));
    assert!(metadata.style.is_some());

    // Test different heading property names
    let data2 = json!({
      "bearing": 180.0
    });
    let metadata2 = StyleParser::extract_metadata_from_json(Some(&data2));
    assert_eq!(metadata2.heading, Some(180.0));

    let data3 = json!({
      "direction": 270.0
    });
    let metadata3 = StyleParser::extract_metadata_from_json(Some(&data3));
    assert_eq!(metadata3.heading, Some(270.0));

    let data4 = json!({
      "course": 90.0
    });
    let metadata4 = StyleParser::extract_metadata_from_json(Some(&data4));
    assert_eq!(metadata4.heading, Some(90.0));
  }

  #[test]
  fn test_temporal_data_extraction() {
    // Test timestamp extraction
    let data = json!({
      "name": "Event",
      "timestamp": "2024-01-15T08:00:00Z"
    });
    let metadata = StyleParser::extract_metadata_from_json(Some(&data));
    assert!(metadata.time_data.is_some());
    if let Some(time_data) = &metadata.time_data {
      assert!(time_data.timestamp.is_some());
      assert!(time_data.time_span.is_none());
    }

    // Test time span extraction
    let data2 = json!({
      "name": "Event Period",
      "startTime": "2024-01-15T08:00:00Z",
      "endTime": "2024-01-15T16:00:00Z"
    });
    let metadata2 = StyleParser::extract_metadata_from_json(Some(&data2));
    assert!(metadata2.time_data.is_some());
    if let Some(time_data) = &metadata2.time_data {
      assert!(time_data.timestamp.is_none());
      assert!(time_data.time_span.is_some());
      if let Some(time_span) = &time_data.time_span {
        assert!(time_span.begin.is_some());
        assert!(time_span.end.is_some());
      }
    }

    // Test different time property names
    let data3 = json!({
      "time": "2024-01-15T12:00:00"
    });
    let metadata3 = StyleParser::extract_metadata_from_json(Some(&data3));
    assert!(metadata3.time_data.is_some());

    let data4 = json!({
      "datetime": "2024-01-15T12:00:00"
    });
    let metadata4 = StyleParser::extract_metadata_from_json(Some(&data4));
    assert!(metadata4.time_data.is_some());

    let data5 = json!({
      "date": "2024-01-15"
    });
    let metadata5 = StyleParser::extract_metadata_from_json(Some(&data5));
    assert!(metadata5.time_data.is_some());

    // Test no temporal data
    let data6 = json!({
      "name": "No Time Event"
    });
    let metadata6 = StyleParser::extract_metadata_from_json(Some(&data6));
    assert!(metadata6.time_data.is_none());

    // Test Unix timestamp (seconds) as number
    let data7 = json!({
      "timestamp": 1_765_875_780
    });
    let metadata7 = StyleParser::extract_metadata_from_json(Some(&data7));
    assert!(metadata7.time_data.is_some());
    if let Some(time_data) = &metadata7.time_data {
      assert!(time_data.timestamp.is_some());
    }

    // Test Unix timestamp (milliseconds) as number
    let data8 = json!({
      "timestamp": 1_765_875_780_123_i64
    });
    let metadata8 = StyleParser::extract_metadata_from_json(Some(&data8));
    assert!(metadata8.time_data.is_some());
    if let Some(time_data) = &metadata8.time_data {
      assert!(time_data.timestamp.is_some());
    }

    // Test time_range array
    let data9 = json!({
      "time_range": ["2024-01-15T08:00:00Z", "2024-01-15T16:00:00Z"]
    });
    let metadata9 = StyleParser::extract_metadata_from_json(Some(&data9));
    assert!(metadata9.time_data.is_some());
    if let Some(time_data) = &metadata9.time_data {
      assert!(time_data.time_span.is_some());
      if let Some(time_span) = &time_data.time_span {
        assert!(time_span.begin.is_some());
        assert!(time_span.end.is_some());
      }
    }
  }
}
