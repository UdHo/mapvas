use std::sync::Arc;

use egui::IconData;
use mapvas::{config::Config, map::mapvas_egui::Map, mapvas_ui::MapApp, remote::spawn_remote_runner};

fn load_icon() -> Option<Arc<IconData>> {
  Some(Arc::new(
    eframe::icon_data::from_png_bytes(include_bytes!("../../../logo.png")).ok()?,
  ))
}

fn main() -> eframe::Result {
  // init logger.
  env_logger::init();

  // Tokio runtime.
  let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
  let _enter = rt.enter();

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
      spawn_remote_runner(rt, remote.clone());
      Ok(Box::new(MapApp::new(map, remote, data_holder, config)))
    }),
  )
}
