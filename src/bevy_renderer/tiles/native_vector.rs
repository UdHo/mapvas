mod content;

use content::native_vector_color;
pub(super) use content::native_vector_tile_content;

use std::collections::{HashMap, HashSet};

use crate::map::{
  coordinates::{CANVAS_SIZE, PixelCoordinate, PixelPosition, Tile, TileCoordinate, TilePriority},
  tile_renderer::{StyleConfig, should_show_place_point_label, style_config},
  viewport::MapViewport,
};
use bevy::{prelude::*, sprite::Anchor};

use super::super::{map::BevyMapViewport, surface::BevyRenderSurface};
use super::{
  BevyNativeVectorTileLabelMarker, BevyNativeVectorTileLabelText, BevyNativeVectorTileMesh,
  BevyTileLayer, BevyTileOverlayGizmos, BevyTileRuntime, CoordinateDisplayMode,
  NativeVectorDebugFeature, NativeVectorDebugGeometry, NativeVectorDebugSelection,
  NativeVectorTileBounds, NativeVectorTileLabelCopy, NativeVectorTileLabelInstance,
  NativeVectorTileMeshInstance,
  fonts::BevyTileLabelFonts,
  screen::{
    native_vector_bounds_visible_wrap_offsets, native_vector_label_font_size,
    native_vector_label_marker_radius, native_vector_label_scale,
    native_vector_label_screen_positions, native_vector_tile_transform, screen_to_bevy_2d,
  },
};

const NATIVE_VECTOR_LABEL_MARKER_Z: f32 = -24.0;
const NATIVE_VECTOR_LABEL_TEXT_Z: f32 = -23.0;
const DEBUG_POINT_HIT_SCREEN_RADIUS: f32 = 8.0;
const DEBUG_LINE_HIT_SCREEN_RADIUS: f32 = 6.0;
const DEBUG_SELECTION_COLOR: Color = Color::srgb(1.0, 0.72, 0.0);

type NativeVectorLabelMarkerQuery<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static mut Transform,
    &'static mut Sprite,
    &'static mut Visibility,
  ),
  (
    With<BevyNativeVectorTileLabelMarker>,
    Without<BevyNativeVectorTileLabelText>,
  ),
>;

type NativeVectorLabelTextQuery<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static mut Transform,
    &'static mut Text2d,
    &'static mut TextFont,
    &'static mut TextColor,
    &'static mut Anchor,
    &'static mut Visibility,
  ),
  (
    With<BevyNativeVectorTileLabelText>,
    Without<BevyNativeVectorTileLabelMarker>,
  ),
>;

fn loaded_native_vector_tile_or_parent(layer: &BevyTileLayer, mut tile: Tile) -> Option<Tile> {
  loop {
    if layer.loaded_native_vector_tiles.contains_key(&tile) {
      return Some(tile);
    }
    tile = tile.parent()?;
  }
}

pub(super) fn select_debug_feature(
  layer: &BevyTileLayer,
  viewport: MapViewport,
  screen_pos: PixelPosition,
) -> Option<NativeVectorDebugSelection> {
  let click = viewport.transform.invert().apply(screen_pos);
  let click = PixelCoordinate {
    x: click.x.rem_euclid(CANVAS_SIZE),
    y: click.y,
  };
  let point_tolerance = DEBUG_POINT_HIT_SCREEN_RADIUS / viewport.transform.zoom.max(0.001);
  let line_tolerance = DEBUG_LINE_HIT_SCREEN_RADIUS / viewport.transform.zoom.max(0.001);
  let mut best = None::<(f32, NativeVectorDebugFeature)>;

  for entry in layer.loaded_native_vector_tiles.values() {
    for feature in &entry.debug_features {
      let Some(score) = debug_feature_hit_score(feature, click, point_tolerance, line_tolerance)
      else {
        continue;
      };
      if best
        .as_ref()
        .is_none_or(|(best_score, _)| score < *best_score)
      {
        best = Some((score, feature.clone()));
      }
    }
  }

  best.map(|(_, feature)| NativeVectorDebugSelection { click, feature })
}

