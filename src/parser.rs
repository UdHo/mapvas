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
  parser: Box<dyn FileParser>,
}

impl AutoFileParser {
  #[must_use]
  pub fn new(path: PathBuf) -> Self {
    Self {
      parser: Self::get_parser(&path),
      path,
    }
  }

  fn get_parser(_path: &Path) -> Box<dyn FileParser> {
    Box::new(GrepParser::new(false))
  }

  pub fn parse(&mut self) -> Box<dyn Iterator<Item = MapEvent> + '_> {
    let f = File::open(self.path.clone());
    if let Ok(f) = f {
      let read = BufReader::new(f);
      self.parser.parse(Box::new(read))
    } else {
      Box::new(empty())
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::parser::FileParser;

  use super::GrepParser;

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
}
