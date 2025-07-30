use super::Parser;
use serde::{Deserialize, Serialize};

use crate::map::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata},
  map_event::{Layer, MapEvent},
};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct KmlParser {
  #[serde(skip)]
  data: String,
}

impl KmlParser {
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

  fn parse_kml(&self) -> Vec<Geometry<PixelCoordinate>> {
    let mut geometries = Vec::new();
    
    let mut reader = kml::reader::KmlReader::<_, f64>::from_string(&self.data);
    match reader.read() {
      Ok(kml_data) => {
        self.extract_geometries_from_kml(&kml_data, &mut geometries);
      }
      Err(e) => {
        log::warn!("Error reading KML: {}", e);
      }
    }

    geometries
  }

  fn extract_geometries_from_kml(&self, kml: &kml::Kml<f64>, geometries: &mut Vec<Geometry<PixelCoordinate>>) {
    match kml {
      kml::Kml::Document { elements, .. } | kml::Kml::Folder { elements, .. } => {
        for element in elements {
          self.extract_geometries_from_kml(element, geometries);
        }
      }
      kml::Kml::Placemark(placemark) => {
        if let Some(geometry) = &placemark.geometry {
          self.convert_geometry(geometry, geometries);
        }
      }
      kml::Kml::Point(point) => {
        let coord = PixelCoordinate::from(WGS84Coordinate::new(
          point.coord.y as f32,
          point.coord.x as f32,
        ));
        geometries.push(Geometry::Point(coord, Metadata::default()));
      }
      kml::Kml::LineString(linestring) => {
        let line_points: Vec<PixelCoordinate> = linestring
          .coords
          .iter()
          .map(|coord| {
            PixelCoordinate::from(WGS84Coordinate::new(
              coord.y as f32,
              coord.x as f32,
            ))
          })
          .collect();
        
        if line_points.len() >= 2 {
          geometries.push(Geometry::LineString(line_points, Metadata::default()));
        }
      }
      kml::Kml::Polygon(polygon) => {
        if !polygon.outer.coords.is_empty() {
          let poly_points: Vec<PixelCoordinate> = polygon
            .outer
            .coords
            .iter()
            .map(|coord| {
              PixelCoordinate::from(WGS84Coordinate::new(
                coord.y as f32,
                coord.x as f32,
              ))
            })
            .collect();
          
          if poly_points.len() >= 3 {
            geometries.push(Geometry::Polygon(poly_points, Metadata::default()));
          }
        }
      }
      _ => {}
    }
  }

  fn convert_geometry(&self, geom: &kml::types::Geometry<f64>, geometries: &mut Vec<Geometry<PixelCoordinate>>) {
    match geom {
      kml::types::Geometry::Point(point) => {
        let coord = PixelCoordinate::from(WGS84Coordinate::new(
          point.coord.y as f32,
          point.coord.x as f32,
        ));
        geometries.push(Geometry::Point(coord, Metadata::default()));
      }
      kml::types::Geometry::LineString(linestring) => {
        let line_points: Vec<PixelCoordinate> = linestring
          .coords
          .iter()
          .map(|coord| {
            PixelCoordinate::from(WGS84Coordinate::new(
              coord.y as f32,
              coord.x as f32,
            ))
          })
          .collect();
        
        if line_points.len() >= 2 {
          geometries.push(Geometry::LineString(line_points, Metadata::default()));
        }
      }
      kml::types::Geometry::Polygon(polygon) => {
        if !polygon.outer.coords.is_empty() {
          let poly_points: Vec<PixelCoordinate> = polygon
            .outer
            .coords
            .iter()
            .map(|coord| {
              PixelCoordinate::from(WGS84Coordinate::new(
                coord.y as f32,
                coord.x as f32,
              ))
            })
            .collect();
          
          if poly_points.len() >= 3 {
            geometries.push(Geometry::Polygon(poly_points, Metadata::default()));
          }
        }
      }
      _ => {}
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

  #[test]
  fn test_kml_point() {
    let kml_data = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Placemark>
      <Point>
        <coordinates>10.0,52.0,0</coordinates>
      </Point>
    </Placemark>
  </Document>
</kml>"#;

    let mut parser = KmlParser::new();
    parser.parse_line(kml_data);
    let result = parser.finalize();
    assert!(result.is_some());
  }

  #[test]
  fn test_kml_linestring() {
    let kml_data = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Placemark>
      <LineString>
        <coordinates>10.0,52.0,0 10.1,52.1,0 10.2,52.2,0</coordinates>
      </LineString>
    </Placemark>
  </Document>
</kml>"#;

    let mut parser = KmlParser::new();
    parser.parse_line(kml_data);
    let result = parser.finalize();
    assert!(result.is_some());
  }
}