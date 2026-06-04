use bevy::{
  input::mouse::AccumulatedMouseScroll,
  prelude::*,
  window::{PrimaryWindow, Window},
};
use bevy_egui::{egui, input::EguiWantsInput};
use mapvas::map::{
  coordinates::{CANVAS_SIZE, PixelCoordinate, PixelPosition, Transform},
  mapvas_egui::MapViewport,
};

const MAX_ZOOM: f32 = 524_288.0;
const MIN_ZOOM: f32 = 1.0;
const KEY_PAN_SPEED: f32 = 500.0;

pub struct NativeMapPlugin;

impl Plugin for NativeMapPlugin {
  fn build(&self, app: &mut App) {
    app
      .init_resource::<NativeMapControl>()
      .add_systems(Update, native_map_input);
  }
}

#[derive(Resource)]
pub struct NativeMapControl {
  enabled: bool,
  viewport: Option<MapViewport>,
  transform: Option<Transform>,
  dragging: bool,
  last_cursor_pos: Option<egui::Pos2>,
}

impl Default for NativeMapControl {
  fn default() -> Self {
    Self {
      enabled: true,
      viewport: None,
      transform: None,
      dragging: false,
      last_cursor_pos: None,
    }
  }
}

impl NativeMapControl {
  pub fn ui(&mut self, ui: &mut egui::Ui) {
    ui.collapsing("Native Map Viewport", |ui| {
      ui.checkbox(&mut self.enabled, "Bevy input");

      ui.separator();
      if let Some(viewport) = self.viewport {
        stat_row(ui, "Zoom:", format!("{:.2}", viewport.transform.zoom));
        stat_row(
          ui,
          "Map rect:",
          format!("{:.0}x{:.0}", viewport.rect.width(), viewport.rect.height()),
        );
      } else {
        ui.label("No viewport yet");
      }
    });
  }

  #[must_use]
  pub fn enabled(&self) -> bool {
    self.enabled
  }

  #[must_use]
  pub fn transform(&self) -> Option<Transform> {
    self.transform
  }

  pub fn set_viewport(&mut self, viewport: Option<MapViewport>) {
    self.viewport = viewport;
    if let Some(viewport) = viewport {
      self.transform = Some(viewport.transform);
    }
  }
}

fn native_map_input(
  mut control: ResMut<NativeMapControl>,
  windows: Query<&Window, With<PrimaryWindow>>,
  buttons: Res<ButtonInput<MouseButton>>,
  keys: Res<ButtonInput<KeyCode>>,
  scroll: Res<AccumulatedMouseScroll>,
  egui_wants_input: Res<EguiWantsInput>,
  time: Res<Time>,
) {
  if !control.enabled {
    control.dragging = false;
    control.last_cursor_pos = None;
    return;
  }

  let Some(viewport) = control.viewport else {
    return;
  };
  let Some(mut transform) = control.transform else {
    return;
  };
  let Ok(window) = windows.single() else {
    return;
  };

  let cursor = cursor_pos(window);
  let cursor_in_viewport = cursor.is_some_and(|pos| viewport.rect.contains(pos));
  let mut changed = false;

  if buttons.just_pressed(MouseButton::Left) && cursor_in_viewport {
    control.dragging = true;
    control.last_cursor_pos = cursor;
  }
  if !buttons.pressed(MouseButton::Left) {
    control.dragging = false;
    control.last_cursor_pos = cursor;
  }
  if control.dragging
    && buttons.pressed(MouseButton::Left)
    && let (Some(last), Some(current)) = (control.last_cursor_pos, cursor)
  {
    let delta = current - last;
    if delta != egui::Vec2::ZERO {
      transform.translate(PixelPosition {
        x: delta.x,
        y: delta.y,
      });
      control.last_cursor_pos = Some(current);
      changed = true;
    }
  }

  if cursor_in_viewport
    && scroll.delta.y != 0.0
    && let Some(cursor) = cursor
  {
    let zoom_delta = (scroll.delta.y + 1.0).clamp(0.8, 1.4).sqrt();
    if zoom_with_center(&mut transform, zoom_delta, cursor.into()) {
      changed = true;
    }
  }

  if !egui_wants_input.wants_any_keyboard_input() {
    let mut pan = PixelPosition { x: 0.0, y: 0.0 };
    let pan_delta = KEY_PAN_SPEED * time.delta_secs();
    if keys.pressed(KeyCode::ArrowDown) {
      pan.y -= pan_delta;
    }
    if keys.pressed(KeyCode::ArrowLeft) {
      pan.x += pan_delta;
    }
    if keys.pressed(KeyCode::ArrowRight) {
      pan.x -= pan_delta;
    }
    if keys.pressed(KeyCode::ArrowUp) {
      pan.y += pan_delta;
    }
    if pan.x != 0.0 || pan.y != 0.0 {
      transform.translate(pan);
      changed = true;
    }
    if keys.just_pressed(KeyCode::Minus) && zoom_with_center(&mut transform, 0.9, center(viewport))
    {
      changed = true;
    }
    if (keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadEqual))
      && zoom_with_center(&mut transform, 1.0 / 0.9, center(viewport))
    {
      changed = true;
    }
  }

  if changed {
    fit_to_screen(&mut transform, &viewport.rect);
    control.transform = Some(transform);
  }
}

fn cursor_pos(window: &Window) -> Option<egui::Pos2> {
  window.cursor_position().map(|pos| egui::pos2(pos.x, pos.y))
}

fn center(viewport: MapViewport) -> PixelPosition {
  viewport.rect.center().into()
}

fn zoom_with_center(transform: &mut Transform, delta: f32, center: PixelPosition) -> bool {
  if transform.zoom * delta < MIN_ZOOM || transform.zoom * delta > MAX_ZOOM {
    return false;
  }
  let hover_coord: PixelCoordinate = transform.invert().apply(center);
  transform.zoom(delta);
  set_coordinate_to_pixel(hover_coord, center, transform);
  true
}

fn set_coordinate_to_pixel(
  coord: PixelCoordinate,
  cursor: PixelPosition,
  transform: &mut Transform,
) {
  let current_pos_in_gui = transform.apply(coord);
  transform.translate(current_pos_in_gui * (-1.0) + cursor);
}

fn fit_to_screen(transform: &mut Transform, rect: &egui::Rect) {
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

fn stat_row(ui: &mut egui::Ui, label: &str, value: String) {
  ui.horizontal(|ui| {
    ui.label(label);
    ui.label(value);
  });
}
