use super::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::map::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata, Style, TimeData, TimeSpan},
  map_event::{Layer, MapEvent},
};
use chrono::{DateTime, Utc};
use egui::Color32;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct KmlParser {
  #[serde(skip)]
  data: String,
  #[serde(skip)]
  document_styles: HashMap<String, Style>,
}

impl KmlParser {
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

  fn parse_kml_color(color_str: &str) -> Option<Color32> {
    if color_str.len() != 8 {
      return None;
    }

    let alpha = u8::from_str_radix(&color_str[0..2], 16).ok()?;
    let blue = u8::from_str_radix(&color_str[2..4], 16).ok()?;
    let green = u8::from_str_radix(&color_str[4..6], 16).ok()?;
    let red = u8::from_str_radix(&color_str[6..8], 16).ok()?;

    Some(Color32::from_rgba_unmultiplied(red, green, blue, alpha))
  }

  fn collect_document_styles(&mut self, kml: &kml::Kml<f64>) {
    match kml {
      kml::Kml::KmlDocument(doc) => {
        for element in &doc.elements {
          self.collect_document_styles(element);
        }
      }
      kml::Kml::Document { elements, .. } => {
        for element in elements {
          self.collect_document_styles(element);
        }
      }
      kml::Kml::Folder(folder) => {
        for element in &folder.elements {
          self.collect_document_styles(element);
        }
      }
      kml::Kml::Style(style) => {
        if let Some(id) = &style.id {
          let parsed_style = Self::parse_kml_style(style);
          self.document_styles.insert(id.clone(), parsed_style);
        }
      }
      _ => {}
    }
  }

  fn parse_kml_style(kml_style: &kml::types::Style) -> Style {
    let mut style = Style::default();

    if let Some(line_style) = &kml_style.line {
      if let Some(parsed_color) = Self::parse_kml_color(&line_style.color) {
        style = style.with_color(parsed_color);
      }
    }

    if let Some(poly_style) = &kml_style.poly {
      if let Some(parsed_color) = Self::parse_kml_color(&poly_style.color) {
        style = style.with_fill_color(parsed_color);
      }
    }

    style
  }

  fn extract_style_from_placemark(&self, placemark: &kml::types::Placemark<f64>) -> Style {
    if let Some(style_url) = &placemark.style_url {
      let clean_url = style_url.trim_start_matches('#');
      if let Some(document_style) = self.document_styles.get(clean_url) {
        return document_style.clone();
      }
    }

    Style::default()
  }

  fn create_metadata_with_style(&self, placemark: &kml::types::Placemark<f64>) -> Metadata {
    let style = self.extract_style_from_placemark(placemark);
    let mut metadata = Metadata::default().with_style(style);

    if let Some(name) = &placemark.name {
      metadata = metadata.with_label(name.clone());
    }

    // Extract temporal information from the raw KML data
    if let Some(time_data) = self.extract_temporal_data(placemark) {
      metadata = metadata.with_time_data(time_data);
    }

    metadata
  }

  fn extract_temporal_data(&self, placemark: &kml::types::Placemark<f64>) -> Option<TimeData> {
    // Since the KML crate doesn't support temporal elements, we need to parse them manually
    // from the original XML data. Look for TimeStamp and TimeSpan elements within this placemark.

    // Find this placemark's section in the original XML data
    if let Some(name) = &placemark.name {
      if let Some(timestamp) = self.find_timestamp_in_xml(name) {
        return Some(TimeData {
          timestamp: Some(timestamp),
          time_span: None,
        });
      }

      if let Some(time_span) = self.find_timespan_in_xml(name) {
        return Some(TimeData {
          timestamp: None,
          time_span: Some(time_span),
        });
      }
    }

    None
  }

  fn find_timestamp_in_xml(&self, placemark_name: &str) -> Option<DateTime<Utc>> {
    // Look for <TimeStamp><when>...</when></TimeStamp> in a placemark with this name
    let name_pattern = format!("<name>{}</name>", placemark_name);

    if let Some(placemark_start) = self.data.find(&name_pattern) {
      // Find the end of this placemark
      let placemark_section = &self.data[placemark_start..];
      if let Some(placemark_end) = placemark_section.find("</Placemark>") {
        let placemark_xml = &placemark_section[..placemark_end];

        // Look for TimeStamp within this placemark
        if let Some(timestamp_start) = placemark_xml.find("<TimeStamp>") {
          if let Some(when_start) = placemark_xml[timestamp_start..].find("<when>") {
            let when_content_start = timestamp_start + when_start + 6; // Length of "<when>"
            if let Some(when_end) = placemark_xml[when_content_start..].find("</when>") {
              let when_content = &placemark_xml[when_content_start..when_content_start + when_end];
              return Self::parse_kml_datetime(when_content);
            }
          }
        }
      }
    }

    None
  }

