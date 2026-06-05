use std::path::PathBuf;

use bevy::{
  input::mouse::AccumulatedMouseScroll,
  prelude::*,
  window::{FileDragAndDrop, PrimaryWindow, Window},
};
use bevy_egui::{egui, input::EguiWantsInput};
use mapvas::map::{
  coordinates::{PixelPosition, Transform},
  viewport::{MapViewport, fit_to_screen, zoom_with_center},
};

const KEY_PAN_SPEED: f32 = 500.0;
const DOUBLE_CLICK_MAX_INTERVAL_SECS: f64 = 0.35;
const DOUBLE_CLICK_MAX_DISTANCE: f32 = 8.0;

pub struct BevyMapPlugin;

impl Plugin for BevyMapPlugin {
  fn build(&self, app: &mut App) {
    app
      .init_resource::<BevyMapViewport>()
      .init_resource::<BevyMapControl>()
      .add_systems(Update, bevy_map_input);
  }
}

#[derive(Resource, Default)]
pub struct BevyMapViewport {
  viewport: Option<MapViewport>,
}

impl BevyMapViewport {
  pub fn set(&mut self, viewport: Option<MapViewport>) {
    self.viewport = viewport;
  }

  #[must_use]
  pub fn get(&self) -> Option<MapViewport> {
    self.viewport
  }
}

#[derive(Resource)]
pub struct BevyMapControl {
  dragging: bool,
  last_cursor_pos: Option<PixelPosition>,
  hover_pos: Option<PixelPosition>,
  last_left_click: Option<(PixelPosition, f64)>,
  secondary_drag_start: Option<PixelPosition>,
  secondary_drag_moved: bool,
  actions: Vec<BevyMapAction>,
}

pub struct BevyCommandDrag {
  pub pos: PixelPosition,
  pub delta: PixelPosition,
  pub transform: Transform,
}

pub enum BevyMapAction {
  CommandDrag(BevyCommandDrag),
  ContextMenu(PixelPosition),
  DoubleClick(PixelPosition),
  DroppedFile(PathBuf),
  Shortcut(BevyMapShortcut),
  TransformChanged(Transform),
}

pub enum BevyMapShortcut {
  Clear,
  FocusAll,
  Paste,
  Copy,
  Screenshot,
}

impl Default for BevyMapControl {
  fn default() -> Self {
    Self {
      dragging: false,
      last_cursor_pos: None,
      hover_pos: None,
      last_left_click: None,
      secondary_drag_start: None,
      secondary_drag_moved: false,
      actions: Vec::new(),
    }
  }
}

impl BevyMapControl {
  pub fn ui(&self, ui: &mut egui::Ui, viewport: Option<MapViewport>) {
    ui.collapsing("Bevy Map Viewport", |ui| {
      if let Some(viewport) = viewport {
        stat_row(ui, "Zoom:", format!("{:.2}", viewport.transform.zoom));
        stat_row(
          ui,
          "Map rect:",
          format!("{:.0}x{:.0}", viewport.width(), viewport.height()),
        );
      } else {
        ui.label("No viewport yet");
      }
    });
  }

  #[must_use]
  pub fn hover_pos(&self) -> Option<PixelPosition> {
    self.hover_pos
  }

  pub fn take_actions(&mut self) -> Vec<BevyMapAction> {
    std::mem::take(&mut self.actions)
  }
}

