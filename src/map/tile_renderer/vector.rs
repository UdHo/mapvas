use std::sync::LazyLock;

use egui::ColorImage;
use fontdue::Font;
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::{TileRenderError, TileRenderer};
use crate::map::coordinates::Tile;

// Embedded font - using a basic sans-serif font
static FONT_DATA: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");

// Lazy-loaded font instance
static FONT: LazyLock<Font> = LazyLock::new(|| {
  Font::from_bytes(FONT_DATA, fontdue::FontSettings::default())
    .expect("Failed to load embedded font")
});

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

/// Renders text onto a pixmap at the given position.
/// Returns the width of the rendered text for positioning purposes.
fn render_text(pixmap: &mut Pixmap, text: &str, x: f32, y: f32, font_size: f32, color: Color) -> f32 {
  let font = &*FONT;

  // Get metrics for a typical capital letter to establish baseline
  // We'll use this to calculate a consistent baseline for all characters
  let (ref_metrics, _) = font.rasterize('A', font_size);
  let baseline_offset = ref_metrics.height as f32 + ref_metrics.ymin as f32;

  let mut total_width = 0.0_f32;
  let mut cursor_x = x;

  for ch in text.chars() {
    let (metrics, bitmap) = font.rasterize(ch, font_size);

    if bitmap.is_empty() {
      // Space or non-printable character
      cursor_x += metrics.advance_width;
      total_width += metrics.advance_width;
      continue;
    }

    // Calculate position with consistent baseline
    // All characters align to the same baseline derived from 'A'
    let glyph_x = (cursor_x + metrics.xmin as f32).round() as i32;
    let glyph_y = (y + baseline_offset - metrics.height as f32 - metrics.ymin as f32).round() as i32;

    // Draw glyph pixels
    for (i, &alpha) in bitmap.iter().enumerate() {
      if alpha == 0 {
        continue;
      }

      let px = glyph_x + (i % metrics.width) as i32;
      let py = glyph_y + (i / metrics.width) as i32;

      // Bounds check
      if px < 0 || py < 0 || px >= pixmap.width() as i32 || py >= pixmap.height() as i32 {
        continue;
      }

      // Blend the glyph with the existing pixel
      let pixel_idx = (py as u32 * pixmap.width() + px as u32) as usize * 4;
      if let Some(pixel_slice) = pixmap.data_mut().get_mut(pixel_idx..pixel_idx + 4) {
        let alpha_f = f32::from(alpha) / 255.0;
        let src_r = (color.red() * 255.0) as u8;
        let src_g = (color.green() * 255.0) as u8;
        let src_b = (color.blue() * 255.0) as u8;

        // Alpha blend
        pixel_slice[0] = ((1.0 - alpha_f) * f32::from(pixel_slice[0]) + alpha_f * f32::from(src_r)) as u8;
        pixel_slice[1] = ((1.0 - alpha_f) * f32::from(pixel_slice[1]) + alpha_f * f32::from(src_g)) as u8;
        pixel_slice[2] = ((1.0 - alpha_f) * f32::from(pixel_slice[2]) + alpha_f * f32::from(src_b)) as u8;
        pixel_slice[3] = 255; // Fully opaque
      }
    }

    cursor_x += metrics.advance_width;
    total_width += metrics.advance_width;
  }

  total_width
}

/// Vector tile renderer that parses MVT/PBF data and rasterizes it.
#[derive(Debug, Clone, Default)]
pub struct VectorTileRenderer;

impl VectorTileRenderer {
  #[must_use]
  pub fn new() -> Self {
    Self
  }

