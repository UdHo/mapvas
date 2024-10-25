use mapvas::map::mapvas_egui::{map_ui, MapData};

#[derive(Default)]
struct MapApp {
  map: MapData,
}
impl eframe::App for MapApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    egui::CentralPanel::default().show(ctx, |ui| {
      map_ui(ui, &mut self.map);
    });
  }
}

fn main() -> eframe::Result {
  // init logger.
  env_logger::init();

  // start tokio on another thread.
  let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
  let _enter = rt.enter();
  std::thread::spawn(move || {
    rt.block_on(async {
      loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
      }
    });
  });

  let options = eframe::NativeOptions {
    ..Default::default()
  };
  eframe::run_native(
    "My egui App",
    options,
    Box::new(|cc| {
      // This gives us image support:
      egui_extras::install_image_loaders(&cc.egui_ctx);

      Ok(Box::new(MapApp {
        map: MapData::new(),
      }))
    }),
  )
}
