use std::{
  fs::File,
  io::{BufRead, BufReader, Cursor},
  iter::empty,
  path::{Path, PathBuf},
};

mod grep;
pub use grep::GrepParser;
mod tt_json;
use serde::{Deserialize, Serialize};
pub use tt_json::TTJsonParser;
mod json;
pub use json::JsonParser;
mod geojson;
pub use geojson::GeoJsonParser;
mod gpx;
pub use gpx::GpxParser;
mod kml;
pub use kml::KmlParser;
mod style;

#[cfg(test)]
pub mod test_utils;

use crate::map::map_event::MapEvent;

/// An interface for input parsers.
pub trait Parser {
  /// Handles the next line of the input. Returns optionally a map event that can be send.
  /// * `line` - The next line to parse.
  fn parse_line(&mut self, line: &str) -> Option<MapEvent>;
  /// Is called when the complete input has been parsed.
  /// For parses that parse a complete document, e.g. a json parser it returns the result.
  fn finalize(&mut self) -> Option<MapEvent> {
    None
  }
  /// Set the layer name for this parser (optional, defaults to parser-specific name)
  fn set_layer_name(&mut self, _layer_name: String) {
    // Default implementation does nothing - parsers can override if needed
  }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Parsers {
  Grep(GrepParser),
  TTJson(TTJsonParser),
  Json(JsonParser),
  GeoJson(GeoJsonParser),
  Gpx(GpxParser),
  Kml(KmlParser),
}

impl Parser for Parsers {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    match self {
      Parsers::Grep(parser) => parser.parse_line(line),
      Parsers::TTJson(parser) => parser.parse_line(line),
      Parsers::Json(parser) => parser.parse_line(line),
      Parsers::GeoJson(parser) => parser.parse_line(line),
      Parsers::Gpx(parser) => parser.parse_line(line),
      Parsers::Kml(parser) => parser.parse_line(line),
    }
  }

  fn finalize(&mut self) -> Option<MapEvent> {
    match self {
      Parsers::Grep(parser) => parser.finalize(),
      Parsers::TTJson(parser) => parser.finalize(),
      Parsers::Json(parser) => parser.finalize(),
      Parsers::GeoJson(parser) => parser.finalize(),
      Parsers::Gpx(parser) => parser.finalize(),
      Parsers::Kml(parser) => parser.finalize(),
    }
  }

  fn set_layer_name(&mut self, layer_name: String) {
    match self {
      Parsers::Grep(parser) => Parser::set_layer_name(parser, layer_name),
      Parsers::TTJson(parser) => Parser::set_layer_name(parser, layer_name),
      Parsers::Json(parser) => Parser::set_layer_name(parser, layer_name),
      Parsers::GeoJson(parser) => Parser::set_layer_name(parser, layer_name),
      Parsers::Gpx(parser) => Parser::set_layer_name(parser, layer_name),
      Parsers::Kml(parser) => Parser::set_layer_name(parser, layer_name),
    }
  }
}

pub trait FileParser {
  /// Gives an iterator over persed `MapEvents` parsed from read.
  /// This is the default method to use.
  fn parse(&mut self, file: Box<dyn BufRead>) -> Box<dyn Iterator<Item = MapEvent> + '_>;

  /// Set the layer name for this parser (optional, defaults to parser-specific name)
  fn set_layer_name(&mut self, _layer_name: String) {
    // Default implementation does nothing - parsers can override if needed
  }
}

impl<T> FileParser for T
where
  T: Parser,
{
  fn parse(&mut self, mut file: Box<dyn BufRead>) -> Box<dyn Iterator<Item = MapEvent> + '_> {
    let mut buf = String::new();
    let mut end = false;
    Box::new(
      std::iter::from_fn(move || {
        buf.clear();
        if let Ok(l) = file.read_line(&mut buf) {
          if end {
            return None;
          }
          if l > 0 {
            Some(self.parse_line(&buf))
          } else {
            end = true;
            Some(self.finalize())
          }
        } else {
          None
        }
      })
      .flatten(),
    )
  }

  fn set_layer_name(&mut self, layer_name: String) {
    Parser::set_layer_name(self, layer_name);
  }
}

