use egui::Widget as _;
use mapvas::{
  map::mapvas_egui::Map,
  remote::{remote_runner, spawn_remote_runner},
};

#[derive(Default)]
struct MapApp {
  map: Map,
}
impl eframe::App for MapApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    egui::CentralPanel::default().show(ctx, |ui| {
      (&mut self.map).ui(ui);
    });
  }
}
fn main() -> eframe::Result {
  // init logger.
  env_logger::init();

  // Tokio runtime.
  let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
  let _enter = rt.enter();

  let options = eframe::NativeOptions {
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
