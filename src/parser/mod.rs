use crate::MapEvent;

/// An interface for input parsers.
pub trait Parser {
  /// Handles the next line of the input. Returns optionally a map event that can be send.
  /// * `line` - The next line to parse.
  fn parse_line(&mut self, line: &String) -> Option<MapEvent>;
  /// Is called when the complete input has been parsed.
  /// For parses that parse a complete document, e.g. a json parser it returns the result.
  fn finalize(&self) -> Option<MapEvent> {
    None
  }
}

mod grep;
pub use grep::GrepParser;
mod random;
pub use random::RandomParser;
mod json;
pub use json::JsonParser;