pub(super) fn draw_native_vector_debug_selection(
  layer: Res<BevyTileLayer>,
  viewport: Res<BevyMapViewport>,
  surface: Res<BevyRenderSurface>,
  mut gizmos: Gizmos<BevyTileOverlayGizmos>,
) {
  if !layer.debug_elements_enabled {
    return;
  }
  let Some(selection) = &layer.selected_debug_feature else {
    return;
  };
  let Some(viewport) = viewport.get() else {
    return;
  };
  let surface = *surface;
  draw_debug_feature_geometry(&selection.feature, viewport, surface, &mut gizmos);
  draw_debug_feature_bounds(selection.feature.bounds, viewport, surface, &mut gizmos);
}

fn debug_feature_hit_score(
  feature: &NativeVectorDebugFeature,
  click: PixelCoordinate,
  point_tolerance: f32,
  line_tolerance: f32,
) -> Option<f32> {
  let bounds_tolerance = point_tolerance.max(line_tolerance);
  if !debug_bounds_contains(feature.bounds, click, bounds_tolerance) {
    return None;
  }

  match &feature.geometry {
    NativeVectorDebugGeometry::Points(points) => points
      .iter()
      .map(|point| point.sq_dist(&click))
      .filter(|distance| *distance <= point_tolerance * point_tolerance)
      .min_by(f32::total_cmp),
    NativeVectorDebugGeometry::Lines(lines) => lines
      .iter()
      .flat_map(|line| line.windows(2))
      .map(|segment| click.sq_distance_line_segment(&segment[0], &segment[1]))
      .filter(|distance| *distance <= line_tolerance * line_tolerance)
      .min_by(f32::total_cmp)
      .map(|distance| 1_000.0 + distance.sqrt()),
    NativeVectorDebugGeometry::Polygons(rings) => rings
      .iter()
      .any(|ring| point_in_ring(click, ring))
      .then(|| 2_000.0 + debug_bounds_area(feature.bounds) * 0.000_001),
  }
}

fn debug_bounds_contains(
  bounds: NativeVectorTileBounds,
  point: PixelCoordinate,
  padding: f32,
) -> bool {
  bounds.min_x - padding <= point.x
    && point.x <= bounds.max_x + padding
    && bounds.min_y - padding <= point.y
    && point.y <= bounds.max_y + padding
}

fn debug_bounds_area(bounds: NativeVectorTileBounds) -> f32 {
  (bounds.max_x - bounds.min_x).abs() * (bounds.max_y - bounds.min_y).abs()
}

fn point_in_ring(point: PixelCoordinate, ring: &[PixelCoordinate]) -> bool {
  if ring.len() < 3 {
    return false;
  }

  let mut inside = false;
  let mut previous = ring[ring.len() - 1];
  for current in ring {
    if (current.y > point.y) != (previous.y > point.y) {
      let x_intersection =
        (previous.x - current.x) * (point.y - current.y) / (previous.y - current.y) + current.x;
      if point.x < x_intersection {
        inside = !inside;
      }
    }
    previous = *current;
  }
  inside
}

fn draw_debug_feature_geometry(
  feature: &NativeVectorDebugFeature,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  let wrap_offsets = native_vector_bounds_visible_wrap_offsets(feature.bounds, viewport);
  match &feature.geometry {
    NativeVectorDebugGeometry::Points(points) => {
      for point in points {
        for wrap_offset in &wrap_offsets {
          draw_debug_point(
            shifted_debug_point(*point, *wrap_offset),
            viewport,
            surface,
            gizmos,
          );
        }
      }
    }
    NativeVectorDebugGeometry::Lines(lines) | NativeVectorDebugGeometry::Polygons(lines) => {
      for line in lines {
        for wrap_offset in &wrap_offsets {
          draw_debug_path(line, *wrap_offset, viewport, surface, gizmos);
        }
      }
    }
  }
}

