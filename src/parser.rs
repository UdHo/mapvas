use std::{
  fs::File,
  io::{BufRead, BufReader},
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
}

pub trait FileParser {
  /// Gives an iterator over persed `MapEvents` parsed from read.
  /// This is the default method to use.
  fn parse(&mut self, file: Box<dyn BufRead>) -> Box<dyn Iterator<Item = MapEvent> + '_>;
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
}

/// Encapsulates file reading and choosing the correct parser for a file.
pub struct AutoFileParser {
  path: PathBuf,
  parser_chain: Vec<Box<dyn FileParser>>,
}

impl AutoFileParser {
  #[must_use]
  pub fn new(path: PathBuf) -> Self {
    Self {
      parser_chain: Self::get_parser_chain(&path),
      path,
    }
  }

  fn get_parser_chain(path: &Path) -> Vec<Box<dyn FileParser>> {
    // Get file extension and determine parser chain with fallbacks
    let mut parsers: Vec<Box<dyn FileParser>> = Vec::new();

    if let Some(extension) = path.extension() {
      let ext = extension.to_string_lossy().to_lowercase();
      match ext.as_str() {
        "geojson" => {
          parsers.push(Box::new(GeoJsonParser::new()));
          parsers.push(Box::new(JsonParser::new()));
          parsers.push(Box::new(GrepParser::new(false)));
        }
        "json" => {
          parsers.push(Box::new(TTJsonParser::new()));
          parsers.push(Box::new(JsonParser::new()));
          parsers.push(Box::new(GeoJsonParser::new()));
          parsers.push(Box::new(GrepParser::new(false)));
        }
        "gpx" => {
          parsers.push(Box::new(GpxParser::new()));
          parsers.push(Box::new(GrepParser::new(false)));
        }
        "kml" => {
          parsers.push(Box::new(KmlParser::new()));
          parsers.push(Box::new(GrepParser::new(false)));
        }
        "xml" => {
          parsers.push(Box::new(KmlParser::new()));
          parsers.push(Box::new(GrepParser::new(false)));
        }
        "log" | "txt" => {
          parsers.push(Box::new(GrepParser::new(false)));
        }
        _ => {
          // For unknown extensions, try to guess based on file name patterns
          let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_lowercase();

          if filename.contains("geojson") {
            parsers.push(Box::new(GeoJsonParser::new()));
            parsers.push(Box::new(JsonParser::new()));
            parsers.push(Box::new(GrepParser::new(false)));
          } else if filename.contains("json") {
            parsers.push(Box::new(TTJsonParser::new()));
            parsers.push(Box::new(JsonParser::new()));
            parsers.push(Box::new(GrepParser::new(false)));
          } else {
            // Default to grep parser for unknown files
            parsers.push(Box::new(GrepParser::new(false)));
          }
        }
      }
    } else {
      // No extension, try common parsers in order
      parsers.push(Box::new(TTJsonParser::new()));
      parsers.push(Box::new(JsonParser::new()));
      parsers.push(Box::new(GeoJsonParser::new()));
      parsers.push(Box::new(GrepParser::new(false)));
    }

    parsers
  }

  pub fn parse(&mut self) -> Box<dyn Iterator<Item = MapEvent> + '_> {
    // Try each parser in the chain until one produces events
    for mut parser in self.parser_chain.drain(..) {
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

#[cfg(test)]
mod tests {
  use crate::parser::FileParser;
  use std::path::PathBuf;

  use super::{AutoFileParser, GrepParser};

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
    let parsers = AutoFileParser::get_parser_chain(&geojson_path);
    assert!(parsers.len() >= 2); // Should have GeoJson, Json, and Grep as fallbacks

    let json_path = PathBuf::from("test.json");
    let parsers = AutoFileParser::get_parser_chain(&json_path);
    assert!(parsers.len() >= 3); // Should have TTJson, Json, GeoJson, and Grep as fallbacks

    let txt_path = PathBuf::from("test.txt");
    let parsers = AutoFileParser::get_parser_chain(&txt_path);
    assert_eq!(parsers.len(), 1); // Should only have Grep parser

    let no_ext_path = PathBuf::from("test");
    let parsers = AutoFileParser::get_parser_chain(&no_ext_path);
    assert!(parsers.len() >= 3); // Should try TTJson, Json, GeoJson, and Grep
  }
}
