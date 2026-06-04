use bevy::{log::LogPlugin, prelude::*, window::WindowResolution};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui};
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

    let (map, remote, data_holder) = Map::new(egui_ctx, config.clone());
    spawn_remote_runner_on_handle(self.runtime.handle().clone(), remote.clone());

    self.app = Some(MapApp::new(
      map,
      remote,
      data_holder,
      config,
      self.runtime_monitor.take(),
    ));
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

fn mapvas_ui_system(mut contexts: EguiContexts, mut state: NonSendMut<MapvasBevyState>) -> Result {
  let egui_ctx = contexts.ctx_mut()?.clone();
  let runtime_handle = state.runtime.handle().clone();
  let _runtime_guard = runtime_handle.enter();

  state.initialize(egui_ctx.clone());

  egui::CentralPanel::default()
    .frame(egui::Frame::NONE)
    .show(&egui_ctx, |ui| {
      if let Some(app) = state.app.as_mut() {
        app.show(ui);
      }
    });

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

  App::new()
    .insert_non_send_resource(MapvasBevyState::new(runtime, runtime_monitor))
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
    .add_systems(Startup, setup_camera)
    .add_systems(EguiPrimaryContextPass, mapvas_ui_system)
    .run();
}
