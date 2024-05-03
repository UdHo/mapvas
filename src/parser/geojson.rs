use std::iter::once;

use geo_types::GeometryCollection;
use geojson::{quick_collection, FeatureCollection, GeoJson, Geometry};

use crate::map::{
  coordinates::Coordinate,
  map_event::{MapEvent, Shape},
};

use super::Parser;

#[derive(Default)]
pub struct GeoJsonParser {
  data: String,
}

impl GeoJsonParser {
  pub fn new() -> Self {
    GeoJsonParser::default()
  }
}

fn to_coodinates(coord_vec: &[f64]) -> Option<Vec<Coordinate>> {
  if coord_vec.len() % 2 != 0 || coord_vec.is_empty() {
    None
  } else {
    Some(
      coord_vec
        .windows(2)
        .step_by(2)
        .map(|c| Coordinate {
          lat: c[1] as f32,
          lon: c[0] as f32,
        })
        .collect(),
    )
  }
}

fn parse_geometry(geometry: &Geometry) -> Option<Vec<Shape>> {
  let parse_single = |s: &Vec<f64>| {
    Some(Shape {
      coordinates: to_coodinates(s)?,
      ..Default::default()
    })
  };

  Some(match &geometry.value {
    geojson::Value::Point(p) => vec![parse_single(p)?],
    geojson::Value::MultiPoint(mp) => mp.iter().filter_map(parse_single).collect(),
    geojson::Value::LineString(_) => todo!(),
    geojson::Value::MultiLineString(_) => todo!(),
    geojson::Value::Polygon(_) => todo!(),
    geojson::Value::MultiPolygon(_) => todo!(),
    geojson::Value::GeometryCollection(_) => todo!(),
  })
}

fn iterate_geojson<'a>(geojson: &'a GeoJson) -> Box<dyn Iterator<Item = &Geometry> + 'a> {
  match geojson {
    GeoJson::Geometry(geometry) => Box::new(once(geometry)),
    GeoJson::Feature(feature) => todo!(),
    GeoJson::FeatureCollection(feature_collection) => todo!(),
  }
}

impl Parser for GeoJsonParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    self.data += line;
    None
  }

  fn finalize(&self) -> Option<MapEvent> {
    let geojson = self.data.parse::<GeoJson>().ok()?;

    eprintln!("{geojson:?}");

    None
  }
}
