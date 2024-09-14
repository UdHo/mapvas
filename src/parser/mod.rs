mod grep;
use std::{
  fs::File,
  io::{BufRead, BufReader},
  iter::empty,
  path::PathBuf,
};

pub use grep::GrepParser;
mod random;
pub use random::RandomParser;
mod tt_json;
pub use tt_json::TTJsonParser;

use crate::map::map_event::MapEvent;

/// An interface for input parsers.
pub trait Parser {
  /// Handles the next line of the input. Returns optionally a map event that can be send.
  /// * `line` - The next line to parse.
  fn parse_line(&mut self, line: &str) -> Option<MapEvent>;
  /// Is called when the complete input has been parsed.
  /// For parses that parse a complete document, e.g. a json parser it returns the result.
  fn finalize(&self) -> Option<MapEvent> {
    None
  }
}

pub trait FileParser {
  /// Gives an iterator over persed MapEvents parsed from read.
  /// This is the default method to use.
  fn parse<'a>(&'a mut self, file: Box<dyn BufRead>) -> Box<dyn Iterator<Item = MapEvent> + '_>;
}

impl<T> FileParser for T
where
  T: Parser,
{
  fn parse<'a>(
    &'a mut self,
    mut file: Box<dyn BufRead>,
  ) -> Box<dyn Iterator<Item = MapEvent> + '_> {
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
  pub fn new(path: PathBuf) -> Self {
    Self {
      parser: Self::get_parser(&path),
      path,
    }
  }

  fn get_parser(_path: &PathBuf) -> Box<dyn FileParser> {
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
    let data = r#"52.0, 10.0
                        nothing\n
                        53.0, 11.0 green
                        end"#;
    let mut parser = Box::new(GrepParser::new(false));
    let read = Box::new(data.as_bytes());
    let parsed: Vec<_> = parser.parse(read).collect();
    assert_eq!(parsed.len(), 2);
  }
}
