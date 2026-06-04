mod bevy_geometry;
mod bevy_tiles;

use bevy::{log::LogPlugin, prelude::*, window::WindowResolution};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use bevy_geometry::{NativeGeometryLayer, NativeGeometryPlugin};
use bevy_tiles::{NativeMapViewport, NativeTileLayer, NativeTilePlugin, NativeTileRuntime};
use mapvas::{
  config::Config,
  map::{mapvas_egui::Map, tile_renderer::init_style_config},
  mapvas_ui::MapApp,
  profiling,
  remote::spawn_remote_runner_on_handle,
};
use tokio_metrics::RuntimeMonitor;

struct MapvasBevyState {
  runtime: tokio::runtime::Runtime,
  runtime_monitor: Option<RuntimeMonitor>,
  app: Option<MapApp>,
}

impl MapvasBevyState {
  fn new(runtime: tokio::runtime::Runtime, runtime_monitor: RuntimeMonitor) -> Self {
    Self {
      runtime,
      runtime_monitor: Some(runtime_monitor),
      app: None,
    }
  }

  fn initialize(&mut self, egui_ctx: egui::Context) {
    if self.app.is_some() {
      return;
    }

    egui_extras::install_image_loaders(&egui_ctx);

    let config = Config::new();
    init_style_config(config.vector_style_file.as_deref());

    let (map, remote, data_holder) = Map::new_without_tiles(egui_ctx, config.clone());
    spawn_remote_runner_on_handle(self.runtime.handle().clone(), remote.clone());

    let mut app = MapApp::new(
      map,
      remote,
      data_holder,
      config,
      self.runtime_monitor.take(),
    );
    app.set_transparent_map_background();
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

fn mapvas_ui_system(
  mut contexts: EguiContexts,
  mut state: NonSendMut<MapvasBevyState>,
  mut native_viewport: ResMut<NativeMapViewport>,
  mut native_geometry: ResMut<NativeGeometryLayer>,
  mut native_tiles: ResMut<NativeTileLayer>,
) -> Result {
  let egui_ctx = contexts.ctx_mut()?.clone();
  let runtime_handle = state.runtime.handle().clone();
  let _runtime_guard = runtime_handle.enter();

  state.initialize(egui_ctx.clone());

  let mut viewport = None;
  let mut current_config = None;
  let mut geometry_snapshot = None;
  egui::CentralPanel::default()
    .frame(egui::Frame::NONE)
    .show(&egui_ctx, |ui| {
      if let Some(app) = state.app.as_mut() {
        {
          let mut map_layer_controls = |ui: &mut egui::Ui| {
            native_tiles.ui(ui);
            native_geometry.ui(ui);
          };
          app.show_with_map_layer_controls(ui, &mut map_layer_controls);
        }
        viewport = app.viewport();
        native_tiles.draw_overlay(ui, viewport);
        current_config = Some(app.current_config());
        let geometry_snapshot_version = app.geometry_snapshot_version();
        if native_geometry.needs_snapshot(geometry_snapshot_version) {
          geometry_snapshot = Some(app.geometry_snapshot());
        }
      }
    });
  native_viewport.set(viewport);
  if let Some(snapshot) = geometry_snapshot {
    native_geometry.update_snapshot(snapshot);
  }
  if let Some(config) = current_config {
    native_tiles.update_config(&config);
  }
  native_tiles.refresh_style_version();

  Ok(())
}

fn setup_camera(mut commands: Commands) {
  commands.spawn((Camera2d, PrimaryEguiContext));
}

fn main() {
  let _ = env_logger::try_init();
  profiling::init_profiling();

  let runtime = build_runtime();
  let runtime_handle = runtime.handle().clone();
  let runtime_monitor = RuntimeMonitor::new(&runtime_handle);
  let native_tile_config = Config::new();

  App::new()
    .insert_non_send_resource(MapvasBevyState::new(runtime, runtime_monitor))
    .init_resource::<NativeGeometryLayer>()
    .insert_resource(NativeTileRuntime(runtime_handle))
    .insert_resource(NativeTileLayer::new(native_tile_config))
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
    .add_plugins(NativeTilePlugin)
    .add_plugins(NativeGeometryPlugin)
    .add_systems(Startup, setup_camera)
    .add_systems(EguiPrimaryContextPass, mapvas_ui_system)
    .run();
}