fn draw_debug_feature_bounds(
  bounds: NativeVectorTileBounds,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  for wrap_offset in native_vector_bounds_visible_wrap_offsets(bounds, viewport) {
    let top_left = PixelCoordinate {
      x: bounds.min_x + wrap_offset,
      y: bounds.min_y,
    };
    let top_right = PixelCoordinate {
      x: bounds.max_x + wrap_offset,
      y: bounds.min_y,
    };
    let bottom_right = PixelCoordinate {
      x: bounds.max_x + wrap_offset,
      y: bounds.max_y,
    };
    let bottom_left = PixelCoordinate {
      x: bounds.min_x + wrap_offset,
      y: bounds.max_y,
    };
    draw_debug_world_line(top_left, top_right, viewport, surface, gizmos);
    draw_debug_world_line(top_right, bottom_right, viewport, surface, gizmos);
    draw_debug_world_line(bottom_right, bottom_left, viewport, surface, gizmos);
    draw_debug_world_line(bottom_left, top_left, viewport, surface, gizmos);
  }
}

fn draw_debug_path(
  path: &[PixelCoordinate],
  wrap_offset: f32,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  for segment in path.windows(2) {
    draw_debug_world_line(
      shifted_debug_point(segment[0], wrap_offset),
      shifted_debug_point(segment[1], wrap_offset),
      viewport,
      surface,
      gizmos,
    );
  }
}

fn draw_debug_point(
  point: PixelCoordinate,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  let screen = viewport.transform.apply(point);
  if !viewport.rect.contains(screen) {
    return;
  }
  let center = screen_to_bevy_2d(screen, surface);
  let radius = DEBUG_POINT_HIT_SCREEN_RADIUS;
  gizmos.line_2d(
    Vec2::new(center.x - radius, center.y),
    Vec2::new(center.x + radius, center.y),
    DEBUG_SELECTION_COLOR,
  );
  gizmos.line_2d(
    Vec2::new(center.x, center.y - radius),
    Vec2::new(center.x, center.y + radius),
    DEBUG_SELECTION_COLOR,
  );
}

fn draw_debug_world_line(
  from: PixelCoordinate,
  to: PixelCoordinate,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  let from_screen = viewport.transform.apply(from);
  let to_screen = viewport.transform.apply(to);
  if !viewport.rect.contains(from_screen) && !viewport.rect.contains(to_screen) {
    return;
  }
  gizmos.line_2d(
    screen_to_bevy_2d(from_screen, surface),
    screen_to_bevy_2d(to_screen, surface),
    DEBUG_SELECTION_COLOR,
  );
}

