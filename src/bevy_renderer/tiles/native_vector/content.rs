use crate::map::{
  coordinates::{PixelCoordinate, Tile},
  tile_renderer::{
    TileRenderError, background_color, feature_is_national_capital, feature_string_property,
    get_fill_color, get_place_font_size, get_place_label_style, get_road_styling,
    label_feature_kind_detail, label_population_rank, should_show_place, style_config,
  },
};
use bevy::{
  asset::RenderAssetUsages,
  mesh::{Indices, PrimitiveTopology},
  prelude::*,
};

use super::super::{
  NativeVectorDebugFeature, NativeVectorDebugGeometry, NativeVectorTileBounds,
  NativeVectorTileContent, NativeVectorTileLabelSpec, NativeVectorTileMeshSpec,
};

const NATIVE_VECTOR_BACKGROUND_Z: f32 = -30.0;
const NATIVE_VECTOR_LAND_Z: f32 = -29.0;
const NATIVE_VECTOR_WATER_Z: f32 = -28.0;
const NATIVE_VECTOR_BUILDING_Z: f32 = -27.0;
const NATIVE_VECTOR_ROAD_CASING_Z: f32 = -26.0;
const NATIVE_VECTOR_ROAD_INNER_Z: f32 = -25.0;

struct NativeVectorMeshBatch {
  color_key: [u8; 4],
  color: Color,
  z_key: i16,
  z: f32,
  positions: Vec<[f32; 3]>,
  indices: Vec<u32>,
  bounds: Option<NativeVectorTileBounds>,
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub(in crate::bevy_renderer::tiles) fn native_vector_tile_content(
  tile: &Tile,
  style_zoom: u8,
  data: &[u8],
) -> Result<NativeVectorTileContent, TileRenderError> {
  let reader = mvt_reader::Reader::new(data.to_vec())
    .map_err(|e| TileRenderError::ParseError(format!("Failed to parse MVT for {tile:?}: {e}")))?;
  let layer_names = reader
    .get_layer_names()
    .map_err(|e| TileRenderError::ParseError(format!("Failed to get layers: {e}")))?;

  let cfg = style_config();
  let (tile_nw, tile_se) = tile.position();
  let tile_world_size = tile_se.x - tile_nw.x;
  let mvt_to_world = tile_world_size / cfg.rendering.mvt_extent;
  let style_pixel_to_world = tile_world_size / cfg.rendering.tile_size as f32;

  let mut batches = Vec::new();
  let mut labels = Vec::new();
  let mut debug_features = Vec::new();
  append_native_vector_rect(
    &mut batches,
    background_color(),
    NATIVE_VECTOR_BACKGROUND_Z,
    tile_nw,
    tile_world_size,
    tile_world_size,
  );

  let layer_order = [
    "landcover",
    "landuse",
    "park",
    "water",
    "waterway",
    "building",
    "buildings",
    "transportation",
    "road",
    "highway",
  ];

  for layer_name in &layer_order {
    if let Some(index) = layer_names.iter().position(|name| name == *layer_name) {
      append_native_vector_layer(
        &mut batches,
        &mut labels,
        &mut debug_features,
        &reader,
        index,
        layer_name,
        tile,
        style_zoom,
        tile_nw,
        tile_world_size,
        mvt_to_world,
        style_pixel_to_world,
      );
    }
  }

  for (index, layer_name) in layer_names.iter().enumerate() {
    if layer_order.contains(&layer_name.as_str()) {
      continue;
    }
    append_native_vector_layer(
      &mut batches,
      &mut labels,
      &mut debug_features,
      &reader,
      index,
      layer_name,
      tile,
      style_zoom,
      tile_nw,
      tile_world_size,
      mvt_to_world,
      style_pixel_to_world,
    );
  }

  Ok(NativeVectorTileContent {
    meshes: native_vector_batches_to_mesh_specs(batches, tile_nw),
    labels,
    debug_features,
  })
}

#[allow(clippy::too_many_arguments)]
fn append_native_vector_layer(
  batches: &mut Vec<NativeVectorMeshBatch>,
  labels: &mut Vec<NativeVectorTileLabelSpec>,
  debug_features: &mut Vec<NativeVectorDebugFeature>,
  reader: &mvt_reader::Reader,
  layer_index: usize,
  layer_name: &str,
  tile: &Tile,
  style_zoom: u8,
  tile_origin: PixelCoordinate,
  tile_world_size: f32,
  mvt_to_world: f32,
  style_pixel_to_world: f32,
) {
  let Ok(features) = reader.get_features(layer_index) else {
    return;
  };

  for (feature_index, feature) in features.into_iter().enumerate() {
    let feature_class = feature_string_property(&feature, &["class", "type"]).unwrap_or("");
    let feature_kind = feature_string_property(&feature, &["kind"]);
    let feature_kind_detail = label_feature_kind_detail(
      layer_name,
      feature_string_property(&feature, &["kind_detail"]),
      feature_kind,
      feature_class,
    );
    let feature_name = feature_string_property(&feature, &["name", "name:en", "name_en"]);
    let population_rank =
      label_population_rank(&feature, feature_kind_detail.unwrap_or(feature_class));
    let is_capital = feature_is_national_capital(&feature);
    if let Some(debug_feature) = native_vector_debug_feature(
      tile,
      feature_index,
      &feature,
      layer_name,
      feature_class,
      feature_kind,
      feature_kind_detail,
      feature_name,
      population_rank,
      is_capital,
      style_zoom,
      tile_origin,
      tile_world_size,
      mvt_to_world,
    ) {
      debug_features.push(debug_feature);
    }

    match &feature.geometry {
      geo_types::Geometry::Polygon(polygon) => {
        let polygon_class = polygon_feature_class(layer_name, feature_class, feature_kind);
        append_native_vector_polygon(
          batches,
          polygon,
          layer_name,
          polygon_class,
          tile_origin,
          mvt_to_world,
        );
      }
      geo_types::Geometry::MultiPolygon(multi) => {
        let polygon_class = polygon_feature_class(layer_name, feature_class, feature_kind);
        for polygon in multi.iter() {
          append_native_vector_polygon(
            batches,
            polygon,
            layer_name,
            polygon_class,
            tile_origin,
            mvt_to_world,
          );
        }
      }
      geo_types::Geometry::LineString(line) => {
        let line_class =
          line_feature_class(layer_name, feature_class, feature_kind, feature_kind_detail);
        append_native_vector_line(
          batches,
          line,
          layer_name,
          line_class,
          style_zoom,
          tile_origin,
          mvt_to_world,
          style_pixel_to_world,
        );
      }
      geo_types::Geometry::MultiLineString(multi) => {
        let line_class =
          line_feature_class(layer_name, feature_class, feature_kind, feature_kind_detail);
        for line in multi.iter() {
          append_native_vector_line(
            batches,
            line,
            layer_name,
            line_class,
            style_zoom,
            tile_origin,
            mvt_to_world,
            style_pixel_to_world,
          );
        }
      }
      geo_types::Geometry::Point(point) => {
        append_native_vector_label(
          labels,
          point,
          layer_name,
          feature_kind,
          feature_kind_detail,
          feature_name,
          population_rank,
          is_capital,
          tile.zoom,
          tile_origin,
          tile_world_size,
          mvt_to_world,
        );
      }
      geo_types::Geometry::MultiPoint(multi) => {
        for point in multi.iter() {
          append_native_vector_label(
            labels,
            point,
            layer_name,
            feature_kind,
            feature_kind_detail,
            feature_name,
            population_rank,
            is_capital,
            tile.zoom,
            tile_origin,
            tile_world_size,
            mvt_to_world,
          );
        }
      }
      _ => {}
    }
  }
}

#[allow(clippy::too_many_arguments)]
fn native_vector_debug_feature(
  tile: &Tile,
  feature_index: usize,
  feature: &mvt_reader::feature::Feature,
  layer_name: &str,
  feature_class: &str,
  feature_kind: Option<&str>,
  feature_kind_detail: Option<&str>,
  feature_name: Option<&str>,
  population_rank: Option<i64>,
  is_capital: bool,
  style_zoom: u8,
  tile_origin: PixelCoordinate,
  tile_world_size: f32,
  mvt_to_world: f32,
) -> Option<NativeVectorDebugFeature> {
  let (geometry_type, geometry, bounds) =
    native_vector_debug_geometry(&feature.geometry, tile_origin, mvt_to_world)?;
  Some(NativeVectorDebugFeature {
    tile: *tile,
    feature_index,
    source_layer: layer_name.to_string(),
    geometry_type,
    feature_class: feature_class.to_string(),
    feature_kind: feature_kind.map(str::to_string),
    feature_kind_detail: feature_kind_detail.map(str::to_string),
    feature_name: feature_name.map(str::to_string),
    style_summary: native_vector_debug_style_summary(
      &feature.geometry,
      layer_name,
      feature_class,
      feature_kind,
      feature_kind_detail,
      population_rank,
      is_capital,
      style_zoom,
      tile.zoom,
      tile_world_size,
    ),
    bounds,
    geometry,
    properties: native_vector_debug_properties(feature),
  })
}

fn native_vector_debug_geometry(
  geometry: &geo_types::Geometry<f32>,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
) -> Option<(String, NativeVectorDebugGeometry, NativeVectorTileBounds)> {
  match geometry {
    geo_types::Geometry::Point(point) => {
      let points = vec![native_vector_point_to_world(
        point,
        tile_origin,
        mvt_to_world,
      )];
      let bounds = native_vector_points_bounds(points.iter().copied())?;
      Some((
        "Point".to_string(),
        NativeVectorDebugGeometry::Points(points),
        bounds,
      ))
    }
    geo_types::Geometry::MultiPoint(points) => {
      let points = points
        .iter()
        .map(|point| native_vector_point_to_world(point, tile_origin, mvt_to_world))
        .collect::<Vec<_>>();
      let bounds = native_vector_points_bounds(points.iter().copied())?;
      Some((
        "MultiPoint".to_string(),
        NativeVectorDebugGeometry::Points(points),
        bounds,
      ))
    }
    geo_types::Geometry::LineString(line) => {
      let lines = vec![native_vector_line_to_world(line, tile_origin, mvt_to_world)];
      let bounds = native_vector_path_bounds(&lines)?;
      Some((
        "LineString".to_string(),
        NativeVectorDebugGeometry::Lines(lines),
        bounds,
      ))
    }
    geo_types::Geometry::MultiLineString(lines) => {
      let lines = lines
        .iter()
        .map(|line| native_vector_line_to_world(line, tile_origin, mvt_to_world))
        .collect::<Vec<_>>();
      let bounds = native_vector_path_bounds(&lines)?;
      Some((
        "MultiLineString".to_string(),
        NativeVectorDebugGeometry::Lines(lines),
        bounds,
      ))
    }
    geo_types::Geometry::Polygon(polygon) => {
      let rings = native_vector_polygon_to_world(polygon, tile_origin, mvt_to_world);
      let bounds = native_vector_path_bounds(&rings)?;
      Some((
        "Polygon".to_string(),
        NativeVectorDebugGeometry::Polygons(rings),
        bounds,
      ))
    }
    geo_types::Geometry::MultiPolygon(polygons) => {
      let rings = polygons
        .iter()
        .flat_map(|polygon| native_vector_polygon_to_world(polygon, tile_origin, mvt_to_world))
        .collect::<Vec<_>>();
      let bounds = native_vector_path_bounds(&rings)?;
      Some((
        "MultiPolygon".to_string(),
        NativeVectorDebugGeometry::Polygons(rings),
        bounds,
      ))
    }
    _ => None,
  }
}

fn native_vector_point_to_world(
  point: &geo_types::Point<f32>,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
) -> PixelCoordinate {
  PixelCoordinate {
    x: tile_origin.x + point.x() * mvt_to_world,
    y: tile_origin.y + point.y() * mvt_to_world,
  }
}

fn native_vector_line_to_world(
  line: &geo_types::LineString<f32>,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
) -> Vec<PixelCoordinate> {
  line
    .coords()
    .map(|coord| PixelCoordinate {
      x: tile_origin.x + coord.x * mvt_to_world,
      y: tile_origin.y + coord.y * mvt_to_world,
    })
    .collect()
}

fn native_vector_polygon_to_world(
  polygon: &geo_types::Polygon<f32>,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
) -> Vec<Vec<PixelCoordinate>> {
  std::iter::once(polygon.exterior())
    .chain(polygon.interiors())
    .map(|line| native_vector_line_to_world(line, tile_origin, mvt_to_world))
    .collect()
}

fn native_vector_path_bounds(paths: &[Vec<PixelCoordinate>]) -> Option<NativeVectorTileBounds> {
  native_vector_points_bounds(paths.iter().flat_map(|path| path.iter().copied()))
}

fn native_vector_points_bounds(
  points: impl IntoIterator<Item = PixelCoordinate>,
) -> Option<NativeVectorTileBounds> {
  let mut points = points.into_iter();
  let first = points.next()?;
  let mut bounds = NativeVectorTileBounds::from_min_max(first, first);
  for point in points {
    bounds.include(point);
  }
  Some(bounds)
}

#[allow(clippy::too_many_arguments)]
fn native_vector_debug_style_summary(
  geometry: &geo_types::Geometry<f32>,
  layer_name: &str,
  feature_class: &str,
  feature_kind: Option<&str>,
  feature_kind_detail: Option<&str>,
  population_rank: Option<i64>,
  is_capital: bool,
  style_zoom: u8,
  tile_zoom: u8,
  tile_world_size: f32,
) -> String {
  match geometry {
    geo_types::Geometry::Polygon(_) | geo_types::Geometry::MultiPolygon(_) => {
      let polygon_class = polygon_feature_class(layer_name, feature_class, feature_kind);
      if let Some(color) = get_fill_color(layer_name, polygon_class) {
        format!("fill class={polygon_class}, rgba={:?}", color.to_color_u8())
      } else {
        format!("fill class={polygon_class}, hidden")
      }
    }
    geo_types::Geometry::LineString(_) | geo_types::Geometry::MultiLineString(_) => {
      let line_class =
        line_feature_class(layer_name, feature_class, feature_kind, feature_kind_detail);
      let (_, casing_width, _, inner_width) = get_road_styling(layer_name, line_class, style_zoom);
      format!(
        "line class={line_class}, style_zoom={style_zoom}, casing={casing_width:.2}px, inner={inner_width:.2}px"
      )
    }
    geo_types::Geometry::Point(_) | geo_types::Geometry::MultiPoint(_) => {
      let show = should_show_place(feature_kind_detail, population_rank, is_capital, tile_zoom);
      let font_size = get_place_font_size(feature_kind_detail, is_capital, population_rank);
      let label_style = get_place_label_style(feature_kind_detail);
      format!(
        "label visible={show}, font={font_size:.1}px, marker={}, max_point_zoom={:?}, rank={population_rank:?}, national_capital={is_capital}, tile_world_size={tile_world_size:.2}",
        label_style.show_marker, label_style.max_point_zoom
      )
    }
    _ => "unsupported geometry".to_string(),
  }
}

fn native_vector_debug_properties(feature: &mvt_reader::feature::Feature) -> Vec<(String, String)> {
  let Some(properties) = &feature.properties else {
    return Vec::new();
  };
  let mut properties = properties
    .iter()
    .map(|(key, value)| (key.clone(), feature_value_to_string(value)))
    .collect::<Vec<_>>();
  properties.sort_by(|a, b| a.0.cmp(&b.0));
  properties
}

fn feature_value_to_string(value: &mvt_reader::feature::Value) -> String {
  match value {
    mvt_reader::feature::Value::String(value) => value.clone(),
    mvt_reader::feature::Value::Float(value) => value.to_string(),
    mvt_reader::feature::Value::Double(value) => value.to_string(),
    mvt_reader::feature::Value::Int(value) => value.to_string(),
    mvt_reader::feature::Value::UInt(value) => value.to_string(),
    mvt_reader::feature::Value::SInt(value) => value.to_string(),
    mvt_reader::feature::Value::Bool(value) => value.to_string(),
    mvt_reader::feature::Value::Null => "null".to_string(),
  }
}

fn polygon_feature_class<'a>(
  layer_name: &str,
  feature_class: &'a str,
  feature_kind: Option<&'a str>,
) -> &'a str {
  if layer_name == "landcover" || layer_name == "landuse" {
    feature_kind.unwrap_or(feature_class)
  } else {
    feature_class
  }
}

