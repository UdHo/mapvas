use crate::{
  config::Config,
  map::{Map, tile_renderer::init_style_config},
  mapvas_ui::{KnownBevyGeometrySnapshot, MapApp, MapAppOutput},
  remote::{RepaintSignal, spawn_remote_runner_on_handle},
};
use bevy::{
  log::LogPlugin,
  prelude::*,
  window::{PrimaryWindow, WindowResolution},
  winit::{WINIT_WINDOWS, WinitSettings},
};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use tokio_metrics::RuntimeMonitor;
use winit::window::Icon;

use super::{
  geometry::{BevyGeometryLayer, BevyGeometryPlugin},
  map::{BevyMapAction, BevyMapControl, BevyMapPlugin, BevyMapShortcut, BevyMapViewport},
  repaint::{BevyRepaintPlugin, BevyRepaintRequests, BevyWakeup},
  screenshot::{BevyScreenshotPlugin, BevyScreenshotRequests},
  surface::BevyRenderSurfacePlugin,
  tiles::{BevyTileLayer, BevyTilePlugin, BevyTileRuntime},
};

const MAPVAS_WINDOW_ICON: &[u8] = include_bytes!("../../logo.png");
#[cfg(target_os = "macos")]
const MAPVAS_MACOS_APP_ICON: &[u8] = include_bytes!("../../mapvas_logo.png");

#[derive(Clone, Copy)]
pub struct MapvasBevyRenderPlugin {
  interactive: bool,
}

impl MapvasBevyRenderPlugin {
  #[must_use]
  pub fn interactive() -> Self {
    Self { interactive: true }
  }

  #[must_use]
  pub fn headless() -> Self {
    Self { interactive: false }
  }
}

impl Plugin for MapvasBevyRenderPlugin {
  fn build(&self, app: &mut App) {
    app.init_resource::<BevyGeometryLayer>();

    if self.interactive {
      app
        .add_plugins(BevyMapPlugin)
        .add_plugins(BevyRenderSurfacePlugin)
        .add_plugins(BevyRepaintPlugin)
        .add_plugins(BevyScreenshotPlugin);
    } else {
      app.init_resource::<BevyMapViewport>();
    }

    app
      .add_plugins(BevyTilePlugin)
      .add_plugins(BevyGeometryPlugin);
  }
}

pub(crate) struct MapvasBevyState {
  runtime: tokio::runtime::Runtime,
  runtime_monitor: Option<RuntimeMonitor>,
  initial_config: Config,
  repaint_signal: RepaintSignal,
  app: Option<MapApp>,
}

impl MapvasBevyState {
  pub(crate) fn new(
    runtime: tokio::runtime::Runtime,
    runtime_monitor: RuntimeMonitor,
    initial_config: Config,
    repaint_signal: RepaintSignal,
  ) -> Self {
    Self {
      runtime,
      runtime_monitor: Some(runtime_monitor),
      initial_config,
      repaint_signal,
      app: None,
    }
  }

  fn initialize(&mut self, egui_ctx: &egui::Context) {
    if self.app.is_some() {
      return;
    }

    egui_extras::install_image_loaders(egui_ctx);

    let config = self.initial_config.clone();
    init_style_config(config.vector_style_file.as_deref());

    let (map, remote, data_holder) = Map::new_bevy(config.clone(), self.repaint_signal.clone());
    spawn_remote_runner_on_handle(self.runtime.handle().clone(), remote.clone());

    let app = MapApp::new_bevy(
      map,
      remote,
      data_holder,
      config,
      self.runtime_monitor.take(),
    );
    self.app = Some(app);
  }
}

pub(crate) fn build_runtime() -> tokio::runtime::Runtime {
  match tokio::runtime::Builder::new_multi_thread()
    .worker_threads(4)
    .thread_name("async-io")
    .enable_all()
    .build()
  {
    Ok(rt) => rt,
    Err(e) => {
      eprintln!("Error: Failed to create runtime: {e}");
      std::process::exit(1);
    }
  }
}

