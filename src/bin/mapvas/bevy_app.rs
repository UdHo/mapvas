use bevy::{log::LogPlugin, prelude::*, window::WindowResolution};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use mapvas::{
  config::Config,
  map::{Map, tile_renderer::init_style_config},
  mapvas_ui::{KnownBevyGeometrySnapshot, MapApp, MapAppOutput},
  remote::{RepaintSignal, spawn_remote_runner_on_handle},
};
use tokio_metrics::RuntimeMonitor;

use crate::{
  bevy_geometry::{BevyGeometryLayer, BevyGeometryPlugin},
  bevy_map::{BevyMapAction, BevyMapControl, BevyMapPlugin, BevyMapShortcut, BevyMapViewport},
  bevy_repaint::{BevyRepaintPlugin, BevyRepaintRequests},
  bevy_screenshot::{BevyScreenshotPlugin, BevyScreenshotRequests},
  bevy_tiles::{BevyTileLayer, BevyTilePlugin, BevyTileRuntime},
};

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

fn build_runtime() -> tokio::runtime::Runtime {
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
  bevy_viewport: &mut BevyMapViewport,
  bevy_geometry: &mut BevyGeometryLayer,
  bevy_screenshots: &mut BevyScreenshotRequests,
  bevy_tiles: &mut BevyTileLayer,
  app_exit: &mut MessageWriter<AppExit>,
) {
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
          handle_bevy_map_action(app, action);
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
      &mut bevy_viewport,
      &mut bevy_geometry,
      &mut bevy_screenshots,
      &mut bevy_tiles,
      &mut app_exit,
    );
  } else {
    bevy_viewport.set(None);
  }
  bevy_tiles.refresh_style_version();

  Ok(())
}

fn setup_camera(mut commands: Commands) {
  commands.spawn((Camera2d, PrimaryEguiContext));
}

pub(crate) fn run() {
  let runtime = build_runtime();
  let runtime_handle = runtime.handle().clone();
  let runtime_monitor = RuntimeMonitor::new(&runtime_handle);
  let config = Config::new();
  let (repaint_signal, repaint_requests) = BevyRepaintRequests::channel();

  App::new()
    .insert_resource(repaint_requests)
    .insert_non_send_resource(MapvasBevyState::new(
      runtime,
      runtime_monitor,
      config.clone(),
      repaint_signal,
    ))
    .init_resource::<BevyGeometryLayer>()
    .insert_resource(BevyTileRuntime(runtime_handle))
    .insert_resource(BevyTileLayer::new(config))
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
    .add_plugins(BevyMapPlugin)
    .add_plugins(BevyRepaintPlugin)
    .add_plugins(BevyScreenshotPlugin)
    .add_plugins(BevyTilePlugin)
    .add_plugins(BevyGeometryPlugin)
    .add_systems(Startup, setup_camera)
    .add_systems(EguiPrimaryContextPass, mapvas_ui_system)
    .run();
}