fn line_feature_class<'a>(
  layer_name: &str,
  feature_class: &'a str,
  feature_kind: Option<&'a str>,
  feature_kind_detail: Option<&'a str>,
) -> &'a str {
  if matches!(layer_name, "road" | "roads" | "transportation" | "highway") {
    feature_kind_detail.unwrap_or(feature_kind.unwrap_or(feature_class))
  } else {
    feature_class
  }
}

#[allow(clippy::too_many_arguments)]
fn append_native_vector_label(
  labels: &mut Vec<NativeVectorTileLabelSpec>,
  point: &geo_types::Point<f32>,
  layer_name: &str,
  feature_kind: Option<&str>,
  feature_kind_detail: Option<&str>,
  feature_name: Option<&str>,
  population_rank: Option<i64>,
  is_capital: bool,
  tile_zoom: u8,
  tile_origin: PixelCoordinate,
  tile_world_size: f32,
  mvt_to_world: f32,
) {
  let Some(label_text) = feature_name else {
    return;
  };
  let label_text = label_text.trim();
  if label_text.is_empty() {
    return;
  }

  if layer_name == "water" || feature_kind == Some("ocean") || feature_kind == Some("sea") {
    return;
  }
  if !should_show_place(feature_kind_detail, population_rank, is_capital, tile_zoom) {
    return;
  }
  let label_style = get_place_label_style(feature_kind_detail);

  labels.push(NativeVectorTileLabelSpec {
    coord: PixelCoordinate {
      x: tile_origin.x + point.x() * mvt_to_world,
      y: tile_origin.y + point.y() * mvt_to_world,
    },
    text: label_text.to_string(),
    base_font_size: get_place_font_size(feature_kind_detail, is_capital, population_rank),
    text_color: native_vector_color(label_style.text_color),
    show_marker: label_style.show_marker,
    centered: label_style.centered,
    max_point_zoom: label_style.max_point_zoom,
    tile_world_size,
  });
}