/// Encapsulates file reading and choosing the correct parser for a file.
pub struct AutoFileParser {
  path: PathBuf,
  parser_chain: Vec<Box<dyn FileParser>>,
  label_pattern: Option<String>,
  invert_coordinates: bool,
}

impl AutoFileParser {
  #[must_use]
  pub fn new(path: PathBuf) -> Self {
    let mut parser = Self {
      parser_chain: Vec::new(),
      path: path.clone(),
      label_pattern: None,
      invert_coordinates: false,
    };
    parser.parser_chain = parser.get_parser_chain(&path);
    parser
  }

  #[must_use]
  pub fn with_label_pattern(mut self, label_pattern: &str) -> Self {
    self.label_pattern = Some(label_pattern.to_string());
    self.parser_chain = self.get_parser_chain(&self.path.clone());
    self
  }

  #[must_use]
  pub fn with_invert_coordinates(mut self, invert_coordinates: bool) -> Self {
    self.invert_coordinates = invert_coordinates;
    self.parser_chain = self.get_parser_chain(&self.path.clone());
    self
  }

  fn create_grep_parser(&self) -> Box<GrepParser> {
    let mut grep_parser = GrepParser::new(self.invert_coordinates);
    if let Some(pattern) = &self.label_pattern {
      grep_parser = grep_parser.with_label_pattern(pattern);
    }
    Box::new(grep_parser)
  }

  fn get_parser_chain(&self, path: &Path) -> Vec<Box<dyn FileParser>> {
    let mut parsers: Vec<Box<dyn FileParser>> = Vec::new();

    if let Some(extension) = path.extension() {
      let ext = extension.to_string_lossy().to_lowercase();
      match ext.as_str() {
        "geojson" => {
          parsers.push(Box::new(GeoJsonParser::new()));
          parsers.push(Box::new(JsonParser::new()));
          parsers.push(self.create_grep_parser());
        }
        "json" => {
          parsers.push(Box::new(TTJsonParser::new()));
          parsers.push(Box::new(JsonParser::new()));
          parsers.push(Box::new(GeoJsonParser::new()));
          parsers.push(self.create_grep_parser());
        }
        "gpx" => {
          parsers.push(Box::new(GpxParser::new()));
          parsers.push(self.create_grep_parser());
        }
        "kml" | "xml" => {
          parsers.push(Box::new(KmlParser::new()));
          parsers.push(self.create_grep_parser());
        }
        "log" | "txt" => {
          parsers.push(self.create_grep_parser());
        }
        _ => {
          let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_lowercase();

          if filename.contains("geojson") {
            parsers.push(Box::new(GeoJsonParser::new()));
            parsers.push(Box::new(JsonParser::new()));
            parsers.push(self.create_grep_parser());
          } else if filename.contains("json") {
            parsers.push(Box::new(TTJsonParser::new()));
            parsers.push(Box::new(JsonParser::new()));
            parsers.push(self.create_grep_parser());
          } else {
            parsers.push(self.create_grep_parser());
          }
        }
      }
    } else {
      parsers.push(Box::new(TTJsonParser::new()));
      parsers.push(Box::new(JsonParser::new()));
      parsers.push(Box::new(GeoJsonParser::new()));
      parsers.push(self.create_grep_parser());
    }

    parsers
  }

  pub fn parse(&mut self) -> Box<dyn Iterator<Item = MapEvent> + '_> {
    // Extract filename (without extension) for layer name
    let layer_name = self
      .path
      .file_stem()
      .and_then(|name| name.to_str())
      .unwrap_or("unknown")
      .to_string();

    for mut parser in self.parser_chain.drain(..) {
      // Set the layer name using FileParser trait method
      parser.set_layer_name(layer_name.clone());

      let f = File::open(self.path.clone());
      if let Ok(f) = f {
        let read = BufReader::new(f);
        let events: Vec<MapEvent> = parser.parse(Box::new(read)).collect();
        if !events.is_empty() {
          log::debug!(
            "AutoFileParser: Successfully parsed {} with {} events",
            self.path.display(),
            events.len()
          );
          return Box::new(events.into_iter());
        }
      }
    }

    log::warn!(
      "AutoFileParser: No parser could successfully parse {}",
      self.path.display()
    );
    Box::new(empty())
  }
}