fn bevy_map_input(
  mut control: ResMut<BevyMapControl>,
  viewport: Res<BevyMapViewport>,
  windows: Query<&Window, With<PrimaryWindow>>,
  buttons: Res<ButtonInput<MouseButton>>,
  keys: Res<ButtonInput<KeyCode>>,
  scroll: Res<AccumulatedMouseScroll>,
  egui_wants_input: Res<EguiWantsInput>,
  time: Res<Time>,
  mut file_drops: MessageReader<FileDragAndDrop>,
) {
  for event in file_drops.read() {
    if let FileDragAndDrop::DroppedFile { path_buf, .. } = event {
      control
        .actions
        .push(BevyMapAction::DroppedFile(path_buf.clone()));
    }
  }

  let Some(viewport) = viewport.get() else {
    control.hover_pos = None;
    return;
  };
  let mut transform = viewport.transform;
  let Ok(window) = windows.single() else {
    return;
  };

  let cursor = cursor_pos(window);
  let cursor_in_viewport = cursor.is_some_and(|pos| viewport.contains(pos));
  let egui_popup_open = egui_wants_input.is_popup_open();
  control.hover_pos = if !egui_popup_open && cursor_in_viewport {
    cursor
  } else {
    None
  };
  let mut changed = false;

  if egui_popup_open {
    control.dragging = false;
    control.last_cursor_pos = cursor;
    control.secondary_drag_start = None;
    control.secondary_drag_moved = false;
  }

  if !egui_popup_open && buttons.just_pressed(MouseButton::Left) && cursor_in_viewport {
    if let Some(current) = cursor {
      let now = time.elapsed_secs_f64();
      if let Some((previous, previous_time)) = control.last_left_click {
        let interval = now - previous_time;
        let distance = pixel_distance(current, previous);
        if interval <= DOUBLE_CLICK_MAX_INTERVAL_SECS && distance <= DOUBLE_CLICK_MAX_DISTANCE {
          control.actions.push(BevyMapAction::DoubleClick(current));
          control.last_left_click = None;
        } else {
          control.last_left_click = Some((current, now));
        }
      } else {
        control.last_left_click = Some((current, now));
      }
    }
    control.dragging = true;
    control.last_cursor_pos = cursor;
  }
  if !buttons.pressed(MouseButton::Left) {
    control.dragging = false;
    control.last_cursor_pos = cursor;
  }
  if control.dragging
    && !egui_popup_open
    && buttons.pressed(MouseButton::Left)
    && let (Some(last), Some(current)) = (control.last_cursor_pos, cursor)
  {
    let delta = pixel_delta(last, current);
    if delta != PixelPosition::default() {
      transform.translate(delta);
      control.last_cursor_pos = Some(current);
      changed = true;
    }
  }

  if !egui_popup_open && buttons.just_pressed(MouseButton::Right) && cursor_in_viewport {
    control.secondary_drag_start = cursor;
    control.secondary_drag_moved = false;
  }
  if !egui_popup_open
    && buttons.pressed(MouseButton::Right)
    && let (Some(start), Some(current)) = (control.secondary_drag_start, cursor)
  {
    let delta = pixel_delta(start, current);
    if delta != PixelPosition::default() {
      control.secondary_drag_moved = true;
      control
        .actions
        .push(BevyMapAction::CommandDrag(BevyCommandDrag {
          pos: current,
          delta,
          transform,
        }));
    }
  }
  if buttons.just_released(MouseButton::Right) {
    if let Some(start) = control.secondary_drag_start.take()
      && !control.secondary_drag_moved
      && !egui_popup_open
      && cursor_in_viewport
    {
      control
        .actions
        .push(BevyMapAction::ContextMenu(cursor.unwrap_or(start)));
    }
    control.secondary_drag_moved = false;
  } else if !buttons.pressed(MouseButton::Right) {
    control.secondary_drag_start = None;
    control.secondary_drag_moved = false;
  }

  if !egui_popup_open
    && cursor_in_viewport
    && scroll.delta.y != 0.0
    && let Some(cursor) = cursor
  {
    let zoom_delta = (scroll.delta.y + 1.0).clamp(0.8, 1.4).sqrt();
    if zoom_with_center(&mut transform, zoom_delta, cursor) {
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
    if keys.just_pressed(KeyCode::Delete) {
      control
        .actions
        .push(BevyMapAction::Shortcut(BevyMapShortcut::Clear));
    }
    if keys.just_pressed(KeyCode::KeyF) {
      control
        .actions
        .push(BevyMapAction::Shortcut(BevyMapShortcut::FocusAll));
    }
    if keys.just_pressed(KeyCode::KeyV) {
      control
        .actions
        .push(BevyMapAction::Shortcut(BevyMapShortcut::Paste));
    }
    if keys.just_pressed(KeyCode::KeyC) {
      control
        .actions
        .push(BevyMapAction::Shortcut(BevyMapShortcut::Copy));
    }
    if keys.just_pressed(KeyCode::KeyS) {
      control
        .actions
        .push(BevyMapAction::Shortcut(BevyMapShortcut::Screenshot));
    }
  }

  if changed {
    fit_to_screen(&mut transform, viewport.rect);
    control
      .actions
      .push(BevyMapAction::TransformChanged(transform));
  }
}

fn cursor_pos(window: &Window) -> Option<PixelPosition> {
  window
    .cursor_position()
    .map(|pos| PixelPosition { x: pos.x, y: pos.y })
}

fn pixel_delta(from: PixelPosition, to: PixelPosition) -> PixelPosition {
  PixelPosition {
    x: to.x - from.x,
    y: to.y - from.y,
  }
}

fn pixel_distance(a: PixelPosition, b: PixelPosition) -> f32 {
  let delta = pixel_delta(a, b);
  (delta.x * delta.x + delta.y * delta.y).sqrt()
}

fn center(viewport: MapViewport) -> PixelPosition {
  viewport.center()
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: String) {
  ui.horizontal(|ui| {
    ui.label(label);
    ui.label(value);
  });
}
