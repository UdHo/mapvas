use std::sync::Arc;

use egui::{IconData, Widget as _};
use mapvas::{map::mapvas_egui::Map, remote::spawn_remote_runner};

struct MapApp {
  map: Map,
}
impl eframe::App for MapApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    egui::CentralPanel::default()
      .frame(egui::Frame::none())
      .show(ctx, |ui| {
        (&mut self.map).ui(ui);
      });
  }
}

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
      // This gives us image support:
      egui_extras::install_image_loaders(&cc.egui_ctx);
      let (mapapp, remote) = Map::new(cc.egui_ctx.clone());
      spawn_remote_runner(rt, remote);

      Ok(Box::new(MapApp { map: mapapp }))
    }),
  )
}