fn append_native_vector_polygon(
  batches: &mut Vec<NativeVectorMeshBatch>,
  polygon: &geo_types::Polygon<f32>,
  layer_name: &str,
  feature_class: &str,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
) {
  let Some(color) = get_fill_color(layer_name, feature_class) else {
    return;
  };
  if color.alpha() <= 0.0 {
    return;
  }
  let mut vertices = native_vector_ring_points(polygon.exterior(), mvt_to_world);
  if vertices.len() < 3 {
    return;
  }

  let mut hole_indices = Vec::new();
  for interior in polygon.interiors() {
    let points = native_vector_ring_points(interior, mvt_to_world);
    if points.len() < 3 {
      continue;
    }
    hole_indices.push(vertices.len() as u32);
    vertices.extend(points);
  }

  let mut indices = Vec::new();
  let mut earcut = earcut::Earcut::new();
  earcut.earcut(vertices.iter().copied(), &hole_indices, &mut indices);
  if indices.is_empty() {
    return;
  }

  let z = native_vector_polygon_z(layer_name);
  let batch = native_vector_mesh_batch(batches, color, z);
  let base_index = batch.positions.len() as u32;
  batch
    .positions
    .extend(vertices.iter().map(|coord| [coord[0], coord[1], 0.0]));
  batch
    .indices
    .extend(indices.into_iter().map(|index| base_index + index));
  for coord in vertices {
    include_native_vector_local_point(batch, tile_origin, coord[0], coord[1]);
  }
}

