use std::path::PathBuf;

use crate::map::{
  color::Color,
  coordinates::{BoundingBox, PixelCoordinate, PixelPosition, PixelRect, Transform},
  viewport::set_coordinate_to_pixel,
};

pub(crate) fn color_to_color32(color: Color) -> egui::Color32 {
  let [r, g, b, a] = color.to_rgba_unmultiplied();
  egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub(crate) fn color32_to_color(color: egui::Color32) -> Color {
  Color::from_rgba_unmultiplied(color.r(), color.g(), color.b(), color.a())
}

pub(crate) fn pos_to_pixel(pos: egui::Pos2) -> PixelPosition {
  PixelPosition { x: pos.x, y: pos.y }
}

pub(crate) fn pixel_to_pos(pos: PixelPosition) -> egui::Pos2 {
  egui::pos2(pos.x, pos.y)
}

/// Converts a point, e.g. from a click, to a coordinate.
pub(crate) fn point_to_coordinate(point: PixelPosition, transform: &Transform) -> PixelCoordinate {
  let inv = transform.invert();
  inv.apply(point)
}

/// Shows a given bounding box on the map.
pub(crate) fn show_box(transform: &mut Transform, bb: &BoundingBox, rect: PixelRect) {
  if bb.is_valid() {
    let width_zoom: f32 = 1. / (bb.width() * transform.zoom / rect.width());
    let height_zoom: f32 = 1. / (bb.height() * transform.zoom / rect.height());
    transform.zoom(width_zoom.min(height_zoom));
    transform.zoom(0.95);
    set_coordinate_to_pixel(bb.center(), rect.center(), transform);
  }
}

/// Creating screenshot file names.
pub(crate) fn current_time_screenshot_name() -> PathBuf {
  format!(
    "mapvas_screenshot_{}.png",
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
  )
  .into()
}