fn shifted_debug_point(point: PixelCoordinate, wrap_offset: f32) -> PixelCoordinate {
  PixelCoordinate {
    x: point.x + wrap_offset,
    y: point.y,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn debug_polygon_ring_hit_detects_inside_point() {
    let ring = vec![
      PixelCoordinate::new(0.0, 0.0),
      PixelCoordinate::new(10.0, 0.0),
      PixelCoordinate::new(10.0, 10.0),
      PixelCoordinate::new(0.0, 10.0),
    ];

    assert!(point_in_ring(PixelCoordinate::new(5.0, 5.0), &ring));
    assert!(!point_in_ring(PixelCoordinate::new(15.0, 5.0), &ring));
  }

  #[test]
  fn debug_line_hit_respects_tolerance() {
    let feature = debug_test_feature(NativeVectorDebugGeometry::Lines(vec![vec![
      PixelCoordinate::new(0.0, 0.0),
      PixelCoordinate::new(10.0, 0.0),
    ]]));

    assert!(debug_feature_hit_score(&feature, PixelCoordinate::new(5.0, 0.5), 1.0, 1.0).is_some());
    assert!(debug_feature_hit_score(&feature, PixelCoordinate::new(5.0, 2.0), 1.0, 1.0).is_none());
  }

  fn debug_test_feature(geometry: NativeVectorDebugGeometry) -> NativeVectorDebugFeature {
    NativeVectorDebugFeature {
      tile: Tile {
        x: 0,
        y: 0,
        zoom: 0,
      },
      feature_index: 0,
      source_layer: "test".to_string(),
      geometry_type: "LineString".to_string(),
      feature_class: "test".to_string(),
      feature_kind: None,
      feature_kind_detail: None,
      feature_name: None,
      style_summary: "test".to_string(),
      bounds: NativeVectorTileBounds::from_min_max(
        PixelCoordinate::new(0.0, 0.0),
        PixelCoordinate::new(10.0, 1.0),
      ),
      geometry,
      properties: Vec::new(),
    }
  }
}

pub(super) fn update_native_vector_tiles(
  mut commands: Commands,
  mut layer: ResMut<BevyTileLayer>,
  viewport: Res<BevyMapViewport>,
  runtime: Res<BevyTileRuntime>,
  surface: Res<BevyRenderSurface>,
  mut meshes: Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  let stale_entities = layer
    .stale_native_vector_entities
    .drain(..)
    .collect::<HashSet<_>>();
  for entity in &stale_entities {
    commands.entity(*entity).despawn();
  }

  if !layer.native_vector_tiles_active() {
    hide_native_vector_tile_meshes(&mut meshes);
    despawn_untracked_native_vector_tile_meshes(
      &mut commands,
      &tracked_native_vector_tile_entities(&layer),
      &stale_entities,
      &mut meshes,
    );
    return;
  }
  let Some(viewport) = viewport.get() else {
    hide_native_vector_tile_meshes(&mut meshes);
    despawn_untracked_native_vector_tile_meshes(
      &mut commands,
      &tracked_native_vector_tile_entities(&layer),
      &stale_entities,
      &mut meshes,
    );
    return;
  };
  let surface = *surface;

  if layer.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
    hide_native_vector_tile_meshes(&mut meshes);
    return;
  }

  let visible_tiles = layer.visible_tiles(viewport);
  let runtime_handle = runtime.0.clone();
  for tile in &visible_tiles {
    // Parent tiles are a draw fallback only; exact visible tiles still need a current-priority request.
    layer.request_native_vector_tile(*tile, TilePriority::Current, runtime_handle.clone());
  }
  layer.request_preload_native_vector_tiles(&visible_tiles, runtime_handle);

  let mut tiles_to_draw = visible_tiles
    .iter()
    .filter_map(|tile| loaded_native_vector_tile_or_parent(&layer, *tile))
    .collect::<Vec<_>>();
  tiles_to_draw.sort_unstable_by_key(|tile| tile.zoom);
  tiles_to_draw.dedup();
  tiles_to_draw.reverse();

  let draw_set: HashSet<Tile> = tiles_to_draw.iter().copied().collect();
  for tile in &tiles_to_draw {
    let entry = layer
      .loaded_native_vector_tiles
      .get_mut(tile)
      .expect("native vector tile was checked");
    for instance in &mut entry.instances {
      update_native_vector_tile_mesh_instance(
        &mut commands,
        viewport,
        surface,
        instance,
        &mut meshes,
      );
    }
  }

  for (tile, entry) in &mut layer.loaded_native_vector_tiles {
    if draw_set.contains(tile) {
      continue;
    }
    for instance in &entry.instances {
      hide_native_vector_tile_entities(&instance.entities, &mut meshes);
    }
  }

  despawn_untracked_native_vector_tile_meshes(
    &mut commands,
    &tracked_native_vector_tile_entities(&layer),
    &stale_entities,
    &mut meshes,
  );
}

