use std::sync::LazyLock;

use egui::ColorImage;
use log::debug;
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::{TileRenderError, TileRenderer};
use crate::map::coordinates::Tile;

const TILE_SIZE: u32 = 256;
const MVT_EXTENT: f32 = 4096.0;

// Helper function to create colors (since from_rgba8 is not const)
fn color(r: u8, g: u8, b: u8) -> Color {
  Color::from_rgba8(r, g, b, 255)
}

// Color constants for OSM-style rendering
static BACKGROUND_COLOR: LazyLock<Color> = LazyLock::new(|| color(242, 239, 233)); // Light cream
static WATER_COLOR: LazyLock<Color> = LazyLock::new(|| color(170, 211, 223)); // Blue
static LAND_COLOR: LazyLock<Color> = LazyLock::new(|| color(232, 227, 216)); // Slightly darker cream
static PARK_COLOR: LazyLock<Color> = LazyLock::new(|| color(200, 230, 180)); // Green
static BUILDING_COLOR: LazyLock<Color> = LazyLock::new(|| color(200, 190, 180)); // Tan/brown
static ROAD_COLOR: LazyLock<Color> = LazyLock::new(|| color(255, 255, 255)); // White
static ROAD_CASING_COLOR: LazyLock<Color> = LazyLock::new(|| color(180, 180, 180)); // Gray

/// Vector tile renderer that parses MVT/PBF data and rasterizes it.
#[derive(Debug, Clone, Default)]
pub struct VectorTileRenderer;

impl VectorTileRenderer {
  #[must_use]
  pub fn new() -> Self {
    Self
  }

  #[expect(clippy::too_many_lines)]
  fn render_layer(
    pixmap: &mut Pixmap,
    reader: &mvt_reader::Reader,
    layer_index: usize,
    layer_name: &str,
  ) {
    #[allow(clippy::cast_possible_truncation)] // TILE_SIZE (256) fits in u16
    let scale = f32::from(TILE_SIZE as u16) / MVT_EXTENT;

    let Ok(features) = reader.get_features(layer_index) else {
      log::warn!("Failed to get features for layer '{layer_name}'");
      return;
    };

    let mut feature_count = 0;
    let mut polygon_count = 0;
    let mut line_count = 0;
    let mut point_count = 0;

    for feature in features {
      feature_count += 1;
      match &feature.geometry {
        geo_types::Geometry::Polygon(polygon) => {
          polygon_count += 1;
          if polygon_count <= 2 {
            let coords: Vec<_> = polygon.exterior().coords().take(3).collect();
            log::info!(
              "Layer '{}' polygon {}: {} coords, first 3: {:?}",
              layer_name,
              polygon_count,
              polygon.exterior().coords().count(),
              coords
            );
          }
          Self::render_polygon(pixmap, polygon, layer_name, scale);
        }
        geo_types::Geometry::MultiPolygon(multi) => {
          for polygon in multi.iter() {
            polygon_count += 1;
            if polygon_count <= 2 {
              let coords: Vec<_> = polygon.exterior().coords().take(3).collect();
              log::info!(
                "Layer '{}' multipolygon {}: {} coords, first 3: {:?}",
                layer_name,
                polygon_count,
                polygon.exterior().coords().count(),
                coords
              );
            }
            Self::render_polygon(pixmap, polygon, layer_name, scale);
          }
        }
        geo_types::Geometry::LineString(line) => {
          line_count += 1;
          if line_count <= 2 {
            let coords: Vec<_> = line.coords().take(3).collect();
            log::info!(
              "Layer '{}' line {}: {} coords, first 3: {:?}",
              layer_name,
              line_count,
              line.coords().count(),
              coords
            );
          }
          Self::render_linestring(pixmap, line, layer_name, scale);
        }
        geo_types::Geometry::MultiLineString(multi) => {
          for line in multi.iter() {
            line_count += 1;
            if line_count <= 2 {
              let coords: Vec<_> = line.coords().take(3).collect();
              log::info!(
                "Layer '{}' multiline {}: {} coords, first 3: {:?}",
                layer_name,
                line_count,
                line.coords().count(),
                coords
              );
            }
            Self::render_linestring(pixmap, line, layer_name, scale);
          }
        }
        geo_types::Geometry::Point(point) => {
          point_count += 1;
          if point_count <= 2 {
            log::info!(
              "Layer '{}' point {}: ({}, {})",
              layer_name,
              point_count,
              point.x(),
              point.y()
            );
          }
          Self::render_point(pixmap, *point, layer_name, scale);
        }
        geo_types::Geometry::MultiPoint(multi) => {
          for point in multi.iter() {
            point_count += 1;
            if point_count <= 2 {
              log::info!(
                "Layer '{}' multipoint {}: ({}, {})",
                layer_name,
                point_count,
                point.x(),
                point.y()
              );
            }
            Self::render_point(pixmap, *point, layer_name, scale);
          }
        }
        other => {
          log::warn!(
            "Layer '{}' unsupported geometry: {:?}",
            layer_name,
            std::mem::discriminant(other)
          );
        }
      }
    }
    log::info!(
      "Layer '{layer_name}': {feature_count} features, {polygon_count} polygons, {line_count} lines, {point_count} points"
    );
  }

  fn get_fill_color(layer_name: &str) -> Option<Color> {
    match layer_name {
      "water" | "waterway" => Some(*WATER_COLOR),
      "landcover" | "landuse" => Some(*LAND_COLOR),
      "park" | "green" => Some(*PARK_COLOR),
      "building" => Some(*BUILDING_COLOR),
      _ => None,
    }
  }