  fn find_timespan_in_xml(&self, placemark_name: &str) -> Option<TimeSpan> {
    // Look for <TimeSpan><begin>...</begin><end>...</end></TimeSpan> in a placemark with this name
    let name_pattern = format!("<name>{}</name>", placemark_name);

    if let Some(placemark_start) = self.data.find(&name_pattern) {
      let placemark_section = &self.data[placemark_start..];
      if let Some(placemark_end) = placemark_section.find("</Placemark>") {
        let placemark_xml = &placemark_section[..placemark_end];

        // Look for TimeSpan within this placemark
        if let Some(timespan_start) = placemark_xml.find("<TimeSpan>") {
          let timespan_section = &placemark_xml[timespan_start..];
          if let Some(timespan_end) = timespan_section.find("</TimeSpan>") {
            let timespan_xml = &timespan_section[..timespan_end];

            let mut begin_time = None;
            let mut end_time = None;

            // Parse begin time
            if let Some(begin_start) = timespan_xml.find("<begin>") {
              let begin_content_start = begin_start + 7; // Length of "<begin>"
              if let Some(begin_end) = timespan_xml[begin_content_start..].find("</begin>") {
                let begin_content =
                  &timespan_xml[begin_content_start..begin_content_start + begin_end];
                begin_time = Self::parse_kml_datetime(begin_content);
              }
            }

            // Parse end time
            if let Some(end_start) = timespan_xml.find("<end>") {
              let end_content_start = end_start + 5; // Length of "<end>"
              if let Some(end_end) = timespan_xml[end_content_start..].find("</end>") {
                let end_content = &timespan_xml[end_content_start..end_content_start + end_end];
                end_time = Self::parse_kml_datetime(end_content);
              }
            }

            if begin_time.is_some() || end_time.is_some() {
              return Some(TimeSpan {
                begin: begin_time,
                end: end_time,
              });
            }
          }
        }
      }
    }

    None
  }

  fn parse_kml_datetime(date_str: &str) -> Option<DateTime<Utc>> {
    // KML supports various datetime formats:
    // - 1997-07-16T07:30:15Z (full date-time)
    // - 1997-07-16 (date only)
    // - 1997-07 (year-month)
    // - 1997 (year only)

    let trimmed = date_str.trim();

    // Try full datetime first
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
      return Some(dt.with_timezone(&Utc));
    }

    // Try date only (add default time)
    if trimmed.len() == 10
      && trimmed.chars().nth(4) == Some('-')
      && trimmed.chars().nth(7) == Some('-')
    {
      let datetime_str = format!("{}T00:00:00Z", trimmed);
      if let Ok(dt) = DateTime::parse_from_rfc3339(&datetime_str) {
        return Some(dt.with_timezone(&Utc));
      }
    }

    // Try year-month only
    if trimmed.len() == 7 && trimmed.chars().nth(4) == Some('-') {
      let datetime_str = format!("{}-01T00:00:00Z", trimmed);
      if let Ok(dt) = DateTime::parse_from_rfc3339(&datetime_str) {
        return Some(dt.with_timezone(&Utc));
      }
    }

    // Try year only
    if trimmed.len() == 4 && trimmed.chars().all(|c| c.is_ascii_digit()) {
      let datetime_str = format!("{}-01-01T00:00:00Z", trimmed);
      if let Ok(dt) = DateTime::parse_from_rfc3339(&datetime_str) {
        return Some(dt.with_timezone(&Utc));
      }
    }

