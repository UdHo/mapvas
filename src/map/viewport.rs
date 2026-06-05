use super::{
  coordinates::{CANVAS_SIZE, PixelCoordinate, PixelPosition, PixelRect, Transform},
  geometry_collection::Geometry,
};

pub const MAX_ZOOM: f32 = 524_288.0;
pub const MIN_ZOOM: f32 = 1.0;

pub fn set_coordinate_to_pixel(
  coord: PixelCoordinate,
  cursor: PixelPosition,
  transform: &mut Transform,
) {
  let current_pos_in_gui = transform.apply(coord);
  transform.translate(current_pos_in_gui * (-1.0) + cursor);
}

pub fn zoom_with_center(transform: &mut Transform, delta: f32, center: PixelPosition) -> bool {
  if transform.zoom * delta < MIN_ZOOM || transform.zoom * delta > MAX_ZOOM {
    return false;
  }
  let hover_coord: PixelCoordinate = transform.invert().apply(center);
  transform.zoom(delta);
  set_coordinate_to_pixel(hover_coord, center, transform);
  true
}

pub fn fit_to_screen(transform: &mut Transform, rect: PixelRect) {
  transform.zoom = transform.zoom.clamp(MIN_ZOOM, MAX_ZOOM);

  let inv = transform.invert();
  let world_h_screen = CANVAS_SIZE * transform.zoom;
  let view_h = rect.height();
  let top_y = inv.apply(PixelPosition { x: 0.0, y: 0.0 }).y;

  if view_h >= world_h_screen {
    let desired_top_y = -(view_h - world_h_screen) / (2.0 * transform.zoom);
    let shift = (top_y - desired_top_y) * transform.zoom;
    if shift.abs() > 0.01 {
      transform.translate(PixelPosition { x: 0.0, y: shift });
    }
  } else if top_y < 0.0 {
    transform.translate(PixelPosition {
      x: 0.0,
      y: top_y * transform.zoom,
    });
  } else {
    let bottom_y = inv
      .apply(PixelPosition {
        x: rect.max.x,
        y: rect.max.y,
      })
      .y;
    if bottom_y > CANVAS_SIZE {
      transform.translate(PixelPosition {
        x: 0.0,
        y: (bottom_y - CANVAS_SIZE) * transform.zoom,
      });
    }
  }

  let left_x = inv
    .apply(PixelPosition {
      x: rect.min.x,
      y: 0.0,
    })
    .x;
  let wrapped = left_x.rem_euclid(CANVAS_SIZE);
  let shift = wrapped - left_x;
  if shift.abs() > 0.01 {
    transform.translate(PixelPosition {
      x: -shift * transform.zoom,
      y: 0.0,
    });
  }
}

#[derive(Debug, Clone, Copy)]
pub struct MapViewport {
  pub transform: Transform,
  pub rect: PixelRect,
}

impl MapViewport {
  #[must_use]
  pub fn width(&self) -> f32 {
    self.rect.width()
  }

  #[must_use]
  pub fn height(&self) -> f32 {
    self.rect.height()
  }

  #[must_use]
  pub fn center(&self) -> PixelPosition {
    self.rect.center()
  }

  #[must_use]
  pub fn min(&self) -> PixelPosition {
    self.rect.min
  }

  #[must_use]
  pub fn max(&self) -> PixelPosition {
    self.rect.max
  }

  #[must_use]
  pub fn contains(&self, pos: PixelPosition) -> bool {
    self.rect.contains(pos)
  }
}

#[derive(Clone, Default)]
pub struct GeometrySnapshot {
  pub version: u64,
  pub geometry_version: u64,
  pub geometries: Vec<Geometry<PixelCoordinate>>,
  pub highlighted_geometries: Vec<Geometry<PixelCoordinate>>,
}