pub(super) fn update_native_vector_tile_labels(
  mut commands: Commands,
  mut layer: ResMut<BevyTileLayer>,
  label_fonts: Res<BevyTileLabelFonts>,
  viewport: Res<BevyMapViewport>,
  surface: Res<BevyRenderSurface>,
  mut markers: NativeVectorLabelMarkerQuery<'_, '_>,
  mut texts: NativeVectorLabelTextQuery<'_, '_>,
) {
  if !layer.native_vector_tiles_active() {
    hide_native_vector_tile_labels(&mut markers, &mut texts);
    despawn_untracked_native_vector_tile_labels(
      &mut commands,
      &tracked_native_vector_tile_label_entities(&layer),
      &mut markers,
      &mut texts,
    );
    return;
  }
  let Some(viewport) = viewport.get() else {
    hide_native_vector_tile_labels(&mut markers, &mut texts);
    despawn_untracked_native_vector_tile_labels(
      &mut commands,
      &tracked_native_vector_tile_label_entities(&layer),
      &mut markers,
      &mut texts,
    );
    return;
  };
  let surface = *surface;
  if layer.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
    hide_native_vector_tile_labels(&mut markers, &mut texts);
    despawn_untracked_native_vector_tile_labels(
      &mut commands,
      &tracked_native_vector_tile_label_entities(&layer),
      &mut markers,
      &mut texts,
    );
    return;
  }

  let visible_tiles = if layer.last_visible_tiles.is_empty() {
    layer.visible_tiles(viewport)
  } else {
    layer.last_visible_tiles.clone()
  };
  let mut tiles_to_draw = visible_tiles
    .iter()
    .filter_map(|tile| loaded_native_vector_tile_or_parent(&layer, *tile))
    .collect::<Vec<_>>();
  tiles_to_draw.sort_unstable_by_key(|tile| tile.zoom);
  tiles_to_draw.dedup();
  tiles_to_draw.reverse();

  let cfg = style_config();
  let label_responsibilities = native_vector_label_responsibilities(&layer, &visible_tiles);
  let request_zoom = visible_tiles
    .first()
    .map_or(layer.current_request_zoom, |tile| tile.zoom);
  let map_zoom = layer.current_ideal_zoom;
  let draw_set: HashSet<Tile> = tiles_to_draw.iter().copied().collect();
  for tile in &tiles_to_draw {
    let entry = layer
      .loaded_native_vector_tiles
      .get_mut(tile)
      .expect("native vector tile was checked");
    for label in &mut entry.labels {
      if !should_show_place_point_label(label.max_point_zoom, map_zoom) {
        hide_native_vector_tile_label_entities(&label.entities, &mut markers, &mut texts);
        continue;
      }
      if !native_vector_label_is_responsible(label, request_zoom, label_responsibilities.get(tile))
      {
        hide_native_vector_tile_label_entities(&label.entities, &mut markers, &mut texts);
        continue;
      }
      update_native_vector_tile_label_instance(
        &mut commands,
        viewport,
        surface,
        label,
        &cfg,
        &label_fonts.map_label,
        &mut markers,
        &mut texts,
      );
    }
  }

  for (tile, entry) in &mut layer.loaded_native_vector_tiles {
    if draw_set.contains(tile) {
      continue;
    }
    for label in &entry.labels {
      hide_native_vector_tile_label_entities(&label.entities, &mut markers, &mut texts);
    }
  }

  despawn_untracked_native_vector_tile_labels(
    &mut commands,
    &tracked_native_vector_tile_label_entities(&layer),
    &mut markers,
    &mut texts,
  );
}

fn native_vector_label_responsibilities(
  layer: &BevyTileLayer,
  visible_tiles: &[Tile],
) -> HashMap<Tile, HashSet<Tile>> {
  let mut responsibilities = HashMap::<Tile, HashSet<Tile>>::new();
  for requested_tile in visible_tiles {
    let Some(draw_tile) = loaded_native_vector_tile_or_parent(layer, *requested_tile) else {
      continue;
    };
    responsibilities
      .entry(draw_tile)
      .or_default()
      .insert(*requested_tile);
  }
  responsibilities
}

fn native_vector_label_is_responsible(
  label: &NativeVectorTileLabelInstance,
  request_zoom: u8,
  responsible_tiles: Option<&HashSet<Tile>>,
) -> bool {
  let Some(responsible_tiles) = responsible_tiles else {
    return false;
  };
  let request_tile = native_vector_label_request_tile(label.coord, request_zoom);
  responsible_tiles.contains(&request_tile)
}

fn native_vector_label_request_tile(coord: PixelCoordinate, zoom: u8) -> Tile {
  Tile::from(TileCoordinate::from_pixel_position(
    PixelCoordinate {
      x: coord.x.rem_euclid(CANVAS_SIZE),
      y: coord.y,
    },
    zoom,
  ))
}

