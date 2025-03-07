use egui::Rect;
use tracing::instrument;

use crate::map::coordinates::{BoundingBox, PixelPosition, Transform};

pub const MAX_ZOOM: f32 = 524_288.;
pub const MIN_ZOOM: f32 = 1.;

#[instrument]
pub(crate) fn set_coordinate_to_pixel(
  coord: PixelPosition,
  cursor: PixelPosition,
  transform: &mut Transform,
) {
  let current_pos_in_gui = transform.apply(coord);
  transform.translate(current_pos_in_gui * (-1.) + cursor);
}

pub(crate) fn fit_to_screen(transform: &mut Transform, rect: &Rect) {
  transform.zoom = transform.zoom.clamp(MIN_ZOOM, MAX_ZOOM);

  let inv = transform.invert();
  let PixelPosition { x, y } = inv.apply(PixelPosition { x: 0., y: 0. });
  if x < 0. || y < 0. {
    transform.translate(
      PixelPosition {
        x: (x.min(0.)),
        y: (y.min(0.)),
      } * transform.zoom,
    );
  }

  let PixelPosition { x, y } = inv.apply(PixelPosition {
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

pub(crate) fn show_box(transform: &mut Transform, bb: &BoundingBox, rect: Rect) {
  if bb.is_valid() {
    let width_zoom: f32 = 1. / (bb.width() * transform.zoom / rect.width());
    let height_zoom: f32 = 1. / (bb.height() * transform.zoom / rect.height());
    transform.zoom(width_zoom.min(height_zoom));
    transform.zoom(0.95);
    set_coordinate_to_pixel(bb.center(), rect.center().into(), transform);
  }
}