  fn get_stroke_color(layer_name: &str) -> Color {
    match layer_name {
      "transportation" | "road" | "highway" => *ROAD_COLOR,
      "water" | "waterway" => *WATER_COLOR,
      _ => *ROAD_CASING_COLOR,
    }
  }

  fn get_stroke_width(layer_name: &str) -> f32 {
    match layer_name {
      "transportation" | "road" | "highway" => 2.0,
      "waterway" => 1.5,
      _ => 1.0,
    }
  }

  fn render_polygon(
    pixmap: &mut Pixmap,
    polygon: &geo_types::Polygon<f32>,
    layer_name: &str,
    scale: f32,
  ) {
    let Some(color) = Self::get_fill_color(layer_name) else {
      return;
    };

    let mut pb = PathBuilder::new();
    let exterior = polygon.exterior();

    if exterior.coords().count() < 3 {
      return;
    }

    let mut coords = exterior.coords();
    if let Some(first) = coords.next() {
      pb.move_to(first.x * scale, first.y * scale);
      for coord in coords {
        pb.line_to(coord.x * scale, coord.y * scale);
      }
      pb.close();
    }

    if let Some(path) = pb.finish() {
      let mut paint = Paint::default();
      paint.set_color(color);
      paint.anti_alias = true;
      pixmap.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::EvenOdd,
        Transform::identity(),
        None,
      );
    }
  }

  fn render_linestring(
    pixmap: &mut Pixmap,
    line: &geo_types::LineString<f32>,
    layer_name: &str,
    scale: f32,
  ) {
    if line.coords().count() < 2 {
      return;
    }

    let mut pb = PathBuilder::new();
    let mut coords = line.coords();

    if let Some(first) = coords.next() {
      pb.move_to(first.x * scale, first.y * scale);
      for coord in coords {
        pb.line_to(coord.x * scale, coord.y * scale);
      }
    }

    if let Some(path) = pb.finish() {
      let mut paint = Paint::default();
      paint.set_color(Self::get_stroke_color(layer_name));
      paint.anti_alias = true;

      let stroke = Stroke {
        width: Self::get_stroke_width(layer_name),
        ..Stroke::default()
      };

      pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
  }

  fn render_point(
    pixmap: &mut Pixmap,
    point: geo_types::Point<f32>,
    _layer_name: &str,
    scale: f32,
  ) {
    let x = point.x() * scale;
    let y = point.y() * scale;
    let radius = 3.0;

    let mut pb = PathBuilder::new();
    pb.push_circle(x, y, radius);

    if let Some(path) = pb.finish() {
      let mut paint = Paint::default();
      paint.set_color(Color::from_rgba8(100, 100, 100, 255));
      paint.anti_alias = true;
      pixmap.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        Transform::identity(),
        None,
      );
    }
  }
}

impl TileRenderer for VectorTileRenderer {
  fn render(&self, tile: &Tile, data: &[u8]) -> Result<ColorImage, TileRenderError> {
    log::info!(
      "VectorTileRenderer::render called for {:?}, data size: {} bytes",
      tile,
      data.len()
    );

    let reader = mvt_reader::Reader::new(data.to_vec()).map_err(|e| {
      log::error!("Failed to parse MVT for {tile:?}: {e}");
      TileRenderError::ParseError(format!("Failed to parse MVT for {tile:?}: {e}"))
    })?;

    log::info!("MVT parsed successfully for {tile:?}");

    let mut pixmap = Pixmap::new(TILE_SIZE, TILE_SIZE)
      .ok_or_else(|| TileRenderError::ParseError("Failed to create pixmap".to_string()))?;

    // Fill background
    pixmap.fill(*BACKGROUND_COLOR);

    // Get layer names and render in order (background layers first)
    let layer_names = reader
      .get_layer_names()
      .map_err(|e| TileRenderError::ParseError(format!("Failed to get layers: {e}")))?;

    log::info!(
      "Tile {:?} has {} layers: {:?}",
      tile,
      layer_names.len(),
      layer_names
    );

    // Render layers in a sensible order: land, water, parks, buildings, roads
    let layer_order = [
      "landcover",
      "landuse",
      "water",
      "waterway",
      "park",
      "building",
      "transportation",
      "road",
      "highway",
    ];

    for layer_name in &layer_order {
      if let Some(index) = layer_names.iter().position(|n| n == *layer_name) {
        debug!("Rendering layer '{layer_name}' at index {index}");
        Self::render_layer(&mut pixmap, &reader, index, layer_name);
      }
    }

    // Render any remaining layers not in our order
    for (index, name) in layer_names.iter().enumerate() {
      if !layer_order.contains(&name.as_str()) {
        debug!("Rendering extra layer '{name}' at index {index}");
        Self::render_layer(&mut pixmap, &reader, index, name);
      }
    }

    // Convert pixmap to ColorImage
    let pixels = pixmap.data();
    let size = [TILE_SIZE as usize, TILE_SIZE as usize];

    Ok(ColorImage::from_rgba_unmultiplied(size, pixels))
  }

  fn name(&self) -> &'static str {
    "Vector"
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_vector_renderer_name() {
    let renderer = VectorTileRenderer::new();
    assert_eq!(renderer.name(), "Vector");
  }

  #[test]
  fn test_vector_renderer_invalid_data() {
    let renderer = VectorTileRenderer::new();
    let tile = Tile {
      x: 0,
      y: 0,
      zoom: 0,
    };
    let result = renderer.render(&tile, &[0, 1, 2, 3]);
    assert!(result.is_err());
  }
}