fn handle_bevy_map_action(app: &mut MapApp, action: BevyMapAction) {
  match action {
    BevyMapAction::CommandDrag(drag) => {
      app.handle_map_command_drag(drag.pos, drag.delta, drag.transform);
    }
    BevyMapAction::Click(_) => {}
    BevyMapAction::ContextMenu(pos) => {
      app.open_map_context_menu(pos);
    }
    BevyMapAction::DoubleClick(pos) => {
      app.handle_map_double_click(pos);
    }
    BevyMapAction::DroppedFile(path) => {
      app.handle_map_dropped_file(path);
    }
    BevyMapAction::Shortcut(shortcut) => handle_bevy_map_shortcut(app, shortcut),
    BevyMapAction::TransformChanged(transform) => {
      app.set_map_transform(transform);
    }
  }
}

fn handle_bevy_map_shortcut(app: &mut MapApp, shortcut: BevyMapShortcut) {
  match shortcut {
    BevyMapShortcut::Clear => app.clear_map(),
    BevyMapShortcut::FocusAll => app.focus_map(),
    BevyMapShortcut::Paste => app.paste_map(),
    BevyMapShortcut::Copy => app.copy_map(),
    BevyMapShortcut::Screenshot => app.request_map_screenshot(),
  }
}

fn apply_map_app_output(
  output: MapAppOutput,
  bevy_map: &mut BevyMapControl,
  bevy_viewport: &mut BevyMapViewport,
  bevy_geometry: &mut BevyGeometryLayer,
  bevy_screenshots: &mut BevyScreenshotRequests,
  bevy_tiles: &mut BevyTileLayer,
  app_exit: &mut MessageWriter<AppExit>,
) {
  bevy_map.set_pointer_blocking_rects(output.bevy_pointer_blocking_rects);
  for path in output.screenshot_requests {
    bevy_screenshots.request_path(path);
  }
  bevy_viewport.set(output.viewport);
  if let Some(snapshot) = output.bevy_geometry_snapshot {
    bevy_geometry.update_snapshot(snapshot);
  }
  if let Some(config) = output.config_update {
    bevy_tiles.update_config(&config);
    bevy_geometry.update_config(&config);
  }
  if output.quit_requested {
    app_exit.write(AppExit::Success);
  }
}

fn mapvas_ui_system(
  mut contexts: EguiContexts,
  mut state: NonSendMut<MapvasBevyState>,
  mut bevy_map: ResMut<BevyMapControl>,
  mut bevy_viewport: ResMut<BevyMapViewport>,
  mut bevy_geometry: ResMut<BevyGeometryLayer>,
  mut bevy_screenshots: ResMut<BevyScreenshotRequests>,
  mut bevy_tiles: ResMut<BevyTileLayer>,
  mut app_exit: MessageWriter<AppExit>,
) -> Result {
  let egui_ctx = contexts.ctx_mut()?.clone();
  let runtime_handle = state.runtime.handle().clone();
  let _runtime_guard = runtime_handle.enter();

  state.initialize(&egui_ctx);

  let mut output = None;
  egui::CentralPanel::default()
    .frame(egui::Frame::NONE)
    .show(&egui_ctx, |ui| {
      if let Some(app) = state.app.as_mut() {
        for action in bevy_map.take_actions() {
          match action {
            BevyMapAction::Click(pos) => {
              bevy_tiles.select_debug_feature(pos, bevy_viewport.get());
            }
            action => handle_bevy_map_action(app, action),
          }
        }
        app.handle_bevy_map_hover(bevy_map.hover_pos());
        {
          let known_bevy_geometry_snapshot = KnownBevyGeometrySnapshot {
            snapshot_version: bevy_geometry.snapshot_version(),
            geometry_version: bevy_geometry.geometry_version(),
          };
          let mut map_layer_controls = |ui: &mut egui::Ui| {
            bevy_map.ui(ui, bevy_viewport.get());
            bevy_tiles.ui(ui);
            bevy_geometry.ui(ui);
          };
          output = Some(app.show_bevy(ui, &mut map_layer_controls, known_bevy_geometry_snapshot));
        }
      }
    });
  if let Some(output) = output {
    apply_map_app_output(
      output,
      &mut bevy_map,
      &mut bevy_viewport,
      &mut bevy_geometry,
      &mut bevy_screenshots,
      &mut bevy_tiles,
      &mut app_exit,
    );
  } else {
    bevy_map.set_pointer_blocking_rects(Vec::new());
    bevy_viewport.set(None);
  }
  bevy_tiles.refresh_style_version();

  Ok(())
}

