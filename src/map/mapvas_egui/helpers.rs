use std::path::PathBuf;

use egui::Rect;

use crate::map::coordinates::{BoundingBox, PixelCoordinate, PixelPosition, Transform};

pub const MAX_ZOOM: f32 = 524_288.;
pub const MIN_ZOOM: f32 = 1.;

/// Sets a coordinate to the position in the map.
pub(crate) fn set_coordinate_to_pixel(
  coord: PixelCoordinate,
  cursor: PixelPosition,
  transform: &mut Transform,
) {
  let current_pos_in_gui = coordinate_to_point(coord, transform);
  transform.translate(current_pos_in_gui * (-1.) + cursor);
}

/// Converts a point, e.g. from a click, to a coordinate.
pub(crate) fn point_to_coordinate(point: PixelPosition, transform: &Transform) -> PixelCoordinate {
  let inv = transform.invert();
  inv.apply(point)
}

/// Converts a point to a coordinate.
pub(crate) fn coordinate_to_point(point: PixelCoordinate, transform: &Transform) -> PixelPosition {
  transform.apply(point)
}

/// Sets reasonable zoom defaults.
pub(crate) fn fit_to_screen(transform: &mut Transform, rect: &Rect) {
  transform.zoom = transform.zoom.clamp(MIN_ZOOM, MAX_ZOOM);

  let inv = transform.invert();
  let PixelCoordinate { x, y } = inv.apply(PixelPosition { x: 0., y: 0. });
  if x < 0. || y < 0. {
    transform.translate(
      PixelPosition {
        x: (x.min(0.)),
        y: (y.min(0.)),
      } * transform.zoom,
    );
  }

  let PixelCoordinate { x, y } = inv.apply(PixelPosition {
    x: rect.max.x,
    y: rect.max.y,
  });
  if x > 2000. || y > 2000. {
    transform.translate(
      PixelPosition {
        x: (x - 2000.).max(0.),
        y: (y - 2000.).max(0.),
      } * transform.zoom,
    );
  }
}

/// Shows a given bounding box on the map.
pub(crate) fn show_box(transform: &mut Transform, bb: &BoundingBox, rect: Rect) {
  if bb.is_valid() {
    let width_zoom: f32 = 1. / (bb.width() * transform.zoom / rect.width());
    let height_zoom: f32 = 1. / (bb.height() * transform.zoom / rect.height());
    transform.zoom(width_zoom.min(height_zoom));
    transform.zoom(0.95);
    set_coordinate_to_pixel(bb.center(), rect.center().into(), transform);
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
