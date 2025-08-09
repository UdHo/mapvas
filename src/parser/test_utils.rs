use crate::map::{
  coordinates::{PixelCoordinate, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata, Style},
  map_event::{Layer, MapEvent},
};
use crate::parser::FileParser;
use std::fs::File;
use std::io::BufReader;

/// Helper to load a test file and parse it with a given parser
#[must_use]
pub fn load_and_parse<P: FileParser + Default>(filename: &str) -> Vec<MapEvent> {
  let file_path = format!(
    "{}/tests/resources/{}",
    env!("CARGO_MANIFEST_DIR"),
    filename
  );
  let file =
    File::open(&file_path).unwrap_or_else(|_| panic!("Could not open test file: {file_path}"));
  let reader = Box::new(BufReader::new(file));

  let mut parser = P::default();
  parser.parse(reader).collect()
}

/// Extract the first layer from parsed events, panicking with clear messages if not found
#[must_use]
pub fn extract_layer<'a>(events: &'a [MapEvent], expected_layer_id: &str) -> &'a Layer {
  assert!(!events.is_empty(), "Parser should produce events");

  if let Some(MapEvent::Layer(layer)) = events.first() {
    assert_eq!(
      layer.id, expected_layer_id,
      "Layer should be named '{expected_layer_id}'"
    );
    layer
  } else {
    panic!("First event should be a Layer event");
  }
}

/// Assert that the actual layer equals the expected layer
pub fn assert_layer_eq(actual: &Layer, expected: &Layer) {
  assert_eq!(actual.id, expected.id, "Layer IDs should match");
  assert_eq!(
    actual.geometries.len(),
    expected.geometries.len(),
    "Layer should have {} geometries, but has {}",
    expected.geometries.len(),
    actual.geometries.len()
  );

  for (i, (actual_geom, expected_geom)) in actual
    .geometries
    .iter()
    .zip(&expected.geometries)
    .enumerate()
  {
    assert_eq!(
      actual_geom, expected_geom,
      "Geometry at index {i} should match expected"
    );
  }
}

/// Builder for creating test coordinates
pub struct CoordBuilder;

impl CoordBuilder {
  #[must_use]
  pub fn coord(lat: f32, lon: f32) -> PixelCoordinate {
    PixelCoordinate::from(WGS84Coordinate::new(lat, lon))
  }

  #[must_use]
  pub fn coords(coords: &[(f32, f32)]) -> Vec<PixelCoordinate> {
    coords
      .iter()
      .map(|(lat, lon)| Self::coord(*lat, *lon))
      .collect()
  }
}

/// Builder for creating test geometries
pub struct GeometryBuilder;

impl GeometryBuilder {
  #[must_use]
  pub fn point(lat: f32, lon: f32) -> Geometry<PixelCoordinate> {
    Geometry::Point(CoordBuilder::coord(lat, lon), Metadata::default())
  }

  #[must_use]
  pub fn point_with_metadata(lat: f32, lon: f32, metadata: Metadata) -> Geometry<PixelCoordinate> {
    Geometry::Point(CoordBuilder::coord(lat, lon), metadata)
  }

  #[must_use]
  pub fn linestring(coords: &[(f32, f32)]) -> Geometry<PixelCoordinate> {
    Geometry::LineString(CoordBuilder::coords(coords), Metadata::default())
  }

  #[must_use]
  pub fn linestring_with_metadata(
    coords: &[(f32, f32)],
    metadata: Metadata,
  ) -> Geometry<PixelCoordinate> {
    Geometry::LineString(CoordBuilder::coords(coords), metadata)
  }

  #[must_use]
  pub fn polygon(coords: &[(f32, f32)]) -> Geometry<PixelCoordinate> {
    Geometry::Polygon(CoordBuilder::coords(coords), Metadata::default())
  }

  #[must_use]
  pub fn polygon_with_metadata(
    coords: &[(f32, f32)],
    metadata: Metadata,
  ) -> Geometry<PixelCoordinate> {
    Geometry::Polygon(CoordBuilder::coords(coords), metadata)
  }
}

/// Builder for creating test styles
pub struct StyleBuilder;

impl StyleBuilder {
  #[must_use]
  pub fn with_color(color: egui::Color32) -> Style {
    Style::default().with_color(color)
  }

  #[must_use]
  pub fn with_colors(color: egui::Color32, fill_color: egui::Color32) -> Style {
    Style::default()
      .with_color(color)
      .with_fill_color(fill_color)
  }
}

/// Builder for creating test metadata
pub struct MetadataBuilder;

impl MetadataBuilder {
  #[must_use]
  pub fn with_label(label: &str) -> Metadata {
    Metadata::default().with_label(label.to_string())
  }

  #[must_use]
  pub fn with_label_and_style(label: &str, style: Style) -> Metadata {
    Metadata::default()
      .with_label(label.to_string())
      .with_style(style)
  }

  #[must_use]
  pub fn with_style(style: Style) -> Metadata {
    Metadata::default().with_style(style)
  }
}

/// Builder for creating expected test layers
pub struct LayerBuilder {
  id: String,
  geometries: Vec<Geometry<PixelCoordinate>>,
}

impl LayerBuilder {
  #[must_use]
  pub fn new(id: &str) -> Self {
    Self {
      id: id.to_string(),
      geometries: Vec::new(),
    }
  }

  #[must_use]
  pub fn with_geometry(mut self, geometry: Geometry<PixelCoordinate>) -> Self {
    self.geometries.push(geometry);
    self
  }

  #[must_use]
  pub fn with_geometries(mut self, geometries: Vec<Geometry<PixelCoordinate>>) -> Self {
    self.geometries.extend(geometries);
    self
  }

  #[must_use]
  pub fn build(self) -> Layer {
    let mut layer = Layer::new(self.id);
    layer.geometries = self.geometries;
    layer
  }
}

/// Macro for simple parser tests that compare entire layers
#[macro_export]
macro_rules! parser_layer_test {
  (
    $test_name:ident,
    $parser_type:ty,
    $file:literal,
    $layer_id:literal,
    $geometry:expr
  ) => {
    #[test]
    fn $test_name() {
      use $crate::parser::test_utils::{
        LayerBuilder, assert_layer_eq, extract_layer, load_and_parse,
      };

      let events = load_and_parse::<$parser_type>($file);
      let actual_layer = extract_layer(&events, $layer_id);
      let expected_layer = LayerBuilder::new($layer_id)
        .with_geometry($geometry)
        .build();
      assert_layer_eq(actual_layer, &expected_layer);
    }
  };
}
