use crate::map::{
  coordinates::{CANVAS_SIZE, PixelCoordinate, PixelPosition, PixelRect, Tile},
  tile_renderer::StyleConfig,
  viewport::MapViewport,
};
use bevy::prelude::*;

use super::super::surface::BevyRenderSurface;
use super::{NativeVectorTileBounds, NativeVectorTileLabelInstance};

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn native_vector_bounds_visible_wrap_offsets(
  bounds: NativeVectorTileBounds,
  viewport: MapViewport,
) -> Vec<f32> {
  let inv = viewport.transform.invert();
  let viewport_start = inv.apply(viewport.min());
  let viewport_end = inv.apply(viewport.max());
  let viewport_min_y = viewport_start.y.min(viewport_end.y);
  let viewport_max_y = viewport_start.y.max(viewport_end.y);
  if bounds.max_y < viewport_min_y || bounds.min_y > viewport_max_y {
    return Vec::new();
  }

  let viewport_min_x = viewport_start.x.min(viewport_end.x);
  let viewport_max_x = viewport_start.x.max(viewport_end.x);
  let min_copy = ((viewport_min_x - bounds.max_x) / CANVAS_SIZE - 1e-6).ceil() as i32;
  let max_copy = ((viewport_max_x - bounds.min_x) / CANVAS_SIZE + 1e-6).floor() as i32;
  if min_copy > max_copy {
    return Vec::new();
  }

  (min_copy..=max_copy)
    .map(|copy| copy as f32 * CANVAS_SIZE)
    .collect()
}

pub(super) fn native_vector_tile_transform(
  viewport: MapViewport,
  surface: BevyRenderSurface,
  origin: PixelCoordinate,
  wrap_offset: f32,
  z: f32,
) -> Transform {
  Transform::from_xyz(
    viewport.transform.trans.x + (origin.x + wrap_offset) * viewport.transform.zoom
      - surface.width() / 2.0,
    surface.height() / 2.0 - viewport.transform.trans.y - origin.y * viewport.transform.zoom,
    z,
  )
  .with_scale(Vec3::new(
    viewport.transform.zoom,
    -viewport.transform.zoom,
    1.0,
  ))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn native_vector_label_screen_positions(
  coord: PixelCoordinate,
  viewport: MapViewport,
) -> Vec<PixelPosition> {
  let inv = viewport.transform.invert();
  let viewport_start = inv.apply(viewport.min());
  let viewport_end = inv.apply(viewport.max());
  let viewport_min_y = viewport_start.y.min(viewport_end.y);
  let viewport_max_y = viewport_start.y.max(viewport_end.y);
  if coord.y < viewport_min_y || coord.y > viewport_max_y {
    return Vec::new();
  }

  let viewport_min_x = viewport_start.x.min(viewport_end.x);
  let viewport_max_x = viewport_start.x.max(viewport_end.x);
  let min_copy = ((viewport_min_x - coord.x) / CANVAS_SIZE - 1e-6).ceil() as i32;
  let max_copy = ((viewport_max_x - coord.x) / CANVAS_SIZE + 1e-6).floor() as i32;
  if min_copy > max_copy {
    return Vec::new();
  }

  let mut positions = Vec::with_capacity((max_copy - min_copy + 1) as usize);
  for copy in min_copy..=max_copy {
    let shifted = PixelCoordinate {
      x: coord.x + copy as f32 * CANVAS_SIZE,
      y: coord.y,
    };
    let screen = viewport.transform.apply(shifted);
    if viewport.rect.contains(screen) {
      positions.push(screen);
    }
  }
  positions
}

pub(super) fn native_vector_label_scale(
  viewport: MapViewport,
  label: &NativeVectorTileLabelInstance,
) -> f32 {
  (label.tile_world_size * viewport.transform.zoom / 256.0).max(0.0)
}

pub(super) fn native_vector_label_font_size(
  base_font_size: f32,
  scale: f32,
  cfg: &StyleConfig,
) -> f32 {
  (base_font_size * scale)
    .max(1.0)
    .min(cfg.font_sizes.max_font_size)
}

pub(super) fn native_vector_label_marker_radius(scale: f32, cfg: &StyleConfig) -> f32 {
  let max_radius = cfg.markers.max_radius.max(cfg.markers.base_radius);
  (cfg.markers.base_radius * scale).clamp(0.0, max_radius)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn coordinate_tile_rects(viewport: MapViewport, tile: Tile) -> Vec<PixelRect> {
  let (nw, se) = tile.position();
  let inv = viewport.transform.invert();
  let viewport_start = inv.apply(viewport.min());
  let viewport_end = inv.apply(viewport.max());
  let viewport_min_x = viewport_start.x.min(viewport_end.x);
  let viewport_max_x = viewport_start.x.max(viewport_end.x);
  let min_copy = ((viewport_min_x - se.x) / CANVAS_SIZE - 1e-6).ceil() as i32;
  let max_copy = ((viewport_max_x - nw.x) / CANVAS_SIZE + 1e-6).floor() as i32;

  if min_copy > max_copy {
    return Vec::new();
  }

  let mut tile_rects = Vec::with_capacity((max_copy - min_copy + 1) as usize);
  for copy in min_copy..=max_copy {
    let offset = copy as f32 * CANVAS_SIZE;
    let nw_shifted = PixelCoordinate {
      x: nw.x + offset,
      y: nw.y,
    };
    let se_shifted = PixelCoordinate {
      x: se.x + offset,
      y: se.y,
    };
    let (nw_screen, se_screen) = (
      viewport.transform.apply(nw_shifted),
      viewport.transform.apply(se_shifted),
    );
    let tile_rect = PixelRect::from_min_max(nw_screen, se_screen);
    if tile_rect.intersects(viewport.rect) {
      tile_rects.push(tile_rect);
    }
  }

  tile_rects
}

pub(super) fn transform_for_screen_rect(
  rect: PixelRect,
  surface: BevyRenderSurface,
  tile_zoom: u8,
) -> Transform {
  let z = -10.0 + f32::from(tile_zoom) * 0.001;
  transform_for_screen_rect_at_z(rect, surface, z)
}

pub(super) fn transform_for_screen_rect_at_z(
  rect: PixelRect,
  surface: BevyRenderSurface,
  z: f32,
) -> Transform {
  let center = rect.center();
  transform_for_screen_pos(center, surface, z)
}

pub(super) fn transform_for_screen_pos(
  pos: PixelPosition,
  surface: BevyRenderSurface,
  z: f32,
) -> Transform {
  let bevy_pos = screen_to_bevy_2d(pos, surface);
  Transform::from_xyz(bevy_pos.x, bevy_pos.y, z)
}

pub(super) fn screen_to_bevy_2d(screen: PixelPosition, surface: BevyRenderSurface) -> Vec2 {
  Vec2::new(
    screen.x - surface.width() / 2.0,
    surface.height() / 2.0 - screen.y,
  )
}
