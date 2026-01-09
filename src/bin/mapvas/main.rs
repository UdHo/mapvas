use std::sync::Arc;

use egui::IconData;
use mapvas::{
  config::Config, map::mapvas_egui::Map, mapvas_ui::MapApp, profiling, remote::spawn_remote_runner,
};

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

  // Create dedicated runtime for HTTP server (high priority, isolated)
  let server_rt = match tokio::runtime::Builder::new_multi_thread()
    .worker_threads(2)
    .thread_name("mapcat-server")
    .enable_all()
    .build()
  {
    Ok(rt) => rt,
    Err(e) => {
      eprintln!("Error: Failed to create server runtime: {e}");
      std::process::exit(1);
    }
  };

  // Create separate runtime for tile operations (can be saturated without affecting server)
  let tile_rt = match tokio::runtime::Builder::new_multi_thread()
    .worker_threads(4)
    .max_blocking_threads(32)  // Ensure blocking pool can handle concurrent renders
    .thread_name("tile-loader")
    .enable_all()
    .build()
  {
    Ok(rt) => rt,
    Err(e) => {
      eprintln!("Error: Failed to create tile runtime: {e}");
      std::process::exit(1);
    }
  };

  // Enter tile runtime globally (used by ambient tokio::spawn calls)
  let _tile_enter = tile_rt.enter();

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
    Box::new(|cc| {
      // Image support
      egui_extras::install_image_loaders(&cc.egui_ctx);

      let config = Config::new();
      let (map, remote, data_holder) = Map::new(cc.egui_ctx.clone());
      spawn_remote_runner(server_rt, remote.clone());
      Ok(Box::new(MapApp::new(map, remote, data_holder, config)))
    }),
  )
}
