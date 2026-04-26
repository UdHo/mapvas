use std::sync::Arc;

use egui::IconData;
use mapvas::{
  config::Config,
  map::{mapvas_egui::Map, tile_renderer::init_style_config},
  mapvas_ui::MapApp,
  profiling,
  remote::spawn_remote_runner,
};
use tokio_metrics::RuntimeMonitor;

fn load_icon() -> Option<Arc<IconData>> {
  Some(Arc::new(
    eframe::icon_data::from_png_bytes(include_bytes!("../../../logo.png")).ok()?,
  ))
}

fn main() -> eframe::Result {
  env_logger::init();
  profiling::init_profiling();

  let runtime = match tokio::runtime::Builder::new_multi_thread()
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
  };

  let runtime_handle = runtime.handle().clone();
  let runtime_monitor = RuntimeMonitor::new(&runtime_handle);
  let _enter = runtime.enter();

  let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder {
      inner_size: Some(egui::vec2(1600.0, 1200.0)),
      clamp_size_to_monitor_size: Some(true),
      icon: load_icon(),
      ..Default::default()
    },
    ..Default::default()
  };

  eframe::run_native(
    "mapvas",
    options,
    Box::new(move |cc| {
      egui_extras::install_image_loaders(&cc.egui_ctx);

      let config = Config::new();
      init_style_config(config.vector_style_file.as_deref());

      let (map, remote, data_holder) = Map::new(cc.egui_ctx.clone(), config.clone());
      spawn_remote_runner(runtime, remote.clone());
      Ok(Box::new(MapApp::new(
        map,
        remote,
        data_holder,
        config,
        Some(runtime_monitor),
      )))
    }),
  )
}