  fn render_layer(
    pixmap: &mut Pixmap,
    reader: &mvt_reader::Reader,
    layer_index: usize,
    layer_name: &str,
    _tile_zoom: u8,
    tile_size: u32,
  ) -> (
    Vec<(geo_types::Point<f32>, String, Option<String>, Option<String>, Option<String>, Option<i64>, bool)>,
    Vec<(geo_types::LineString<f32>, String, String)>,
  ) {
    let layer_start = std::time::Instant::now();
    #[allow(clippy::cast_possible_truncation)]
    let scale = tile_size as f32 / MVT_EXTENT;

    let Ok(features) = reader.get_features(layer_index) else {
      log::warn!("Failed to get features for layer '{layer_name}'");
      return (Vec::new(), Vec::new());
    };

    let mut feature_count = 0;
    let mut polygon_count = 0;
    let mut line_count = 0;
    let mut point_count = 0;

    // Collect all point features to render last (on top)
    // Tuple: (point, layer_name, feature_kind, feature_kind_detail, feature_name, population_rank, is_capital)
    type PointFeature = (geo_types::Point<f32>, String, Option<String>, Option<String>, Option<String>, Option<i64>, bool);
    let mut point_features: Vec<PointFeature> = Vec::new();

    // Collect all line features with names to render text labels
    // Tuple: (linestring, layer_name, feature_name)
    type LineFeature = (geo_types::LineString<f32>, String, String);
    let mut line_features: Vec<LineFeature> = Vec::new();

    for feature in features {
      feature_count += 1;

      // Get the feature class/type for better color selection
      let feature_class = feature
        .properties
        .as_ref()
        .and_then(|props| props.get("class"))
        .or_else(|| {
          feature
            .properties
            .as_ref()
            .and_then(|props| props.get("type"))
        })
        .and_then(|v| {
          if let mvt_reader::feature::Value::String(s) = v {
            Some(s.as_str())
          } else {
            None
          }
        })
        .unwrap_or("");

      // Extract name for labeling
      let feature_name = feature
        .properties
        .as_ref()
        .and_then(|props| {
          props.get("name")
            .or_else(|| props.get("name:en"))
            .or_else(|| props.get("name_en"))
        })
        .and_then(|v| {
          if let mvt_reader::feature::Value::String(s) = v {
            Some(s.as_str())
          } else {
            None
          }
        });

      // Extract Protomaps-specific properties for filtering
      let feature_kind = feature
        .properties
        .as_ref()
        .and_then(|props| props.get("kind"))
        .and_then(|v| {
          if let mvt_reader::feature::Value::String(s) = v {
            Some(s.as_str())
          } else {
            None
          }
        });

      let feature_kind_detail = feature
        .properties
        .as_ref()
        .and_then(|props| props.get("kind_detail"))
        .and_then(|v| {
          if let mvt_reader::feature::Value::String(s) = v {
            Some(s.as_str())
          } else {
            None
          }
        });

      let population_rank = feature
        .properties
        .as_ref()
        .and_then(|props| props.get("population_rank"))
        .and_then(|v| {
          match v {
            mvt_reader::feature::Value::UInt(n) => Some(*n as i64),
            mvt_reader::feature::Value::Int(n) => Some(*n),
            mvt_reader::feature::Value::SInt(n) => Some(*n),
            _ => None,
          }
        });

      let is_capital = feature
        .properties
        .as_ref()
        .and_then(|props| props.get("capital"))
        .is_some();

      match &feature.geometry {
        geo_types::Geometry::Polygon(polygon) => {
          polygon_count += 1;
          if polygon_count <= 2 {
            log::info!(
              "Layer '{}' polygon {} class='{}': {} coords",
              layer_name,
              polygon_count,
              feature_class,
              polygon.exterior().coords().count()
            );
          }
          Self::render_polygon_with_class(pixmap, polygon, layer_name, feature_class, scale);
        }
        geo_types::Geometry::MultiPolygon(multi) => {
          for polygon in multi.iter() {
            polygon_count += 1;
            Self::render_polygon_with_class(pixmap, polygon, layer_name, feature_class, scale);
          }
        }
        geo_types::Geometry::LineString(line) => {
          line_count += 1;
          Self::render_linestring_with_class(pixmap, line, layer_name, feature_class, scale);

          // Collect line features with names for text rendering
          if let Some(name) = feature_name {
            if (layer_name == "transportation" || layer_name == "road" || layer_name == "highway") && line.coords().count() >= 2 {
              line_features.push((line.clone(), layer_name.to_string(), name.to_string()));
            }
          }
        }
        geo_types::Geometry::MultiLineString(multi) => {
          for line in multi.iter() {
            line_count += 1;
            Self::render_linestring_with_class(pixmap, line, layer_name, feature_class, scale);

            // Collect line features with names for text rendering
            if let Some(name) = feature_name {
              if (layer_name == "transportation" || layer_name == "road" || layer_name == "highway") && line.coords().count() >= 2 {
                line_features.push((line.clone(), layer_name.to_string(), name.to_string()));
              }
            }
          }
        }
        geo_types::Geometry::Point(point) => {
          point_count += 1;
          // Collect points to render later (on top of everything)
          point_features.push((
            *point,
            layer_name.to_string(),
            feature_kind.map(String::from),
            feature_kind_detail.map(String::from),
            feature_name.map(String::from),
            population_rank,
            is_capital,
          ));
        }
        geo_types::Geometry::MultiPoint(multi) => {
          for point in multi.iter() {
            point_count += 1;
            // Collect points to render later (on top of everything)
            point_features.push((
              *point,
              layer_name.to_string(),
              feature_kind.map(String::from),
              feature_kind_detail.map(String::from),
              feature_name.map(String::from),
              population_rank,
              is_capital,
            ));
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

    let layer_time = layer_start.elapsed();
    log::info!(
      "Layer '{layer_name}': {feature_count} features ({polygon_count} polygons, {line_count} lines, {point_count} points) rendered in {layer_time:?}"
    );

    // Return collected features to be rendered later (on top of all layers)
    (point_features, line_features)
  }

  fn get_fill_color(layer_name: &str, feature_class: &str) -> Option<Color> {
    // Check feature class first for specific types
    match feature_class {
      // Water features
      "water" | "ocean" | "bay" | "lake" | "river" | "stream" => Some(*WATER_COLOR),
      // Green/park features
      "grass" | "forest" | "wood" | "park" | "garden" | "meadow" | "scrub" => Some(*PARK_COLOR),
      // Buildings
      "building" => Some(*BUILDING_COLOR),
      // Default to layer-based coloring
      _ => match layer_name {
        "water" | "waterway" => Some(*WATER_COLOR),
        "landcover" | "landuse" => Some(*LAND_COLOR),
        "park" | "green" => Some(*PARK_COLOR),
        "building" => Some(*BUILDING_COLOR),
        _ => None,
      },
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

  fn render_polygon_with_class(
    pixmap: &mut Pixmap,
    polygon: &geo_types::Polygon<f32>,
    layer_name: &str,
    feature_class: &str,
    scale: f32,
  ) {
    let Some(color) = Self::get_fill_color(layer_name, feature_class) else {
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

  fn render_linestring_with_class(
    pixmap: &mut Pixmap,
    line: &geo_types::LineString<f32>,
    layer_name: &str,
    _feature_class: &str,
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

  fn render_street_name(
    pixmap: &mut Pixmap,
    linestring: &geo_types::LineString<f32>,
    name: &str,
    scale: f32,
    tile_zoom: u8,
  ) {
    // Only render street names at higher zoom levels
    if tile_zoom < 14 {
      return;
    }

    let coords: Vec<_> = linestring.coords().collect();
    if coords.len() < 2 {
      return;
    }

    // Find the midpoint of the line
    let mid_idx = coords.len() / 2;
    let mid_point = coords[mid_idx];

    let x = mid_point.x * scale;
    let y = mid_point.y * scale;

    // Font size for street names (smaller than city names)
    let font_size = 8.0;

    // Render the street name at the midpoint
    // TODO: In the future, we could rotate the text to follow the line angle
    render_text(
      pixmap,
      name,
      x,
      y,
      font_size,
      Color::from_rgba8(80, 80, 80, 255), // Dark gray for street names
    );
  }

  fn render_point_with_class(
    pixmap: &mut Pixmap,
    point: geo_types::Point<f32>,
    layer_name: &str,
    feature_kind: Option<&str>,
    feature_kind_detail: Option<&str>,
    name: Option<&str>,
    population_rank: Option<i64>,
    is_capital: bool,
    tile_zoom: u8,
    scale: f32,
    tile_size: u32,
  ) {
    let x = point.x() * scale;
    let y = point.y() * scale;

    // Only render points with labels (skip unlabeled points to reduce clutter)
    if let Some(label_text) = name {
      // Skip water features (oceans, seas) - they clutter the map
      if layer_name == "water" || feature_kind == Some("ocean") || feature_kind == Some("sea") {
        return;
      }

      // Protomaps-based filtering using kind, kind_detail, and population_rank
      // population_rank: 13-17 (lower = more populous)
      // More lenient filtering to show cities earlier
      let should_show = match (feature_kind_detail, population_rank, is_capital, tile_zoom) {
        // Capitals always show from zoom 3+
        (_, _, true, z) if z >= 3 => true,

        // Cities by population rank - more lenient thresholds
        (Some("city"), Some(pr), _, z) if z < 5 => pr <= 14,   // Large cities (5M+)
        (Some("city"), Some(pr), _, z) if z < 7 => pr <= 15,   // Cities (1M+)
        (Some("city"), Some(pr), _, z) if z < 9 => pr <= 16,   // Medium cities (500K+)
        (Some("city"), Some(pr), _, z) if z >= 9 => pr <= 17,  // All cities
        (Some("city"), None, _, z) => z >= 8,                  // Cities without rank: zoom 8+

        // Localities (may include cities) - treat like cities
        (Some("locality"), Some(pr), _, z) if z < 5 => pr <= 14,
        (Some("locality"), Some(pr), _, z) if z < 7 => pr <= 15,
        (Some("locality"), Some(pr), _, z) if z < 9 => pr <= 16,
        (Some("locality"), Some(pr), _, z) if z >= 9 => pr <= 17,
        (Some("locality"), None, _, z) => z >= 8,

        // Towns and villages
        (Some("town"), _, _, z) => z >= 11,
        (Some("village"), _, _, z) => z >= 14,

        // Countries (always show from zoom 3)
        (Some("country"), _, _, z) => z >= 3,

        // Default: no label for other features
        _ => false,
      };

      if !should_show {
        return; // Skip this label
      }

      // Calculate base font size based on feature importance
      let base_font_size = match (feature_kind_detail, is_capital, population_rank) {
        // Countries - largest
        (Some("country"), _, _) => 14.0,

        // Capitals - very large
        (_, true, _) => 12.0,

        // Cities by population rank (13-17, lower = more populous)
        (Some("city"), _, Some(pr)) if pr <= 13 => 12.0,  // Megacities (10M+)
        (Some("city"), _, Some(pr)) if pr <= 14 => 11.0,  // Large cities (5M+)
        (Some("city"), _, Some(pr)) if pr <= 15 => 10.0,  // Cities (1M+)
        (Some("city"), _, Some(pr)) if pr <= 16 => 9.0,   // Medium cities (500K+)
        (Some("city"), _, Some(pr)) if pr <= 17 => 8.5,   // Smaller cities

        // Localities by population rank
        (Some("locality"), _, Some(pr)) if pr <= 14 => 11.0,
        (Some("locality"), _, Some(pr)) if pr <= 15 => 10.0,
        (Some("locality"), _, Some(pr)) if pr <= 16 => 9.0,
        (Some("locality"), _, Some(pr)) if pr <= 17 => 8.5,

        // Cities/localities without rank
        (Some("city"), _, None) => 9.0,
        (Some("locality"), _, None) => 9.0,

        // Towns - smaller
        (Some("town"), _, _) => 8.0,

        // Villages - smallest
        (Some("village"), _, _) => 7.5,

        // Default
        _ => 9.0,
      };

      // Scale font size based on tile resolution, but cap to prevent huge text
      let font_size = (base_font_size * (tile_size as f32 / 256.0)).min(20.0);

      // Draw small marker dot
      let radius = 2.0 * (tile_size as f32 / 256.0).min(4.0);
      let mut pb = PathBuilder::new();
      pb.push_circle(x, y, radius);

      if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(50, 50, 50, 255));
        paint.anti_alias = true;
        pixmap.fill_path(
          &path,
          &paint,
          tiny_skia::FillRule::Winding,
          Transform::identity(),
          None,
        );
      }

      // Render text label offset to the right of the point
      let text_x = x + radius + 2.0;
      let text_y = y + font_size / 3.0; // Vertically center text with point

      render_text(
        pixmap,
        label_text,
        text_x,
        text_y,
        font_size,
        Color::from_rgba8(0, 0, 0, 255),
      );
    }
  }
}

impl TileRenderer for VectorTileRenderer {
  fn render(&self, tile: &Tile, data: &[u8]) -> Result<ColorImage, TileRenderError> {
    self.render_scaled(tile, data, 1)
  }

  fn render_scaled(&self, tile: &Tile, data: &[u8], scale: u32) -> Result<ColorImage, TileRenderError> {
    let start = std::time::Instant::now();
    let scaled_size = TILE_SIZE * scale;
    log::info!(
      "VectorTileRenderer::render_scaled called for {tile:?}, data size: {} bytes, scale: {scale}x ({}x{})",
      data.len(),
      scaled_size,
      scaled_size
    );

    let reader = mvt_reader::Reader::new(data.to_vec()).map_err(|e| {
      log::error!("Failed to parse MVT for {tile:?}: {e}");
      TileRenderError::ParseError(format!("Failed to parse MVT for {tile:?}: {e}"))
    })?;

    let parse_time = start.elapsed();
    log::info!("MVT parsed successfully for {tile:?} in {parse_time:?}");

    let mut pixmap = Pixmap::new(scaled_size, scaled_size)
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

    // Collect all point features from all layers to render at the end
    type PointFeature = (geo_types::Point<f32>, String, Option<String>, Option<String>, Option<String>, Option<i64>, bool);
    let mut all_point_features: Vec<PointFeature> = Vec::new();

    // Collect all line features with names from all layers
    type LineFeature = (geo_types::LineString<f32>, String, String);
    let mut line_features: Vec<LineFeature> = Vec::new();

    // Render layers in a sensible order: land, parks, water (on top), buildings, roads
    let layer_order = [
      "landcover",
      "landuse",
      "park",
      "water",
      "waterway",
      "building",
      "transportation",
      "road",
      "highway",
    ];

    for layer_name in &layer_order {
      if let Some(index) = layer_names.iter().position(|n| n == *layer_name) {
        log::info!("Rendering ordered layer '{layer_name}' at index {index}");
        let (point_features, layer_line_features) = Self::render_layer(&mut pixmap, &reader, index, layer_name, tile.zoom, scaled_size);
        all_point_features.extend(point_features);
        line_features.extend(layer_line_features);
      }
    }

    // Render any remaining layers not in our order
    for (index, name) in layer_names.iter().enumerate() {
      if !layer_order.contains(&name.as_str()) {
        log::info!("Rendering extra layer '{name}' at index {index}");
        let (point_features, layer_line_features) = Self::render_layer(&mut pixmap, &reader, index, name, tile.zoom, scaled_size);
        all_point_features.extend(point_features);
        line_features.extend(layer_line_features);
      }
    }

    // Now render all points and their labels on top of ALL geometry from ALL layers
    log::info!("Rendering {} point labels on top", all_point_features.len());
    for (point, point_layer_name, feature_kind, feature_kind_detail, feature_name, population_rank, is_capital) in all_point_features {
      #[allow(clippy::cast_possible_truncation)]
      let scale = scaled_size as f32 / MVT_EXTENT;
      Self::render_point_with_class(
        &mut pixmap,
        point,
        &point_layer_name,
        feature_kind.as_deref(),
        feature_kind_detail.as_deref(),
        feature_name.as_deref(),
        population_rank,
        is_capital,
        tile.zoom,
        scale,
        scaled_size,
      );
    }

    // Render street names along the lines
    log::info!("Rendering {} street labels", line_features.len());
    #[allow(clippy::cast_possible_truncation)]
    let scale = scaled_size as f32 / MVT_EXTENT;
    for (linestring, _layer_name, street_name) in line_features {
      Self::render_street_name(&mut pixmap, &linestring, &street_name, scale, tile.zoom);
    }

    // Convert pixmap to ColorImage
    let pixels = pixmap.data();
    let size = [scaled_size as usize, scaled_size as usize];

    let total_time = start.elapsed();
    log::info!(
      "Tile {tile:?} rendered at {scale}x scale in {total_time:?} (parse: {parse_time:?}, render: {:?})",
      total_time.checked_sub(parse_time).unwrap()
    );

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