fn update_native_vector_tile_mesh_instance(
  commands: &mut Commands,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  instance: &mut NativeVectorTileMeshInstance,
  meshes: &mut Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  let wrap_offsets = native_vector_bounds_visible_wrap_offsets(instance.bounds, viewport);
  for (entity_index, wrap_offset) in wrap_offsets.iter().copied().enumerate() {
    let transform =
      native_vector_tile_transform(viewport, surface, instance.origin, wrap_offset, instance.z);
    if let Some(entity) = instance.entities.get(entity_index).copied() {
      if let Ok((_, mut mesh_transform, mut visibility)) = meshes.get_mut(entity) {
        *mesh_transform = transform;
        *visibility = Visibility::Visible;
      } else {
        instance.entities[entity_index] =
          spawn_native_vector_tile_mesh(commands, instance, transform);
      }
    } else {
      let entity = spawn_native_vector_tile_mesh(commands, instance, transform);
      instance.entities.push(entity);
    }
  }

  hide_native_vector_tile_entities(&instance.entities[wrap_offsets.len()..], meshes);
}

fn update_native_vector_tile_label_instance(
  commands: &mut Commands,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  label: &mut NativeVectorTileLabelInstance,
  cfg: &StyleConfig,
  label_font: &Handle<Font>,
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) {
  let positions = native_vector_label_screen_positions(label.coord, viewport);
  let scale = native_vector_label_scale(viewport, label);
  let font_size = native_vector_label_font_size(label.base_font_size, scale, cfg);
  let radius = native_vector_label_marker_radius(scale, cfg);
  let marker_size = Vec2::splat(radius * 2.0);
  let marker_color = native_vector_color(cfg.marker_dot.to_color());
  let text_offset_x = if label.show_marker {
    radius + cfg.markers.text_offset_x * scale
  } else {
    0.0
  };
  let text_anchor = if label.centered {
    Anchor::CENTER
  } else {
    Anchor::CENTER_LEFT
  };

  for (entity_index, screen_pos) in positions.iter().copied().enumerate() {
    let bevy_pos = screen_to_bevy_2d(screen_pos, surface);
    let marker_transform =
      Transform::from_xyz(bevy_pos.x, bevy_pos.y, NATIVE_VECTOR_LABEL_MARKER_Z);
    let text_transform = Transform::from_xyz(
      bevy_pos.x + text_offset_x,
      bevy_pos.y,
      NATIVE_VECTOR_LABEL_TEXT_Z,
    );
    let existing = label.entities.get(entity_index).copied();
    let marker = update_native_vector_tile_label_marker(
      commands,
      existing.map(|copy| copy.marker),
      marker_transform,
      marker_color,
      marker_size,
      label.show_marker,
      markers,
    );
    let text = update_native_vector_tile_label_text(
      commands,
      existing.map(|copy| copy.text),
      text_transform,
      &label.text,
      font_size,
      label.text_color,
      text_anchor,
      label_font,
      texts,
    );

    if let Some(copy) = label.entities.get_mut(entity_index) {
      copy.marker = marker;
      copy.text = text;
    } else {
      label
        .entities
        .push(NativeVectorTileLabelCopy { marker, text });
    }
  }

  hide_native_vector_tile_label_entities(&label.entities[positions.len()..], markers, texts);
}

fn update_native_vector_tile_label_marker(
  commands: &mut Commands,
  entity: Option<Entity>,
  transform: Transform,
  color: Color,
  size: Vec2,
  visible: bool,
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
) -> Entity {
  if let Some(entity) = entity {
    if let Ok((_, mut marker_transform, mut sprite, mut visibility)) = markers.get_mut(entity) {
      *marker_transform = transform;
      sprite.custom_size = Some(size);
      sprite.color = color;
      *visibility = if visible {
        Visibility::Visible
      } else {
        Visibility::Hidden
      };
      return entity;
    }
    commands.entity(entity).try_despawn();
  }

  commands
    .spawn((
      Sprite::from_color(color, size),
      transform,
      if visible {
        Visibility::Visible
      } else {
        Visibility::Hidden
      },
      BevyNativeVectorTileLabelMarker,
    ))
    .id()
}

