// Vector tile renderer organized by feature type
//
// Module structure:
// - styling: Colors and styling functions for all features
// - labels: Text rendering (horizontal, rotated, along paths)
// - roads: Road rendering (placeholder for future enhancements)
// - water: Water body and waterway rendering (placeholder)
// - places: City/town/village markers (placeholder)
// - buildings: Building rendering (placeholder)
// - terrain: Landuse/landcover rendering (placeholder)

pub mod styling;
mod labels;
mod roads;
mod water;
mod places;
mod buildings;
mod terrain;

// Re-export key functions from submodules
use labels::{calculate_path_length, render_text, render_text_along_path};
use styling::{
  get_fill_color, get_place_font_size, get_road_styling, should_show_place, style_config,
  BACKGROUND_COLOR,
};

use egui::ColorImage;
use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::{TileRenderError, TileRenderer};
use crate::map::coordinates::Tile;


/// Vector tile renderer that parses MVT/PBF data and rasterizes it.
#[derive(Debug, Clone, Default)]
pub struct VectorTileRenderer;

impl VectorTileRenderer {
  #[must_use]
  pub fn new() -> Self {
    Self
  }

  #[allow(
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::cast_precision_loss,
    clippy::unnecessary_unwrap,
    clippy::items_after_statements,
    clippy::match_same_arms,
    clippy::cast_possible_wrap
  )]
  fn render_layer(
    pixmap: &mut Pixmap,
    reader: &mvt_reader::Reader,
    layer_index: usize,
    layer_name: &str,
    tile_zoom: u8,
    tile_size: u32,
  ) -> (
    Vec<(
      geo_types::Point<f32>,
      String,
      Option<String>,
      Option<String>,
      Option<String>,
      Option<i64>,
      bool,
    )>,
    Vec<(geo_types::LineString<f32>, String, String)>,
  ) {
    let layer_start = std::time::Instant::now();
    #[allow(clippy::cast_possible_truncation)]
    let scale = tile_size as f32 / style_config().rendering.mvt_extent;

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
    type PointFeature = (
      geo_types::Point<f32>,
      String,
      Option<String>,
      Option<String>,
      Option<String>,
      Option<i64>,
      bool,
    );
    let mut point_features: Vec<PointFeature> = Vec::new();

    // Collect linestring features with names for labeling (water and roads)
    // Tuple: (linestring, name, layer_name)
    let mut line_labels: Vec<(geo_types::LineString<f32>, String, String)> = Vec::new();

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
          props
            .get("name")
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
        .and_then(|v| match v {
          mvt_reader::feature::Value::UInt(n) => Some(*n as i64),
          mvt_reader::feature::Value::Int(n) => Some(*n),
          mvt_reader::feature::Value::SInt(n) => Some(*n),
          _ => None,
        });

      let is_capital = feature
        .properties
        .as_ref()
        .and_then(|props| props.get("capital"))
        .is_some();

      match &feature.geometry {
        geo_types::Geometry::Polygon(polygon) => {
          polygon_count += 1;

          // For Protomaps landcover/landuse layers: use 'kind' which has the actual feature type
          let polygon_class = if layer_name == "landcover" || layer_name == "landuse" {
            feature_kind.unwrap_or(feature_class)
          } else {
            feature_class
          };

          if polygon_count <= 2 {
            log::info!(
              "Layer '{}' polygon {} class='{}' kind={:?}: {} coords",
              layer_name,
              polygon_count,
              feature_class,
              feature_kind,
              polygon.exterior().coords().count()
            );
          }
          Self::render_polygon_with_class(pixmap, polygon, layer_name, polygon_class, scale);
        }
        geo_types::Geometry::MultiPolygon(multi) => {
          // For Protomaps landcover/landuse layers: use 'kind' which has the actual feature type
          let polygon_class = if layer_name == "landcover" || layer_name == "landuse" {
            feature_kind.unwrap_or(feature_class)
          } else {
            feature_class
          };

          for polygon in multi.iter() {
            polygon_count += 1;
            Self::render_polygon_with_class(pixmap, polygon, layer_name, polygon_class, scale);
          }
        }
        geo_types::Geometry::LineString(line) => {
          line_count += 1;

          // For Protomaps roads layer: use 'kind_detail' which has the actual road type
          let road_class = if layer_name == "roads" {
            feature_kind_detail.unwrap_or(feature_kind.unwrap_or(feature_class))
          } else {
            feature_class
          };

          Self::render_linestring_with_class(
            pixmap, line, layer_name, road_class, scale, tile_zoom,
          );

          // Collect water AND road features with names for labeling
          if feature_name.is_some()
            && (layer_name == "water" || layer_name == "waterway" || layer_name == "roads")
          {
            line_labels.push((
              line.clone(),
              feature_name.unwrap().to_string(),
              layer_name.to_string(),
            ));
          }
        }
        geo_types::Geometry::MultiLineString(multi) => {
          for line in multi.iter() {
            line_count += 1;

            // For Protomaps roads layer: use 'kind_detail' which has the actual road type
            let road_class = if layer_name == "roads" {
              feature_kind_detail.unwrap_or(feature_kind.unwrap_or(feature_class))
            } else {
              feature_class
            };

            Self::render_linestring_with_class(
              pixmap, line, layer_name, road_class, scale, tile_zoom,
            );

            // Collect water and road features with names for labeling (only first line of multi)
            if feature_name.is_some()
              && (layer_name == "water" || layer_name == "waterway" || layer_name == "roads")
              && line_count == 1
            {
              line_labels.push((
                line.clone(),
                feature_name.unwrap().to_string(),
                layer_name.to_string(),
              ));
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
    (point_features, line_labels)
  }


  fn render_polygon_with_class(
    pixmap: &mut Pixmap,
    polygon: &geo_types::Polygon<f32>,
    layer_name: &str,
    feature_class: &str,
    scale: f32,
  ) {
    let Some(color) = get_fill_color(layer_name, feature_class) else {
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
    feature_class: &str,
    scale: f32,
    tile_zoom: u8,
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
      // Determine road styling based on class, layer, and zoom level
      let (casing_color, casing_width, inner_color, inner_width) =
        get_road_styling(layer_name, feature_class, tile_zoom);

      // First pass: draw casing (border) if applicable
      if casing_width > 0.0 {
        let mut paint = Paint::default();
        paint.set_color(casing_color);
        paint.anti_alias = true;

        let stroke = Stroke {
          width: casing_width,
          ..Stroke::default()
        };

        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
      }

      // Second pass: draw inner road
      let mut paint = Paint::default();
      paint.set_color(inner_color);
      paint.anti_alias = true;

      let stroke = Stroke {
        width: inner_width,
        ..Stroke::default()
      };

      pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
  }

  #[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
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

      // Use centralized visibility check
      if !should_show_place(feature_kind_detail, population_rank, is_capital, tile_zoom) {
        return;
      }

      // Get font size from centralized config
      let cfg = style_config();
      let base_font_size = get_place_font_size(feature_kind_detail, is_capital, population_rank);

      // Scale font size based on tile resolution, but cap to prevent huge text
      let font_size =
        (base_font_size * (tile_size as f32 / 256.0)).min(cfg.font_sizes.max_font_size);

      // Draw small marker dot
      let radius =
        cfg.markers.base_radius * (tile_size as f32 / 256.0).min(cfg.markers.max_radius);
      let mut pb = PathBuilder::new();
      pb.push_circle(x, y, radius);

      if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(cfg.marker_dot.to_color());
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
      let text_x = x + radius + cfg.markers.text_offset_x;
      let text_y = y + font_size / cfg.markers.text_vertical_center_factor;

      render_text(
        pixmap,
        label_text,
        text_x,
        text_y,
        font_size,
        cfg.place_label.to_color(),
      );
    }
  }
}

impl TileRenderer for VectorTileRenderer {
  fn render(&self, tile: &Tile, data: &[u8]) -> Result<ColorImage, TileRenderError> {
    self.render_scaled(tile, data, 1)
  }

  #[allow(clippy::too_many_lines, clippy::items_after_statements, clippy::cast_precision_loss)]
  fn render_scaled(
    &self,
    tile: &Tile,
    data: &[u8],
    scale: u32,
  ) -> Result<ColorImage, TileRenderError> {
    let start = std::time::Instant::now();
    let scaled_size = style_config().rendering.tile_size * scale;
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

    // Collect all point features and water labels from all layers to render at the end
    type PointFeature = (
      geo_types::Point<f32>,
      String,
      Option<String>,
      Option<String>,
      Option<String>,
      Option<i64>,
      bool,
    );
    let mut all_point_features: Vec<PointFeature> = Vec::new();
    let mut all_line_labels: Vec<(geo_types::LineString<f32>, String, String)> = Vec::new();

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
        let (point_features, line_labels) = Self::render_layer(
          &mut pixmap,
          &reader,
          index,
          layer_name,
          tile.zoom,
          scaled_size,
        );
        all_point_features.extend(point_features);
        all_line_labels.extend(line_labels);
      }
    }

    // Render any remaining layers not in our order
    for (index, name) in layer_names.iter().enumerate() {
      if !layer_order.contains(&name.as_str()) {
        log::info!("Rendering extra layer '{name}' at index {index}");
        let (point_features, line_labels) =
          Self::render_layer(&mut pixmap, &reader, index, name, tile.zoom, scaled_size);
        all_point_features.extend(point_features);
        all_line_labels.extend(line_labels);
      }
    }

    // Now render all points and their labels on top of ALL geometry from ALL layers
    log::info!("Rendering {} point labels on top", all_point_features.len());
    for (
      point,
      point_layer_name,
      feature_kind,
      feature_kind_detail,
      feature_name,
      population_rank,
      is_capital,
    ) in all_point_features
    {
      #[allow(clippy::cast_possible_truncation)]
      let scale = scaled_size as f32 / style_config().rendering.mvt_extent;
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

    // Render water and road feature labels along paths
    log::info!(
      "Rendering {} water/road feature labels",
      all_line_labels.len()
    );
    let cfg = style_config();
    for (linestring, name, layer_name) in all_line_labels {
      #[allow(clippy::cast_possible_truncation)]
      let scale = scaled_size as f32 / cfg.rendering.mvt_extent;

      // Build scaled path coordinates
      let scaled_coords: Vec<(f32, f32)> = linestring
        .coords()
        .map(|c| (c.x * scale, c.y * scale))
        .collect();

      if scaled_coords.len() < 2 {
        continue; // Skip if not enough points
      }

      // Different colors for roads vs water features
      let (color, font_size_base) = if layer_name == "roads" {
        (cfg.road_label.to_color(), cfg.font_sizes.road_label)
      } else {
        (cfg.water_label.to_color(), cfg.font_sizes.water_label)
      };

      let font_size =
        font_size_base * (scaled_size as f32 / 256.0).min(cfg.font_sizes.max_label_scale);

      // Calculate path length
      let path_length = calculate_path_length(&scaled_coords);

      // Estimate text width
      let estimated_text_width =
        name.len() as f32 * font_size * cfg.font_sizes.char_width_ratio;

      // Skip if text is too long for the path
      if estimated_text_width > path_length * cfg.font_sizes.max_path_coverage {
        continue;
      }

      // Determine if we need to reverse the path (text should read left-to-right)
      let should_reverse = scaled_coords.first().map(|f| f.0) > scaled_coords.last().map(|l| l.0);

      let path_coords: Vec<(f32, f32)> = if should_reverse {
        scaled_coords.into_iter().rev().collect()
      } else {
        scaled_coords
      };

      // Center the text on the path
      let start_offset = (path_length - estimated_text_width) / 2.0;

      // Render text following the path
      render_text_along_path(
        &mut pixmap,
        &name,
        &path_coords,
        start_offset,
        font_size,
        color,
      );
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

  /// Helper function to get test artifacts directory
  fn test_artifacts_dir() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("test_artifacts");
    std::fs::create_dir_all(&path).unwrap();
    path
  }

  /// Helper to get tile data - tries cache first, then downloads
  #[allow(clippy::single_match)]
  async fn get_berlin_tile_data() -> Option<Vec<u8>> {
    let mut tile_path = test_artifacts_dir();
    tile_path.push("berlin_tile_15_17603_10747.mvt");

    // Try to load from cache first
    if tile_path.exists() {
      match std::fs::read(&tile_path) {
        Ok(data) => return Some(data),
        Err(_) => {} // Fall through to download
      }
    }

    // Try to download from Martin server (Berlin Protomaps)
    // Berlin center at zoom 15: x=17603, y=10747
    let url = "http://192.168.178.31:3000/tiles/{z}/{x}/{y}.mvt";
    let tile_url = url
      .replace("{z}", "15")
      .replace("{x}", "17603")
      .replace("{y}", "10747");

    match surf::get(&tile_url).await {
      Ok(mut response) => match response.body_bytes().await {
        Ok(data) => {
          // Save to cache for future runs
          let _ = std::fs::write(&tile_path, &data);
          Some(data)
        }
        Err(_) => None,
      },
      Err(_) => None,
    }
  }

  /// Test that renders a Berlin tile and saves it as reference
  /// Run this test first to generate the reference image:
  /// cargo test `test_render_berlin_tile_reference` --lib -- --nocapture
  #[tokio::test]
  #[ignore = "Manual test to generate reference image"]
  #[allow(clippy::single_match, clippy::manual_let_else, clippy::cast_possible_truncation)]
  async fn generate_berlin_tile_reference() {
    let tile_data = if let Some(data) = get_berlin_tile_data().await { data } else {
      eprintln!("⚠️  Skipping test: Berlin tile data not available.");
      eprintln!("   Place tile data at: test_artifacts/berlin_tile_15_17603_10747.mvt");
      eprintln!("   Or ensure Martin server is running at http://192.168.178.31:3000");
      return;
    };

    let renderer = VectorTileRenderer::new();
    let tile = Tile {
      x: 17603,
      y: 10747,
      zoom: 15,
    };

    // Render the tile
    let result = renderer.render(&tile, &tile_data);
    assert!(result.is_ok(), "Rendering should succeed");

    let color_image = result.unwrap();

    // Save as reference PNG
    let mut reference_path = test_artifacts_dir();
    reference_path.push("berlin_tile_15_17603_10747_reference.png");

    // Convert ColorImage to image::RgbaImage
    let width = color_image.width();
    let height = color_image.height();
    let pixels = color_image.as_raw();

    let img = image::RgbaImage::from_raw(width as u32, height as u32, pixels.to_vec())
      .expect("Failed to create image");

    img
      .save(&reference_path)
      .expect("Failed to save reference image");

    println!("✓ Reference image saved to: {}", reference_path.display());
    println!("  Size: {width}x{height}");
  }

  /// Test that renders the same Berlin tile and compares against reference
  /// This is a regression test to ensure rendering stays consistent
  ///
  /// Note: Run with --test-threads=1 to avoid race conditions with the reference test,
  /// or run `test_render_berlin_tile_reference` first to generate the reference image.
  #[tokio::test]
  #[allow(clippy::manual_let_else, clippy::manual_assert, clippy::cast_lossless, clippy::cast_precision_loss)]
  async fn test_render_berlin_tile_regression() {
    let tile_data = if let Some(data) = get_berlin_tile_data().await { data } else {
      eprintln!("⚠️  Skipping regression test: Berlin tile data not available.");
      return;
    };

    let renderer = VectorTileRenderer::new();
    let tile = Tile {
      x: 17603,
      y: 10747,
      zoom: 15,
    };

    // Render the tile
    let result = renderer.render(&tile, &tile_data);
    assert!(result.is_ok(), "Rendering should succeed");

    let color_image = result.unwrap();

    // Load reference image
    let mut reference_path = test_artifacts_dir();
    reference_path.push("berlin_tile_15_17603_10747_reference.png");

    assert!(reference_path.exists(), 
      "Reference image not found at {}. Run test_render_berlin_tile_reference first to generate it.",
      reference_path.display()
    );

    let reference_img = image::open(&reference_path)
      .expect("Failed to load reference image")
      .to_rgba8();

    // Compare dimensions
    assert_eq!(
      color_image.width(),
      reference_img.width() as usize,
      "Image width mismatch"
    );
    assert_eq!(
      color_image.height(),
      reference_img.height() as usize,
      "Image height mismatch"
    );

    // Compare pixels
    let rendered_pixels = color_image.as_raw();
    let reference_pixels = reference_img.as_raw();

    let mut diff_count = 0;
    let total_pixels = rendered_pixels.len() / 4;

    for i in 0..total_pixels {
      let idx = i * 4;
      let r_diff = (i32::from(rendered_pixels[idx]) - i32::from(reference_pixels[idx])).abs();
      let g_diff = (i32::from(rendered_pixels[idx + 1]) - i32::from(reference_pixels[idx + 1])).abs();
      let b_diff = (i32::from(rendered_pixels[idx + 2]) - i32::from(reference_pixels[idx + 2])).abs();
      let a_diff = (i32::from(rendered_pixels[idx + 3]) - i32::from(reference_pixels[idx + 3])).abs();

      // Allow small differences (up to 2 per channel) due to floating point precision
      if r_diff > 2 || g_diff > 2 || b_diff > 2 || a_diff > 2 {
        diff_count += 1;
      }
    }

    let diff_percentage = (f64::from(diff_count) / total_pixels as f64) * 100.0;

    // Allow up to 0.1% pixel differences for floating point tolerance
    assert!(
      diff_percentage < 0.1,
      "Too many pixel differences: {diff_percentage:.2}% ({diff_count} pixels out of {total_pixels})"
    );

    println!(
      "Regression test passed! Pixel differences: {diff_percentage:.4}% ({diff_count} pixels)"
    );
  }
}
