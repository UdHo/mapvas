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

/// Detected file format for parser selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetectedFormat {
  GeoJson,
  Json,
  Gpx,
  Kml,
  Text,
  Coordinates,
  Unknown,
}

/// `GeoJSON` type markers for content detection
const GEOJSON_TYPE_MARKERS: &[&str] = &[
  r#""type":"Feature""#,
  r#""type": "Feature""#,
  r#""type":"FeatureCollection""#,
  r#""type": "FeatureCollection""#,
  r#""type":"Point""#,
  r#""type": "Point""#,
  r#""type":"LineString""#,
  r#""type": "LineString""#,
  r#""type":"Polygon""#,
  r#""type": "Polygon""#,
  r#""type":"MultiPoint""#,
  r#""type": "MultiPoint""#,
  r#""type":"MultiLineString""#,
  r#""type": "MultiLineString""#,
  r#""type":"MultiPolygon""#,
  r#""type": "MultiPolygon""#,
  r#""type":"GeometryCollection""#,
  r#""type": "GeometryCollection""#,
];

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
  pub fn new(path: &Path) -> Self {
    let mut parser = Self {
      parser_chain: Vec::new(),
      path: path.to_path_buf(),
      label_pattern: None,
      invert_coordinates: false,
    };
    parser.parser_chain = parser.get_parser_chain(path);
    parser
  }

  #[must_use]
  pub fn with_label_pattern(mut self, label_pattern: &str) -> Self {
    self.label_pattern = Some(label_pattern.to_string());
    self.parser_chain = self.get_parser_chain(&self.path);
    self
  }

  #[must_use]
  pub fn with_invert_coordinates(mut self, invert_coordinates: bool) -> Self {
    self.invert_coordinates = invert_coordinates;
    self.parser_chain = self.get_parser_chain(&self.path);
    self
  }

  fn create_grep_parser(&self) -> GrepParser {
    let mut grep_parser = GrepParser::new(self.invert_coordinates);
    if let Some(pattern) = &self.label_pattern {
      grep_parser = grep_parser.with_label_pattern(pattern);
    }
    grep_parser
  }

  fn detect_format_from_extension(ext: &str) -> DetectedFormat {
    match ext {
      "geojson" => DetectedFormat::GeoJson,
      "json" => DetectedFormat::Json,
      "gpx" => DetectedFormat::Gpx,
      "kml" | "xml" => DetectedFormat::Kml,
      "log" | "txt" => DetectedFormat::Text,
      _ => DetectedFormat::Unknown,
    }
  }

  fn detect_format_from_filename(filename: &str) -> DetectedFormat {
    let lower = filename.to_lowercase();
    if lower.contains("geojson") {
      DetectedFormat::GeoJson
    } else if lower.contains("json") {
      DetectedFormat::Json
    } else {
      DetectedFormat::Unknown
    }
  }

  fn build_parser_chain(&self, format: DetectedFormat) -> Vec<Box<dyn FileParser>> {
    let grep = || Box::new(self.create_grep_parser()) as Box<dyn FileParser>;

    match format {
      DetectedFormat::GeoJson => vec![
        Box::new(GeoJsonParser::new()),
        Box::new(JsonParser::new()),
        grep(),
      ],
      DetectedFormat::Gpx => vec![Box::new(GpxParser::new()), grep()],
      DetectedFormat::Kml => vec![Box::new(KmlParser::new()), grep()],
      DetectedFormat::Text | DetectedFormat::Coordinates => vec![grep()],
      // Json and Unknown both try all JSON-like parsers
      DetectedFormat::Json | DetectedFormat::Unknown => vec![
        Box::new(TTJsonParser::new()),
        Box::new(JsonParser::new()),
        Box::new(GeoJsonParser::new()),
        grep(),
      ],
    }
  }

  fn get_parser_chain(&self, path: &Path) -> Vec<Box<dyn FileParser>> {
    let format = path
      .extension()
      .and_then(|ext| ext.to_str())
      .map_or(DetectedFormat::Unknown, |ext| {
        Self::detect_format_from_extension(&ext.to_lowercase())
      });

    // If extension didn't give us a specific format, try filename
    let format = if format == DetectedFormat::Unknown {
      path
        .file_name()
        .and_then(|name| name.to_str())
        .map_or(DetectedFormat::Unknown, Self::detect_format_from_filename)
    } else {
      format
    };

    self.build_parser_chain(format)
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

  fn create_grep_parser(&self) -> GrepParser {
    let mut grep_parser = GrepParser::new(self.invert_coordinates);
    if let Some(pattern) = &self.label_pattern {
      grep_parser = grep_parser.with_label_pattern(pattern);
    }
    grep_parser
  }

  fn detect_format_from_content(sample: &str) -> DetectedFormat {
    // Check for GeoJSON type markers
    if GEOJSON_TYPE_MARKERS.iter().any(|marker| sample.contains(marker)) {
      return DetectedFormat::GeoJson;
    }

    // Check for JSON with geometry structure (but not GeoJSON)
    let has_geometry = sample.contains(r#""geometry""#);
    let has_coordinates = sample.contains(r#""coordinates""#);
    if has_geometry && has_coordinates {
      return DetectedFormat::Json;
    }

    // Check for TTJson (coordinates with color/label, no geometry)
    if has_coordinates
      && (sample.contains(r#""color""#) || sample.contains(r#""label""#))
      && !has_geometry
    {
      return DetectedFormat::Json; // TTJson is a variant of JSON
    }

    // Check for GPX
    if sample.contains("<gpx") || sample.contains("<wpt") || sample.contains("<trk") {
      return DetectedFormat::Gpx;
    }

    // Check for KML
    if sample.contains("<kml") || sample.contains("<Placemark") || sample.contains("<coordinates>")
    {
      return DetectedFormat::Kml;
    }

    // Check for plain coordinates
    let coordinate_regex = regex::Regex::new(r"[-+]?\d+\.?\d*\s*,\s*[-+]?\d+\.?\d*").unwrap();
    if coordinate_regex.is_match(sample) {
      return DetectedFormat::Coordinates;
    }

    DetectedFormat::Unknown
  }

  fn build_parser_chain(&self, format: DetectedFormat) -> Vec<Box<dyn FileParser>> {
    let grep = || Box::new(self.create_grep_parser()) as Box<dyn FileParser>;

    match format {
      DetectedFormat::GeoJson => vec![
        Box::new(GeoJsonParser::new()),
        Box::new(JsonParser::new()),
        grep(),
      ],
      DetectedFormat::Gpx => vec![Box::new(GpxParser::new()), grep()],
      DetectedFormat::Kml => vec![Box::new(KmlParser::new()), grep()],
      DetectedFormat::Text | DetectedFormat::Coordinates => vec![grep()],
      // Json and Unknown both try all JSON-like parsers
      DetectedFormat::Json | DetectedFormat::Unknown => vec![
        Box::new(TTJsonParser::new()),
        Box::new(JsonParser::new()),
        Box::new(GeoJsonParser::new()),
        grep(),
      ],
    }
  }

  fn get_content_parser_chain(&self) -> Vec<Box<dyn FileParser>> {
    let sample: String = self.content.lines().take(10).collect::<Vec<_>>().join("\n");
    let format = Self::detect_format_from_content(&sample);
    self.build_parser_chain(format)
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
  use crate::map::coordinates::Coordinate;
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
    let auto_parser = AutoFileParser::new(&geojson_path);
    let parsers = auto_parser.get_parser_chain(&geojson_path);
    assert!(parsers.len() >= 2); // Should have GeoJson, Json, and Grep as fallbacks

    let json_path = PathBuf::from("test.json");
    let auto_parser = AutoFileParser::new(&json_path);
    let parsers = auto_parser.get_parser_chain(&json_path);
    assert!(parsers.len() >= 3); // Should have TTJson, Json, GeoJson, and Grep as fallbacks

    let txt_path = PathBuf::from("test.txt");
    let auto_parser = AutoFileParser::new(&txt_path);
    let parsers = auto_parser.get_parser_chain(&txt_path);
    assert_eq!(parsers.len(), 1); // Should only have Grep parser

    let no_ext_path = PathBuf::from("test");
    let auto_parser = AutoFileParser::new(&no_ext_path);
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

  #[derive(Debug, Clone, Copy)]
  enum ExpectedParser {
    GeoJson,
    Json,
    Kml,
    Gpx,
  }

  fn create_parser(parser_type: ExpectedParser) -> Box<dyn FileParser> {
    match parser_type {
      ExpectedParser::GeoJson => Box::new(GeoJsonParser::new()),
      ExpectedParser::Json => Box::new(JsonParser::new()),
      ExpectedParser::Kml => Box::new(KmlParser::new()),
      ExpectedParser::Gpx => Box::new(GpxParser::new()),
    }
  }

  use rstest::rstest;

  #[rstest]
  #[case("tests/resources/simple_point.geojson", ExpectedParser::GeoJson)]
  #[case("tests/resources/simple_linestring.geojson", ExpectedParser::GeoJson)]
  #[case("tests/resources/simple_polygon.geojson", ExpectedParser::GeoJson)]
  #[case("tests/resources/feature_collection.geojson", ExpectedParser::GeoJson)]
  #[case("tests/resources/simple_coordinates.json", ExpectedParser::Json)]
  #[case("tests/resources/multiple_coordinates.json", ExpectedParser::Json)]
  #[case("tests/resources/simple_point.kml", ExpectedParser::Kml)]
  #[case("tests/resources/simple_linestring.kml", ExpectedParser::Kml)]
  #[case("tests/resources/simple_waypoint.gpx", ExpectedParser::Gpx)]
  #[case("tests/resources/simple_track.gpx", ExpectedParser::Gpx)]
  fn test_parser_comparison(#[case] file_path: &str, #[case] parser_type: ExpectedParser) {
    let content = fs::read_to_string(file_path).unwrap();

    let mut manual_parser = create_parser(parser_type);
    let cursor = std::io::Cursor::new(content.as_bytes().to_vec());
    let manual_events: Vec<_> = manual_parser.parse(Box::new(cursor)).collect();

    let mut auto_file_parser = AutoFileParser::new(&PathBuf::from(file_path));
    let extension_events: Vec<_> = auto_file_parser.parse().collect();

    let content_parser = ContentAutoParser::new(content.clone());
    let content_events = content_parser.parse();

    assert!(!manual_events.is_empty());
    assert!(!extension_events.is_empty());
    assert!(!content_events.is_empty());
    assert_eq!(manual_events.len(), extension_events.len());
    assert_eq!(manual_events.len(), content_events.len());

    for (i, (manual, extension)) in manual_events.iter().zip(extension_events.iter()).enumerate() {
      if let (MapEvent::Layer(m), MapEvent::Layer(e)) = (manual, extension) {
        assert_eq!(m.geometries, e.geometries, "Event {i} geometries differ");
      } else {
        assert_eq!(manual, extension);
      }
    }

    for (i, (manual, content)) in manual_events.iter().zip(content_events.iter()).enumerate() {
      if let (MapEvent::Layer(m), MapEvent::Layer(c)) = (manual, content) {
        assert_eq!(m.geometries, c.geometries, "Event {i} geometries differ");
      } else {
        assert_eq!(manual, content);
      }
    }
  }

  #[test]
  fn test_coordinate_order_geojson_vs_json() {
    use crate::map::geometry_collection::Geometry;

    // GeoJSON with type marker should use [lon, lat] order
    // Berlin: lat=52.5, lon=13.4
    let geojson_content = r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [13.4, 52.5]}}"#;
    let parser = ContentAutoParser::new(geojson_content.to_string());
    let events = parser.parse();
    assert!(!events.is_empty(), "GeoJSON should parse");
    if let MapEvent::Layer(layer) = &events[0]
      && let Geometry::Point(coord, _) = &layer.geometries[0]
    {
      // GeoJSON [13.4, 52.5] means lon=13.4, lat=52.5
      let wgs = coord.as_wgs84();
      assert!(
        (wgs.lat - 52.5).abs() < 0.01,
        "GeoJSON lat should be ~52.5, got {}",
        wgs.lat
      );
      assert!(
        (wgs.lon - 13.4).abs() < 0.01,
        "GeoJSON lon should be ~13.4, got {}",
        wgs.lon
      );
    }

    // Plain JSON array without type marker should use [lat, lon] order
    let json_content = r#"{"coordinates": [52.5, 13.4]}"#;
    let parser = ContentAutoParser::new(json_content.to_string());
    let events = parser.parse();
    assert!(!events.is_empty(), "JSON should parse");
    if let MapEvent::Layer(layer) = &events[0]
      && let Geometry::Point(coord, _) = &layer.geometries[0]
    {
      // JSON [52.5, 13.4] means lat=52.5, lon=13.4
      let wgs = coord.as_wgs84();
      assert!(
        (wgs.lat - 52.5).abs() < 0.01,
        "JSON lat should be ~52.5, got {}",
        wgs.lat
      );
      assert!(
        (wgs.lon - 13.4).abs() < 0.01,
        "JSON lon should be ~13.4, got {}",
        wgs.lon
      );
    }
  }

  #[test]
  fn test_linewise_geojson_parsing() {
    use crate::map::geometry_collection::Geometry;

    // Multiple GeoJSON features, one per line (linewise format)
    let linewise_geojson = r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [10.0, 50.0]}, "properties": {"name": "Point 1"}}
{"type": "Feature", "geometry": {"type": "Point", "coordinates": [11.0, 51.0]}, "properties": {"name": "Point 2"}}
{"type": "Feature", "geometry": {"type": "Point", "coordinates": [12.0, 52.0]}, "properties": {"name": "Point 3"}}"#;

    let parser = ContentAutoParser::new(linewise_geojson.to_string());
    let events = parser.parse();

    assert!(!events.is_empty(), "Linewise GeoJSON should parse");
    if let MapEvent::Layer(layer) = &events[0] {
      assert_eq!(
        layer.geometries.len(),
        3,
        "Should have 3 points from linewise GeoJSON"
      );

      // Verify coordinates are parsed correctly (GeoJSON order: [lon, lat])
      if let Geometry::Point(coord, _) = &layer.geometries[0] {
        let wgs = coord.as_wgs84();
        assert!((wgs.lon - 10.0).abs() < 0.01, "First point lon should be 10.0");
        assert!((wgs.lat - 50.0).abs() < 0.01, "First point lat should be 50.0");
      }
    }
  }

  #[test]
  fn test_simple_coordinates_grep_parser() {
    use crate::map::geometry_collection::Geometry;

    // Simple coordinates without JSON structure should use grep parser with [lat, lon]
    // Each line with a single coordinate creates a point
    let coord_content = "52.5, 13.4";
    let parser = ContentAutoParser::new(coord_content.to_string());
    let events = parser.parse();

    assert!(!events.is_empty(), "Simple coordinates should parse");
    if let MapEvent::Layer(layer) = &events[0] {
      assert!(
        !layer.geometries.is_empty(),
        "Should have at least 1 geometry"
      );
      // Get the first point coordinate
      let coord = match &layer.geometries[0] {
        Geometry::Point(c, _) => c,
        Geometry::LineString(coords, _) => &coords[0],
        _ => panic!("Expected Point or LineString"),
      };
      let wgs = coord.as_wgs84();
      assert!(
        (wgs.lat - 52.5).abs() < 0.01,
        "Grep parser lat should be ~52.5, got {}",
        wgs.lat
      );
      assert!(
        (wgs.lon - 13.4).abs() < 0.01,
        "Grep parser lon should be ~13.4, got {}",
        wgs.lon
      );
    }
  }

  #[test]
  fn test_geojson_feature_collection_detection() {
    use crate::map::geometry_collection::Geometry;

    // Single-line FeatureCollection for content parser
    let feature_collection = r#"{"type": "FeatureCollection", "features": [{"type": "Feature", "geometry": {"type": "Point", "coordinates": [8.0, 48.0]}}, {"type": "Feature", "geometry": {"type": "Point", "coordinates": [9.0, 49.0]}}]}"#;

    let parser = ContentAutoParser::new(feature_collection.to_string());
    let events = parser.parse();

    assert!(!events.is_empty(), "FeatureCollection should parse");
    if let MapEvent::Layer(layer) = &events[0] {
      assert_eq!(layer.geometries.len(), 2, "Should have 2 features");

      // Verify GeoJSON coordinate order [lon, lat]
      if let Geometry::Point(coord, _) = &layer.geometries[0] {
        let wgs = coord.as_wgs84();
        assert!((wgs.lon - 8.0).abs() < 0.01, "lon should be 8.0");
        assert!((wgs.lat - 48.0).abs() < 0.01, "lat should be 48.0");
      }
    }
  }

  #[test]
  fn test_gpx_content_detection() {
    let gpx_content = r#"<?xml version="1.0"?>
<gpx version="1.1">
  <wpt lat="52.5" lon="13.4">
    <name>Berlin</name>
  </wpt>
</gpx>"#;

    let parser = ContentAutoParser::new(gpx_content.to_string());
    let events = parser.parse();

    assert!(!events.is_empty(), "GPX content should parse");
  }

  #[test]
  fn test_kml_content_detection() {
    let kml_content = r#"<?xml version="1.0"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Placemark>
    <name>Test</name>
    <Point>
      <coordinates>13.4,52.5,0</coordinates>
    </Point>
  </Placemark>
</kml>"#;

    let parser = ContentAutoParser::new(kml_content.to_string());
    let events = parser.parse();

    assert!(!events.is_empty(), "KML content should parse");
  }

  #[test]
  fn test_json_with_lat_lon_keys() {
    use crate::map::geometry_collection::Geometry;

    // JSON with explicit lat/lon keys
    let json_content = r#"{"latitude": 52.5, "longitude": 13.4, "label": "Berlin"}"#;
    let parser = ContentAutoParser::new(json_content.to_string());
    let events = parser.parse();

    assert!(!events.is_empty(), "JSON with lat/lon keys should parse");
    if let MapEvent::Layer(layer) = &events[0]
      && let Geometry::Point(coord, _) = &layer.geometries[0]
    {
      let wgs = coord.as_wgs84();
      assert!(
        (wgs.lat - 52.5).abs() < 0.01,
        "lat should be 52.5, got {}",
        wgs.lat
      );
      assert!(
        (wgs.lon - 13.4).abs() < 0.01,
        "lon should be 13.4, got {}",
        wgs.lon
      );
    }
  }

  #[test]
  fn test_mixed_content_fallback() {
    // Content that doesn't match any specific format should fall back to grep
    let mixed_content = "Some text with coordinates 52.5, 13.4 embedded in it";
    let parser = ContentAutoParser::new(mixed_content.to_string());
    let events = parser.parse();

    assert!(!events.is_empty(), "Mixed content should parse via grep fallback");
  }
}