    log::warn!("Failed to parse KML datetime: {}", date_str);
    None
  }

  fn parse_kml(&mut self) -> Vec<Geometry<PixelCoordinate>> {
    let mut reader = kml::reader::KmlReader::<_, f64>::from_string(&self.data);
    match reader.read() {
      Ok(kml_data) => {
        self.collect_document_styles(&kml_data);
        if let Some(geometry) = self.extract_geometry_from_kml(&kml_data) {
          vec![geometry]
        } else {
          vec![]
        }
      }
      Err(e) => {
        log::warn!("Error reading KML: {e}");
        vec![]
      }
    }
  }

  #[allow(clippy::cast_possible_truncation)]
  #[allow(clippy::too_many_lines)]
  fn extract_geometry_from_kml(&self, kml: &kml::Kml<f64>) -> Option<Geometry<PixelCoordinate>> {
    match kml {
      kml::Kml::KmlDocument(doc) => {
        let mut child_geometries = Vec::new();
        for element in &doc.elements {
          if let Some(geometry) = self.extract_geometry_from_kml(element) {
            child_geometries.push(geometry);
          }
        }
        if child_geometries.is_empty() {
          None
        } else {
          Some(Geometry::GeometryCollection(
            child_geometries,
            Metadata::default(),
          ))
        }
      }
      kml::Kml::Document { elements, attrs } => {
        let mut child_geometries = Vec::new();
        for element in elements {
          if let Some(geometry) = self.extract_geometry_from_kml(element) {
            child_geometries.push(geometry);
          }
        }
        if child_geometries.is_empty() {
          None
        } else {
          let mut metadata = Metadata::default();
          if let Some(name) = attrs.get("name") {
            metadata = metadata.with_label(name.clone());
          } else {
            for element in elements {
              if let kml::Kml::Element(el) = element {
                if el.name == "name" {
                  if let Some(text) = &el.content {
                    metadata = metadata.with_label(text.clone());
                    break;
                  }
                }
              }
            }
          }
          Some(Geometry::GeometryCollection(child_geometries, metadata))
        }
      }
      kml::Kml::Folder(folder) => {
        let mut child_geometries = Vec::new();
        for element in &folder.elements {
          if let Some(geometry) = self.extract_geometry_from_kml(element) {
            child_geometries.push(geometry);
          }
        }
        if child_geometries.is_empty() {
          None
        } else {
          let mut metadata = Metadata::default();
          if let Some(name) = &folder.name {
            metadata = metadata.with_label(name.clone());
          }
          Some(Geometry::GeometryCollection(child_geometries, metadata))
        }
      }
      kml::Kml::Placemark(placemark) => {
        if let Some(geometry) = &placemark.geometry {
          let metadata = self.create_metadata_with_style(placemark);
          Self::convert_geometry_with_metadata(geometry, metadata)
        } else {
          None
        }
      }
      kml::Kml::Point(point) => {
        let coord = PixelCoordinate::from(WGS84Coordinate::new(
          point.coord.y as f32,
          point.coord.x as f32,
        ));
        Some(Geometry::Point(coord, Metadata::default()))
      }
      kml::Kml::LineString(linestring) => {
        let line_points: Vec<PixelCoordinate> = linestring
          .coords
          .iter()
          .map(|coord| PixelCoordinate::from(WGS84Coordinate::new(coord.y as f32, coord.x as f32)))
          .collect();

        if line_points.len() >= 2 {
          Some(Geometry::LineString(line_points, Metadata::default()))
        } else {
          None
        }
      }
      kml::Kml::Polygon(polygon) => {
        if polygon.outer.coords.is_empty() {
          None
        } else {
          let poly_points: Vec<PixelCoordinate> = polygon
            .outer
            .coords
            .iter()
            .map(|coord| {
              PixelCoordinate::from(WGS84Coordinate::new(coord.y as f32, coord.x as f32))
            })
            .collect();

          if poly_points.len() >= 3 {
            Some(Geometry::Polygon(poly_points, Metadata::default()))
          } else {
            None
          }
        }
      }
      _ => None,
    }
  }

  #[allow(clippy::cast_possible_truncation)]
  fn convert_geometry_with_metadata(
    geom: &kml::types::Geometry<f64>,
    metadata: Metadata,
  ) -> Option<Geometry<PixelCoordinate>> {
    match geom {
      kml::types::Geometry::Point(point) => {
        let coord = PixelCoordinate::from(WGS84Coordinate::new(
          point.coord.y as f32,
          point.coord.x as f32,
        ));
        Some(Geometry::Point(coord, metadata))
      }
      kml::types::Geometry::LineString(linestring) => {
        let line_points: Vec<PixelCoordinate> = linestring
          .coords
          .iter()
          .map(|coord| PixelCoordinate::from(WGS84Coordinate::new(coord.y as f32, coord.x as f32)))
          .collect();

        if line_points.len() >= 2 {
          Some(Geometry::LineString(line_points, metadata))
        } else {
          None
        }
      }
      kml::types::Geometry::Polygon(polygon) => {
        if polygon.outer.coords.is_empty() {
          None
        } else {
          let poly_points: Vec<PixelCoordinate> = polygon
            .outer
            .coords
            .iter()
            .map(|coord| {
              PixelCoordinate::from(WGS84Coordinate::new(coord.y as f32, coord.x as f32))
            })
            .collect();

          if poly_points.len() >= 3 {
            Some(Geometry::Polygon(poly_points, metadata))
          } else {
            None
          }
        }
      }
      _ => None,
    }
  }
}

