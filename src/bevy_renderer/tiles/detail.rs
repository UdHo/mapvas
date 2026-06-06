use crate::map::coordinates::{Transform, tile_zoom_for_transform};

const BASE_TILE_DETAIL_FACTOR: f32 = 2.0;
pub(super) const MIN_TILE_DETAIL_FACTOR: f32 = 0.5;
pub(super) const MAX_TILE_DETAIL_FACTOR: f32 = 2.0;

pub(super) fn tile_zoom_with_detail_factor(transform: Transform, detail_factor: f32) -> u8 {
  let mut detail_transform = transform;
  detail_transform.zoom *= effective_tile_detail_factor(detail_factor);
  tile_zoom_for_transform(&detail_transform)
}

pub(super) fn native_vector_style_zoom(tile_zoom: u8, detail_factor: f32) -> u8 {
  tile_zoom.saturating_sub(tile_detail_zoom_offset(detail_factor))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn tile_detail_zoom_offset(detail_factor: f32) -> u8 {
  effective_tile_detail_factor(detail_factor)
    .max(1.0)
    .log2()
    .round()
    .clamp(0.0, f32::from(u8::MAX)) as u8
}

fn effective_tile_detail_factor(detail_factor: f32) -> f32 {
  BASE_TILE_DETAIL_FACTOR * clamped_tile_detail_factor(detail_factor)
}

pub(super) fn clamped_tile_detail_factor(detail_factor: f32) -> f32 {
  if detail_factor.is_finite() {
    detail_factor.clamp(MIN_TILE_DETAIL_FACTOR, MAX_TILE_DETAIL_FACTOR)
  } else {
    1.0
  }
}

pub(super) fn tile_detail_factor_label(detail_factor: f32) -> String {
  format!("{:.2}x", clamped_tile_detail_factor(detail_factor))
}

pub(super) fn effective_tile_detail_factor_label(detail_factor: f32) -> String {
  format!("{:.2}x", effective_tile_detail_factor(detail_factor))
}
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tile_detail_factor_maps_to_adjacent_zoom_levels() {
    let mut transform = crate::map::coordinates::Transform::default();
    transform.zoom = 2f32.powi(10);

    assert_eq!(tile_zoom_with_detail_factor(transform, 0.5), 12);
    assert_eq!(tile_zoom_with_detail_factor(transform, 1.0), 13);
    assert_eq!(tile_zoom_with_detail_factor(transform, 2.0), 14);
  }

  #[test]
  fn tile_detail_factor_is_clamped_to_supported_range() {
    assert_eq!(clamped_tile_detail_factor(0.25), 0.5);
    assert_eq!(clamped_tile_detail_factor(2.5), 2.0);
    assert_eq!(clamped_tile_detail_factor(f32::NAN), 1.0);
  }

  #[test]
  fn tile_detail_factor_label_formats_float_factor() {
    assert_eq!(tile_detail_factor_label(0.5), "0.50x");
    assert_eq!(tile_detail_factor_label(1.0), "1.00x");
    assert_eq!(tile_detail_factor_label(1.25), "1.25x");
    assert_eq!(tile_detail_factor_label(2.0), "2.00x");
  }

  #[test]
  fn effective_tile_detail_factor_includes_base_factor() {
    assert_eq!(effective_tile_detail_factor(0.5), 1.0);
    assert_eq!(effective_tile_detail_factor(1.0), 2.0);
    assert_eq!(effective_tile_detail_factor(2.0), 4.0);
  }

  #[test]
  fn native_vector_style_zoom_removes_detail_zoom_offset() {
    assert_eq!(native_vector_style_zoom(13, 0.5), 13);
    assert_eq!(native_vector_style_zoom(13, 1.0), 12);
    assert_eq!(native_vector_style_zoom(13, 2.0), 11);
  }
}
