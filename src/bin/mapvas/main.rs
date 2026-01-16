use std::sync::Arc;

use egui::IconData;
use mapvas::{
  config::Config, map::mapvas_egui::Map, mapvas_ui::MapApp, profiling, remote::spawn_remote_runner,
};
use tokio_metrics::RuntimeMonitor;

fn load_icon() -> Option<Arc<IconData>> {
  Some(Arc::new(
    eframe::icon_data::from_png_bytes(include_bytes!("../../../logo.png")).ok()?,
  ))
}

fn main() -> eframe::Result {
  // init logger.
  env_logger::init();

  // Initialize profiling
  profiling::init_profiling();

  // Single tokio runtime for all async I/O operations
  // CPU-bound rendering is handled by rayon thread pool (see src/render_pool.rs)
  let runtime = match tokio::runtime::Builder::new_multi_thread()
    .worker_threads(4)  // Enough for I/O operations (downloads, HTTP server)
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

  // Create runtime monitor for metrics
  let runtime_handle = runtime.handle().clone();
  let runtime_monitor = RuntimeMonitor::new(&runtime_handle);

  // Enter runtime globally (used by ambient tokio::spawn calls)
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
      // Image support
      egui_extras::install_image_loaders(&cc.egui_ctx);

      let config = Config::new();
      let (map, remote, data_holder) = Map::new(cc.egui_ctx.clone());
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