impl Parser for KmlParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    self.data.push_str(line);
    None
  }

  fn finalize(&mut self) -> Option<MapEvent> {
    let geometries = self.parse_kml();
    log::debug!("KML parser found {} geometries", geometries.len());
    if !geometries.is_empty() {
      let mut layer = Layer::new("kml".to_string());
      layer.geometries = geometries;
      return Some(MapEvent::Layer(layer));
    }
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::map::{
    coordinates::{PixelCoordinate, WGS84Coordinate},
    geometry_collection::{Geometry, Metadata, Style},
    map_event::MapEvent,
  };

  fn extract_leaf_geometries(geom: &Geometry<PixelCoordinate>) -> Vec<&Geometry<PixelCoordinate>> {
    match geom {
      Geometry::GeometryCollection(children, _) => {
        let mut results = Vec::new();
        for child in children {
          results.extend(extract_leaf_geometries(child));
        }
        results
      }
      _ => vec![geom],
    }
  }

  #[test]
  fn test_kml_point() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let kml_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/simple_point.kml"
    );
    let file = File::open(kml_path).expect("Could not open simple point KML file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = KmlParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "KML parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.geometries.len(), 1, "Should have 1 root geometry");

      let leaf_geometries = extract_leaf_geometries(&layer.geometries[0]);
      assert_eq!(leaf_geometries.len(), 1, "Should have 1 point geometry");

      let expected_coord = PixelCoordinate::from(WGS84Coordinate::new(52.0, 10.0));
      let expected_style = Style::default().with_color(egui::Color32::BLUE);
      let expected_metadata = Metadata::default()
        .with_label("Simple Point".to_string())
        .with_style(expected_style);
      let expected_geometry = Geometry::Point(expected_coord, expected_metadata);

      assert_eq!(
        leaf_geometries[0], &expected_geometry,
        "Point geometry should match expected"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_kml_linestring() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let kml_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/simple_linestring.kml"
    );
    let file = File::open(kml_path).expect("Could not open simple linestring KML file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = KmlParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "KML parser should produce events");
    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.geometries.len(), 1, "Should have 1 root geometry");

      let leaf_geometries = extract_leaf_geometries(&layer.geometries[0]);
      assert_eq!(
        leaf_geometries.len(),
        1,
        "Should have 1 linestring geometry"
      );

      let expected_coords = vec![
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.0, 10.0),
        ),
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.1, 10.1),
        ),
        crate::map::coordinates::PixelCoordinate::from(
          crate::map::coordinates::WGS84Coordinate::new(52.2, 10.2),
        ),
      ];
      let expected_style = Style::default().with_color(egui::Color32::BLUE);
      let expected_metadata = Metadata::default()
        .with_label("Simple Line".to_string())
        .with_style(expected_style);
      let expected_geometry = Geometry::LineString(expected_coords, expected_metadata);

      assert_eq!(
        leaf_geometries[0], &expected_geometry,
        "LineString geometry should match expected"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_comprehensive_kml_parsing() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let kml_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/comprehensive_test.kml"
    );
    let file = File::open(kml_path).expect("Could not open comprehensive test KML file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = KmlParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "KML parser should produce events");

    let layer_events: Vec<_> = events
      .iter()
      .filter(|event| matches!(event, MapEvent::Layer(_)))
      .collect();

    assert!(
      !layer_events.is_empty(),
      "Should produce at least one layer event"
    );
  }

  #[test]
  fn test_styled_kml_parsing() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let kml_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/styled_test.kml"
    );
    let file = File::open(kml_path).expect("Could not open styled test KML file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = KmlParser::new();
    let events: Vec<_> = parser.parse(reader).collect();
    assert!(
      !events.is_empty(),
      "Styled KML parser should produce events"
    );

    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.geometries.len(), 1, "Should have 1 root geometry");

      let leaf_geometries = extract_leaf_geometries(&layer.geometries[0]);
      assert!(
        leaf_geometries.len() >= 3,
        "Should have at least 3 leaf geometries"
      );

      let geometries_with_labels: Vec<_> = leaf_geometries
        .iter()
        .filter(|geom| match geom {
          Geometry::Point(_, metadata)
          | Geometry::LineString(_, metadata)
          | Geometry::Polygon(_, metadata)
          | Geometry::GeometryCollection(_, metadata) => metadata.label.is_some(),
        })
        .collect();

      assert!(
        !geometries_with_labels.is_empty(),
        "Some geometries should have labels"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_kml_color_parsing() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let kml_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/styled_linestring.kml"
    );
    let file = File::open(kml_path).expect("Could not open styled linestring KML file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = KmlParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "Should parse KML with styles");

    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(layer.geometries.len(), 1, "Should have 1 root geometry");

      let leaf_geometries = extract_leaf_geometries(&layer.geometries[0]);
      assert!(!leaf_geometries.is_empty(), "Should have geometries");

      if let Some(geometry) = leaf_geometries.first() {
        match geometry {
          Geometry::LineString(_, metadata) => {
            assert!(metadata.style.is_some(), "Geometry should have style");
            if let Some(style) = &metadata.style {
              let _color = style.color();
            }
          }
          _ => panic!("Expected LineString geometry"),
        }
      }
    }
  }

  #[test]
  fn test_kml_nested_folders() {
    use crate::parser::FileParser;
    use std::fs::File;
    use std::io::BufReader;

    let kml_path = concat!(
      env!("CARGO_MANIFEST_DIR"),
      "/tests/resources/nested_folders.kml"
    );
    let file = File::open(kml_path).expect("Could not open nested folders KML file");
    let reader = Box::new(BufReader::new(file));

    let mut parser = KmlParser::new();
    let events: Vec<_> = parser.parse(reader).collect();

    assert!(!events.is_empty(), "Should parse nested KML structure");

    if let Some(MapEvent::Layer(layer)) = events.first() {
      assert_eq!(
        layer.geometries.len(),
        1,
        "Should have 1 root geometry collection"
      );

      let nested_point_coord = PixelCoordinate::from(WGS84Coordinate::new(52.0, 10.0));
      let nested_point_style = Style::default().with_color(egui::Color32::BLUE);
      let nested_point_metadata = Metadata::default()
        .with_label("Nested Point".to_string())
        .with_style(nested_point_style.clone());
      let nested_point = Geometry::Point(nested_point_coord, nested_point_metadata);

      let outer_point_coord = PixelCoordinate::from(WGS84Coordinate::new(53.0, 11.0));
      let outer_point_metadata = Metadata::default()
        .with_label("Outer Point".to_string())
        .with_style(nested_point_style.clone());
      let outer_point = Geometry::Point(outer_point_coord, outer_point_metadata);

      let inner_folder = Geometry::GeometryCollection(
        vec![nested_point],
        Metadata::default().with_label("Inner Folder".to_string()),
      );

      let outer_folder = Geometry::GeometryCollection(
        vec![inner_folder, outer_point],
        Metadata::default().with_label("Outer Folder".to_string()),
      );

      let document = Geometry::GeometryCollection(vec![outer_folder], Metadata::default());

      let expected_root = Geometry::GeometryCollection(vec![document], Metadata::default());

      assert_eq!(
        layer.geometries[0], expected_root,
        "Nested KML structure should match expected geometry hierarchy"
      );
    } else {
      panic!("First event should be a Layer event");
    }
  }

  #[test]
  fn test_geometry_equality() {
    let coord1 = PixelCoordinate::new(10.0, 20.0);
    let coord2 = PixelCoordinate::new(10.0, 20.0);
    let coord3 = PixelCoordinate::new(10.1, 20.0);

    let metadata = Metadata::default().with_label("Test Point".to_string());
    let style = Style::default().with_color(egui::Color32::RED);
    let metadata_with_style = Metadata::default()
      .with_label("Test Point".to_string())
      .with_style(style);

    let geom1 = Geometry::Point(coord1, metadata.clone());
    let geom2 = Geometry::Point(coord2, metadata.clone());
    let geom3 = Geometry::Point(coord3, metadata.clone());
    let geom4 = Geometry::Point(coord1, metadata_with_style);

    assert_eq!(
      geom1, geom2,
      "Geometries with same coordinates and metadata should be equal"
    );
    assert_ne!(
      geom1, geom3,
      "Geometries with different coordinates should not be equal"
    );
    assert_ne!(
      geom1, geom4,
      "Geometries with different metadata should not be equal"
    );

    let collection1 =
      Geometry::GeometryCollection(vec![geom1.clone(), geom2.clone()], Metadata::default());
    let collection2 =
      Geometry::GeometryCollection(vec![geom1.clone(), geom2.clone()], Metadata::default());
    let collection3 =
      Geometry::GeometryCollection(vec![geom1.clone(), geom3.clone()], Metadata::default());

    assert_eq!(
      collection1, collection2,
      "Collections with same geometries should be equal"
    );
    assert_ne!(
      collection1, collection3,
      "Collections with different geometries should not be equal"
    );
  }
}
