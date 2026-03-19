use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use egui::IconData;
use mapvas::{
  config::Config,
  map::{mapvas_egui::Map, tile_renderer::init_style_config},
  mapvas_ui::MapApp,
  profiling,
  remote::spawn_remote_runner,
};
use tokio_metrics::RuntimeMonitor;

#[derive(Parser)]
#[command(name = "mapvas", about = "A map viewer with drawing functionality")]
struct Cli {
  /// Input files to load (geojson, kml, gpx, json, etc.)
  #[arg()]
  files: Vec<PathBuf>,

  /// Render headlessly to an image file (no window)
  #[arg(long)]
  headless: bool,

  /// Output image path (used with --headless)
  #[arg(short, long, default_value = "screenshot.png")]
  output: PathBuf,

  /// Image width in pixels (used with --headless)
  #[arg(long, default_value_t = 1600)]
  width: u32,

  /// Image height in pixels (used with --headless)
  #[arg(long, default_value_t = 1200)]
  height: u32,

}

fn load_icon() -> Option<Arc<IconData>> {
  Some(Arc::new(
    eframe::icon_data::from_png_bytes(include_bytes!("../../../logo.png")).ok()?,
  ))
}

fn run_headless(cli: &Cli) {
  use mapvas::{
    headless::HeadlessRenderer,
    map::map_event::MapEvent,
    parser::AutoFileParser,
  };

  let config = Config::new();
  init_style_config(config.vector_style_file.as_deref());

  let renderer = HeadlessRenderer::with_config(config);

  let mut events: Vec<MapEvent> = Vec::new();
  for path in &cli.files {
    let mut parser = AutoFileParser::new(path);
    events.extend(parser.parse());
  }
  events.push(MapEvent::Focus);

  let img = renderer.render(&events, cli.width, cli.height);

  if let Err(e) = img.save(&cli.output) {
    eprintln!("Failed to save image to {}: {e}", cli.output.display());
    std::process::exit(1);
  }
  eprintln!(
    "Saved {}x{} image to {}",
    cli.width,
    cli.height,
    cli.output.display()
  );
}

fn main() -> eframe::Result {
  env_logger::init();
  profiling::init_profiling();

  let cli = Cli::parse();

  if cli.headless {
    // Headless mode needs a tokio runtime for tile loading
    let runtime = tokio::runtime::Builder::new_multi_thread()
      .worker_threads(4)
      .thread_name("async-io")
      .enable_all()
      .build()
      .expect("Failed to create tokio runtime");
    let _enter = runtime.enter();

    run_headless(&cli);
    return Ok(());
  }

  // GUI mode
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