fn native_vector_ring_points(
  ring: &geo_types::LineString<f32>,
  mvt_to_world: f32,
) -> Vec<[f32; 2]> {
  let mut points = ring
    .coords()
    .map(|coord| [coord.x * mvt_to_world, coord.y * mvt_to_world])
    .collect::<Vec<_>>();
  if points
    .first()
    .zip(points.last())
    .is_some_and(|(first, last)| first[0] == last[0] && first[1] == last[1])
  {
    points.pop();
  }
  points
}

#[allow(clippy::too_many_arguments)]
fn append_native_vector_line(
  batches: &mut Vec<NativeVectorMeshBatch>,
  line: &geo_types::LineString<f32>,
  layer_name: &str,
  feature_class: &str,
  tile_zoom: u8,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
  style_pixel_to_world: f32,
) {
  if line.coords().count() < 2 {
    return;
  }
  let (casing_color, casing_width, inner_color, inner_width) =
    get_road_styling(layer_name, feature_class, tile_zoom);
  if casing_width > 0.0 {
    append_native_vector_line_mesh(
      batches,
      line,
      casing_color,
      casing_width * style_pixel_to_world,
      NATIVE_VECTOR_ROAD_CASING_Z,
      tile_origin,
      mvt_to_world,
    );
  }
  if inner_width > 0.0 {
    append_native_vector_line_mesh(
      batches,
      line,
      inner_color,
      inner_width * style_pixel_to_world,
      NATIVE_VECTOR_ROAD_INNER_Z,
      tile_origin,
      mvt_to_world,
    );
  }
}

