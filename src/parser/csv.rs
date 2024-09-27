use csv::{Reader, StringRecord};

use super::Parser;

struct Csv {
  separator: char,
  parser: Box<dyn Parser>,
}

impl Parser for Csv {
  fn parse_line(&mut self, line: &str) -> Option<crate::map::map_event::MapEvent> {
    let mut rdr = Reader::from_reader(line.as_bytes());
    let mut record = StringRecord::new();

    rdr.read_record(&mut record);
    None
  }
}