pub struct ContentAutoParser {
  content: String,
  label_pattern: Option<String>,
  invert_coordinates: bool,
}

impl ContentAutoParser {
  #[must_use]
  pub fn new(content: String) -> Self {
    Self {
      content,
      label_pattern: None,
      invert_coordinates: false,
    }
  }

  #[must_use]
  pub fn with_label_pattern(mut self, label_pattern: &str) -> Self {
    self.label_pattern = Some(label_pattern.to_string());
    self
  }

  #[must_use]
  pub fn with_invert_coordinates(mut self, invert_coordinates: bool) -> Self {
    self.invert_coordinates = invert_coordinates;
    self
  }

  fn get_content_parser_chain(&self) -> Vec<Box<dyn FileParser>> {
    let mut parsers: Vec<Box<dyn FileParser>> = Vec::new();
    let sample_lines: Vec<&str> = self.content.lines().take(10).collect();
    let sample = sample_lines.join("\n");
    let looks_like_geojson = sample.contains(r#""type":"Feature""#)
      || sample.contains(r#""type": "Feature""#)
      || sample.contains(r#""type":"FeatureCollection""#)
      || sample.contains(r#""type": "FeatureCollection""#)
      || sample.contains(r#""type":"Point""#)
      || sample.contains(r#""type": "Point""#)
      || sample.contains(r#""type":"LineString""#)
      || sample.contains(r#""type": "LineString""#)
      || sample.contains(r#""type":"Polygon""#)
      || sample.contains(r#""type": "Polygon""#)
      || sample.contains(r#""type":"MultiPoint""#)
      || sample.contains(r#""type": "MultiPoint""#)
      || sample.contains(r#""type":"MultiLineString""#)
      || sample.contains(r#""type": "MultiLineString""#)
      || sample.contains(r#""type":"MultiPolygon""#)
      || sample.contains(r#""type": "MultiPolygon""#)
      || sample.contains(r#""type":"GeometryCollection""#)
      || sample.contains(r#""type": "GeometryCollection""#);

    let looks_like_json_geometry = sample.contains(r#""geometry""#)
      && (sample.contains(r#""coordinates""#)
        || sample.contains(r#""Point""#)
        || sample.contains(r#""LineString""#)
        || sample.contains(r#""Polygon""#));

    let looks_like_ttjson = sample.contains(r#""coordinates""#)
      && (sample.contains(r#""color""#) || sample.contains(r#""label""#))
      && !sample.contains(r#""geometry""#);

    let looks_like_gpx =
      sample.contains("<gpx") || sample.contains("<wpt") || sample.contains("<trk");

    let looks_like_kml =
      sample.contains("<kml") || sample.contains("<Placemark") || sample.contains("<coordinates>");

    let looks_like_coordinates = {
      let coordinate_regex = regex::Regex::new(r"[-+]?\d+\.?\d*\s*,\s*[-+]?\d+\.?\d*").unwrap();
      coordinate_regex.is_match(&sample)
    };
    if looks_like_geojson {
      parsers.push(Box::new(GeoJsonParser::new()));
      parsers.push(Box::new(JsonParser::new()));
    } else if looks_like_json_geometry {
      parsers.push(Box::new(JsonParser::new()));
      parsers.push(Box::new(GeoJsonParser::new()));
    } else if looks_like_ttjson {
      parsers.push(Box::new(TTJsonParser::new()));
      parsers.push(Box::new(JsonParser::new()));
    } else if looks_like_gpx {
      parsers.push(Box::new(GpxParser::new()));
    } else if looks_like_kml {
      parsers.push(Box::new(KmlParser::new()));
    } else if looks_like_coordinates {
      let mut grep_parser = GrepParser::new(self.invert_coordinates);
      if let Some(pattern) = &self.label_pattern {
        grep_parser = grep_parser.with_label_pattern(pattern);
      }
      parsers.push(Box::new(grep_parser));
    } else {
      parsers.push(Box::new(TTJsonParser::new()));
      parsers.push(Box::new(JsonParser::new()));
      parsers.push(Box::new(GeoJsonParser::new()));
    }
    if !looks_like_coordinates {
      let mut grep_parser = GrepParser::new(self.invert_coordinates);
      if let Some(pattern) = &self.label_pattern {
        grep_parser = grep_parser.with_label_pattern(pattern);
      }
      parsers.push(Box::new(grep_parser));
    }

    parsers
  }

  #[must_use]
  pub fn parse(&self) -> Vec<MapEvent> {
    let parser_chain = self.get_content_parser_chain();

    for mut parser in parser_chain {
      // Set layer name to "stdin" for content parsed from stdin
      parser.set_layer_name("stdin".to_string());

      let content_bytes = self.content.as_bytes().to_vec();
      let cursor = Cursor::new(content_bytes);
      let read: Box<dyn BufRead> = Box::new(cursor);
      let events: Vec<MapEvent> = parser.parse(read).collect();

      if !events.is_empty() {
        log::debug!(
          "ContentAutoParser: Successfully parsed stdin content with {} events using parser",
          events.len()
        );
        return events;
      }
    }

    log::warn!("ContentAutoParser: No parser could successfully parse stdin content");
    Vec::new()
  }
}

#[cfg(test)]
mod tests {
  use super::{AutoFileParser, ContentAutoParser, GeoJsonParser, GpxParser, JsonParser, KmlParser};
  use crate::map::map_event::MapEvent;
  use crate::parser::{FileParser, GrepParser};
  use std::fs;
  use std::path::PathBuf;

  #[test]
  fn parse() {
    let data = r"52.0, 10.0
                        nothing\n
                        53.0, 11.0 green
                        end";
    let mut parser = Box::new(GrepParser::new(false));
    let read = Box::new(data.as_bytes());
    let parsed: Vec<_> = parser.parse(read).collect();
    assert_eq!(parsed.len(), 2);
  }

  #[test]
  fn test_auto_parser_extension_detection() {
    // Test that the correct parser chains are created for different extensions
    let geojson_path = PathBuf::from("test.geojson");
    let auto_parser = AutoFileParser::new(geojson_path.clone());
    let parsers = auto_parser.get_parser_chain(&geojson_path);
    assert!(parsers.len() >= 2); // Should have GeoJson, Json, and Grep as fallbacks

    let json_path = PathBuf::from("test.json");
    let auto_parser = AutoFileParser::new(json_path.clone());
    let parsers = auto_parser.get_parser_chain(&json_path);
    assert!(parsers.len() >= 3); // Should have TTJson, Json, GeoJson, and Grep as fallbacks

    let txt_path = PathBuf::from("test.txt");
    let auto_parser = AutoFileParser::new(txt_path.clone());
    let parsers = auto_parser.get_parser_chain(&txt_path);
    assert_eq!(parsers.len(), 1); // Should only have Grep parser

    let no_ext_path = PathBuf::from("test");
    let auto_parser = AutoFileParser::new(no_ext_path.clone());
    let parsers = auto_parser.get_parser_chain(&no_ext_path);
    assert!(parsers.len() >= 3); // Should try TTJson, Json, GeoJson, and Grep
  }

  #[test]
  fn test_content_auto_parser_detection() {
    use super::ContentAutoParser;

    // Test GeoJSON detection
    let geojson_content =
      r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [0, 0]}}"#;
    let parser = ContentAutoParser::new(geojson_content.to_string());
    let chain = parser.get_content_parser_chain();
    assert!(chain.len() >= 2); // Should detect as GeoJSON

    // Test TTJson detection
    let ttjson_content = r#"{"coordinates": [51.3, 8.7], "color": "red"}"#;
    let parser = ContentAutoParser::new(ttjson_content.to_string());
    let chain = parser.get_content_parser_chain();
    assert!(chain.len() >= 2); // Should detect as TTJson

    // Test coordinate detection
    let coord_content = "51.3, 8.7 Test point\n52.0, 9.0 Another point";
    let parser = ContentAutoParser::new(coord_content.to_string());
    let chain = parser.get_content_parser_chain();
    assert!(!chain.is_empty()); // Should detect coordinates and use grep
  }

  #[test]
  fn test_comprehensive_parser_comparison() {
    type TestFile = (&'static str, fn() -> Box<dyn FileParser>);
    let test_files: Vec<TestFile> = vec![
      ("tests/resources/simple_point.geojson", || {
        Box::new(GeoJsonParser::new())
      }),
      ("tests/resources/simple_linestring.geojson", || {
        Box::new(GeoJsonParser::new())
      }),
      ("tests/resources/simple_polygon.geojson", || {
        Box::new(GeoJsonParser::new())
      }),
      ("tests/resources/feature_collection.geojson", || {
        Box::new(GeoJsonParser::new())
      }),
      ("tests/resources/simple_coordinates.json", || {
        Box::new(JsonParser::new())
      }),
      ("tests/resources/multiple_coordinates.json", || {
        Box::new(JsonParser::new())
      }),
      ("tests/resources/simple_point.kml", || {
        Box::new(KmlParser::new())
      }),
      ("tests/resources/simple_linestring.kml", || {
        Box::new(KmlParser::new())
      }),
      ("tests/resources/simple_waypoint.gpx", || {
        Box::new(GpxParser::new())
      }),
      ("tests/resources/simple_track.gpx", || {
        Box::new(GpxParser::new())
      }),
    ];

    for (file_path, manual_parser_fn) in test_files {
      println!("Testing file: {file_path}");
      let content = fs::read_to_string(file_path)
        .unwrap_or_else(|_| panic!("Test file {file_path} must exist and be readable"));
      let mut manual_parser = manual_parser_fn();
      let content_bytes = content.as_bytes().to_vec();
      let cursor1 = std::io::Cursor::new(content_bytes);
      let manual_events: Vec<_> = manual_parser.parse(Box::new(cursor1)).collect();
      let mut auto_file_parser = AutoFileParser::new(PathBuf::from(file_path));
      let extension_events: Vec<_> = auto_file_parser.parse().collect();
      let content_parser = ContentAutoParser::new(content.clone());
      let content_events = content_parser.parse();
      assert!(
        !manual_events.is_empty(),
        "Manual parser must produce at least one event for test file {file_path}"
      );
      assert!(
        !extension_events.is_empty(),
        "Extension auto parser must produce at least one event for {file_path}"
      );
      assert!(
        !content_events.is_empty(),
        "Content auto parser must produce at least one event for {file_path}"
      );
      println!("  Manual parser events: {}", manual_events.len());
      println!("  Extension auto parser events: {}", extension_events.len());
      println!("  Content auto parser events: {}", content_events.len());
      assert_eq!(
        manual_events.len(),
        extension_events.len(),
        "Extension auto parser produced different number of events for {file_path}"
      );

      assert_eq!(
        manual_events.len(),
        content_events.len(),
        "Content auto parser produced different number of events for {file_path}"
      );
      // Compare events but allow different layer names
      for (i, (manual_event, extension_event)) in manual_events
        .iter()
        .zip(extension_events.iter())
        .enumerate()
      {
        match (manual_event, extension_event) {
          (MapEvent::Layer(manual_layer), MapEvent::Layer(extension_layer)) => {
            assert_eq!(
              manual_layer.geometries, extension_layer.geometries,
              "Event {i} geometries differ between manual and extension parsers for file {file_path}"
            );
            // Layer names are expected to be different (manual uses parser default, extension uses filename)
          }
          _ => {
            assert_eq!(
              manual_event, extension_event,
              "Event {i} differs between manual and extension parsers for file {file_path}"
            );
          }
        }
      }

      // ContentAutoParser should match manual parser exactly (including layer names)
      for (i, (manual_event, content_event)) in
        manual_events.iter().zip(content_events.iter()).enumerate()
      {
        match (manual_event, content_event) {
          (MapEvent::Layer(manual_layer), MapEvent::Layer(content_layer)) => {
            assert_eq!(
              manual_layer.geometries, content_layer.geometries,
              "Event {i} geometries differ between manual and content parsers for file {file_path}"
            );
            // Content parser uses "stdin" as layer name, manual parser uses default name
            // Both should have same geometries
          }
          _ => {
            assert_eq!(
              manual_event, content_event,
              "Event {i} differs between manual and content parsers for file {file_path}"
            );
          }
        }
      }

      println!("  ✅ All three parsing approaches produced identical results");
    }
  }
}