fn update_native_vector_tile_label_text(
  commands: &mut Commands,
  entity: Option<Entity>,
  transform: Transform,
  value: &str,
  font_size: f32,
  color: Color,
  anchor: Anchor,
  label_font: &Handle<Font>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) -> Entity {
  if let Some(entity) = entity {
    if let Ok((
      _,
      mut text_transform,
      mut text,
      mut font,
      mut text_color,
      mut text_anchor,
      mut visibility,
    )) = texts.get_mut(entity)
    {
      *text_transform = transform;
      text.0.clear();
      text.0.push_str(value);
      font.font = label_font.clone();
      font.font_size = font_size;
      text_color.0 = color;
      *text_anchor = anchor;
      *visibility = Visibility::Visible;
      return entity;
    }
    commands.entity(entity).try_despawn();
  }

  commands
    .spawn((
      Text2d::new(value.to_string()),
      TextFont::from_font_size(font_size).with_font(label_font.clone()),
      TextColor(color),
      TextLayout::new_with_justify(Justify::Left),
      anchor,
      transform,
      BevyNativeVectorTileLabelText,
    ))
    .id()
}

fn spawn_native_vector_tile_mesh(
  commands: &mut Commands,
  instance: &NativeVectorTileMeshInstance,
  transform: Transform,
) -> Entity {
  commands
    .spawn((
      Mesh2d(instance.mesh.clone()),
      MeshMaterial2d(instance.material.clone()),
      transform,
      BevyNativeVectorTileMesh,
    ))
    .id()
}

fn hide_native_vector_tile_meshes(
  meshes: &mut Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  for (_, _, mut visibility) in meshes.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn hide_native_vector_tile_entities(
  entities: &[Entity],
  meshes: &mut Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  for entity in entities {
    if let Ok((_, _, mut visibility)) = meshes.get_mut(*entity) {
      *visibility = Visibility::Hidden;
    }
  }
}

fn hide_native_vector_tile_labels(
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) {
  for (_, _, _, mut visibility) in markers.iter_mut() {
    *visibility = Visibility::Hidden;
  }
  for (_, _, _, _, _, _, mut visibility) in texts.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn hide_native_vector_tile_label_entities(
  entities: &[NativeVectorTileLabelCopy],
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) {
  for copy in entities {
    if let Ok((_, _, _, mut visibility)) = markers.get_mut(copy.marker) {
      *visibility = Visibility::Hidden;
    }
    if let Ok((_, _, _, _, _, _, mut visibility)) = texts.get_mut(copy.text) {
      *visibility = Visibility::Hidden;
    }
  }
}

fn tracked_native_vector_tile_label_entities(layer: &BevyTileLayer) -> HashSet<Entity> {
  layer
    .loaded_native_vector_tiles
    .values()
    .flat_map(|entry| {
      entry.labels.iter().flat_map(|label| {
        label
          .entities
          .iter()
          .flat_map(|copy| [copy.marker, copy.text])
      })
    })
    .collect()
}

fn tracked_native_vector_tile_entities(layer: &BevyTileLayer) -> HashSet<Entity> {
  layer
    .loaded_native_vector_tiles
    .values()
    .flat_map(|entry| {
      entry
        .instances
        .iter()
        .flat_map(|instance| instance.entities.iter().copied())
    })
    .collect()
}

fn despawn_untracked_native_vector_tile_meshes(
  commands: &mut Commands,
  tracked_entities: &HashSet<Entity>,
  stale_entities: &HashSet<Entity>,
  meshes: &mut Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  for (entity, _, mut visibility) in meshes.iter_mut() {
    if tracked_entities.contains(&entity) || stale_entities.contains(&entity) {
      continue;
    }
    *visibility = Visibility::Hidden;
    commands.entity(entity).despawn();
  }
}

fn despawn_untracked_native_vector_tile_labels(
  commands: &mut Commands,
  tracked_entities: &HashSet<Entity>,
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) {
  for (entity, _, _, mut visibility) in markers.iter_mut() {
    if tracked_entities.contains(&entity) {
      continue;
    }
    *visibility = Visibility::Hidden;
    commands.entity(entity).try_despawn();
  }
  for (entity, _, _, _, _, _, mut visibility) in texts.iter_mut() {
    if tracked_entities.contains(&entity) {
      continue;
    }
    *visibility = Visibility::Hidden;
    commands.entity(entity).try_despawn();
  }
}