fn append_native_vector_line_mesh(
  batches: &mut Vec<NativeVectorMeshBatch>,
  line: &geo_types::LineString<f32>,
  color: tiny_skia::Color,
  width: f32,
  z: f32,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
) {
  if color.alpha() <= 0.0 || width <= 0.0 {
    return;
  }
  let batch = native_vector_mesh_batch(batches, color, z);
  let half_width = width * 0.5;
  let points = line
    .coords()
    .map(|coord| [coord.x * mvt_to_world, coord.y * mvt_to_world])
    .collect::<Vec<_>>();

  for segment in points.windows(2) {
    let [x0, y0] = segment[0];
    let [x1, y1] = segment[1];
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f32::EPSILON {
      continue;
    }

    let nx = -dy / len * half_width;
    let ny = dx / len * half_width;
    let base_index = batch.positions.len() as u32;
    batch.positions.extend([
      [x0 + nx, y0 + ny, 0.0],
      [x1 + nx, y1 + ny, 0.0],
      [x1 - nx, y1 - ny, 0.0],
      [x0 - nx, y0 - ny, 0.0],
    ]);
    batch.indices.extend([
      base_index,
      base_index + 1,
      base_index + 2,
      base_index,
      base_index + 2,
      base_index + 3,
    ]);
    include_native_vector_local_point_with_padding(batch, tile_origin, x0, y0, half_width);
    include_native_vector_local_point_with_padding(batch, tile_origin, x1, y1, half_width);
  }
}