fn setup_camera(mut commands: Commands) {
  commands.spawn((Camera2d, PrimaryEguiContext));
}

fn setup_window_icon(primary_window: Query<Entity, With<PrimaryWindow>>) {
  #[cfg(target_os = "macos")]
  setup_macos_application_icon();

  let Ok(window_entity) = primary_window.single() else {
    return;
  };
  let icon = match load_mapvas_window_icon() {
    Ok(icon) => icon,
    Err(error) => {
      log::warn!("Failed to load mapvas window icon: {error}");
      return;
    }
  };
  WINIT_WINDOWS.with_borrow(|windows| {
    if let Some(window) = windows.get_window(window_entity) {
      window.set_window_icon(Some(icon));
    }
  });
}

fn load_mapvas_window_icon() -> Result<Icon, String> {
  let image = image::load_from_memory(MAPVAS_WINDOW_ICON)
    .map_err(|error| format!("decode logo.png: {error}"))?
    .into_rgba8();
  let (width, height) = image.dimensions();
  Icon::from_rgba(image.into_raw(), width, height)
    .map_err(|error| format!("create winit icon: {error}"))
}

#[cfg(target_os = "macos")]
fn setup_macos_application_icon() {
  if let Err(error) = set_macos_application_icon(MAPVAS_MACOS_APP_ICON) {
    log::warn!("Failed to set macOS application icon: {error}");
  }
}

#[cfg(target_os = "macos")]
fn set_macos_application_icon(icon_png: &[u8]) -> Result<(), String> {
  use objc::{class, msg_send, runtime::Object, sel, sel_impl};

  unsafe {
    let data: *mut Object =
      msg_send![class!(NSData), dataWithBytes: icon_png.as_ptr() length: icon_png.len()];
    if data.is_null() {
      return Err("create NSData from icon bytes".to_string());
    }

    let image: *mut Object = msg_send![class!(NSImage), alloc];
    if image.is_null() {
      return Err("allocate NSImage".to_string());
    }

    let image: *mut Object = msg_send![image, initWithData: data];
    if image.is_null() {
      return Err("initialize NSImage from icon bytes".to_string());
    }

    let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
    if app.is_null() {
      let _: () = msg_send![image, release];
      return Err("get shared NSApplication".to_string());
    }

    let _: () = msg_send![app, setApplicationIconImage: image];
    let _: () = msg_send![image, release];
  }

  Ok(())
}

pub fn run() {
  let runtime = build_runtime();
  let runtime_handle = runtime.handle().clone();
  let runtime_monitor = RuntimeMonitor::new(&runtime_handle);
  let config = Config::new();
  let wakeup = BevyWakeup::default();
  let (repaint_signal, repaint_requests) = BevyRepaintRequests::channel(wakeup.clone());

  App::new()
    .insert_resource(WinitSettings::desktop_app())
    .insert_resource(wakeup.clone())
    .insert_resource(repaint_requests)
    .insert_non_send_resource(MapvasBevyState::new(
      runtime,
      runtime_monitor,
      config.clone(),
      repaint_signal,
    ))
    .init_resource::<BevyGeometryLayer>()
    .insert_resource(BevyTileRuntime(runtime_handle))
    .insert_resource(BevyTileLayer::new(config, wakeup))
    .add_plugins(
      DefaultPlugins
        .set(WindowPlugin {
          primary_window: Some(Window {
            title: "mapvas".to_string(),
            resolution: WindowResolution::new(1600, 1200),
            ..Default::default()
          }),
          ..Default::default()
        })
        .disable::<LogPlugin>(),
    )
    .add_plugins(EguiPlugin::default())
    .add_plugins(MapvasBevyRenderPlugin::interactive())
    .add_systems(Startup, (setup_camera, setup_window_icon))
    .add_systems(EguiPrimaryContextPass, mapvas_ui_system)
    .run();
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn mapvas_window_icon_loads_from_embedded_logo() {
    assert!(load_mapvas_window_icon().is_ok());
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn mapvas_macos_application_icon_source_decodes() {
    assert!(image::load_from_memory(MAPVAS_MACOS_APP_ICON).is_ok());
  }
}