fn append_native_vector_rect(
  batches: &mut Vec<NativeVectorMeshBatch>,
  color: tiny_skia::Color,
  z: f32,
  tile_origin: PixelCoordinate,
  width: f32,
  height: f32,
) {
  let batch = native_vector_mesh_batch(batches, color, z);
  let base_index = batch.positions.len() as u32;
  batch.positions.extend([
    [0.0, 0.0, 0.0],
    [width, 0.0, 0.0],
    [width, height, 0.0],
    [0.0, height, 0.0],
  ]);
  batch.indices.extend([
    base_index,
    base_index + 1,
    base_index + 2,
    base_index,
    base_index + 2,
    base_index + 3,
  ]);
  include_native_vector_local_point(batch, tile_origin, 0.0, 0.0);
  include_native_vector_local_point(batch, tile_origin, width, height);
}

fn native_vector_mesh_batch(
  batches: &mut Vec<NativeVectorMeshBatch>,
  color: tiny_skia::Color,
  z: f32,
) -> &mut NativeVectorMeshBatch {
  let color_key = native_vector_color_key(color);
  let z_key = native_vector_z_key(z);
  if let Some(index) = batches
    .iter()
    .position(|batch| batch.color_key == color_key && batch.z_key == z_key)
  {
    return &mut batches[index];
  }

  batches.push(NativeVectorMeshBatch {
    color_key,
    color: native_vector_color(color),
    z_key,
    z,
    positions: Vec::new(),
    indices: Vec::new(),
    bounds: None,
  });
  batches.last_mut().expect("batch was pushed")
}

fn include_native_vector_local_point(
  batch: &mut NativeVectorMeshBatch,
  tile_origin: PixelCoordinate,
  x: f32,
  y: f32,
) {
  let coord = PixelCoordinate {
    x: tile_origin.x + x,
    y: tile_origin.y + y,
  };
  if let Some(bounds) = &mut batch.bounds {
    bounds.include(coord);
  } else {
    batch.bounds = Some(NativeVectorTileBounds::from_min_max(coord, coord));
  }
}

fn include_native_vector_local_point_with_padding(
  batch: &mut NativeVectorMeshBatch,
  tile_origin: PixelCoordinate,
  x: f32,
  y: f32,
  padding: f32,
) {
  include_native_vector_local_point(batch, tile_origin, x - padding, y - padding);
  include_native_vector_local_point(batch, tile_origin, x + padding, y + padding);
}

fn native_vector_batches_to_mesh_specs(
  batches: Vec<NativeVectorMeshBatch>,
  origin: PixelCoordinate,
) -> Vec<NativeVectorTileMeshSpec> {
  batches
    .into_iter()
    .filter_map(|batch| {
      if batch.positions.is_empty() || batch.indices.is_empty() {
        return None;
      }
      let bounds = batch.bounds?;
      let normals = vec![[0.0, 0.0, 1.0]; batch.positions.len()];
      let uvs = vec![[0.0, 0.0]; batch.positions.len()];
      let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
      )
      .with_inserted_indices(Indices::U32(batch.indices))
      .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, batch.positions)
      .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
      .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs);

      Some(NativeVectorTileMeshSpec {
        mesh,
        color: batch.color,
        bounds,
        origin,
        z: batch.z,
      })
    })
    .collect()
}

fn native_vector_polygon_z(layer_name: &str) -> f32 {
  match layer_name {
    "landcover" | "landuse" | "park" => NATIVE_VECTOR_LAND_Z,
    "water" | "waterway" => NATIVE_VECTOR_WATER_Z,
    "building" | "buildings" => NATIVE_VECTOR_BUILDING_Z,
    _ => NATIVE_VECTOR_LAND_Z,
  }
}

fn native_vector_z_key(z: f32) -> i16 {
  (z * 10.0).round() as i16
}

fn native_vector_color_key(color: tiny_skia::Color) -> [u8; 4] {
  let color = color.to_color_u8();
  [color.red(), color.green(), color.blue(), color.alpha()]
}

pub(super) fn native_vector_color(color: tiny_skia::Color) -> Color {
  let color = color.to_color_u8();
  Color::srgba(
    f32::from(color.red()) / 255.0,
    f32::from(color.green()) / 255.0,
    f32::from(color.blue()) / 255.0,
    f32::from(color.alpha()) / 255.0,
  )
}
