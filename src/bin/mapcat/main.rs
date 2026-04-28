use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use clap::Parser as CliParser;
use egui::Color32;
use log::error;
use mapvas::map::coordinates::PixelCoordinate;
use mapvas::map::geometry_collection::{Geometry, Metadata};
use mapvas::map::map_event::{Color, Layer, MapEvent};
use mapvas::parser::{
  AutoFileParser, FileParser, GeoJsonParser, GpxParser, GrepParser, JsonParser, KmlParser,
  TTJsonParser,
};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};

mod sender;

#[derive(Debug, Clone)]
struct FileArg {
  path: std::path::PathBuf,
  heatmap: bool,
}

impl std::str::FromStr for FileArg {
  type Err = std::convert::Infallible;
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    if let Some(path) = s.strip_suffix(":heatmap") {
      Ok(Self {
        path: path.into(),
        heatmap: true,
      })
    } else {
      Ok(Self {
        path: s.into(),
        heatmap: false,
      })
    }
  }
}

/// Walk a geometry tree and append every coordinate it contains to `out`.
fn collect_coords(geometry: &Geometry<PixelCoordinate>, out: &mut Vec<PixelCoordinate>) {
  match geometry {
    Geometry::Point(c, _) => out.push(*c),
    Geometry::LineString(coords, _) | Geometry::Polygon(coords, _) => {
      out.extend(coords.iter().copied());
    }
    Geometry::Heatmap(coords, _) => out.extend(coords.iter().copied()),
    Geometry::GeometryCollection(geometries, _) => {
      for g in geometries {
        collect_coords(g, out);
      }
    }
  }
}

/// Replace every `MapEvent::Layer` in `events` with a single `Geometry::Heatmap`
/// per layer id, accumulating all the coordinates from the original geometries.
fn convert_to_heatmap(events: Vec<MapEvent>) -> Vec<MapEvent> {
  let mut heatmap_coords: BTreeMap<String, Vec<PixelCoordinate>> = BTreeMap::new();
  let mut other = Vec::new();

  for event in events {
    if let MapEvent::Layer(Layer { id, geometries }) = event {
      let entry = heatmap_coords.entry(id).or_default();
      for g in &geometries {
        collect_coords(g, entry);
      }
    } else {
      other.push(event);
    }
  }

  let mut result = Vec::with_capacity(other.len() + heatmap_coords.len());
  for (id, coords) in heatmap_coords {
    if coords.is_empty() {
      continue;
    }
    result.push(MapEvent::Layer(Layer {
      id,
      geometries: vec![Geometry::Heatmap(
        std::sync::Arc::new(coords),
        Metadata::default(),
      )],
    }));
  }
  result.extend(other);
  result
}

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)]
struct Args {
  /// Which parser to use. Values: auto (file extension based with fallbacks), grep, ttjson, json, geojson, gpx, kml.
  #[arg(short, long, default_value = "auto")]
  parser: String,

  /// Inverts the normal lat/lon when using grep as parser.
  #[arg(short, long, default_value_t = false)]
  invert_coordinates: bool,

  /// Sets the default color for most parsers. Values: red, blue, green, yellow, black,...
  /// If you need more just look in the code.
  #[arg(short, long, default_value = "blue")]
  color: String,

  /// Clears the map before drawing new stuff.
  #[arg(short, long)]
  reset: bool,

  /// Zooms to the bounding box of drawn stuff.
  #[arg(short, long)]
  focus: bool,

  /// Defines a regex with one capture group labels.
  #[arg(short, long, default_value = "(.*)")]
  label_pattern: String,

  /// Path to take a screenshot.
  #[arg(short, long, default_value = "")]
  screenshot: String,

  /// Render all input coordinates as a heatmap instead of individual shapes.
  #[arg(short = 'H', long)]
  heatmap: bool,

  /// Render directly to an image file instead of sending to mapvas
  #[arg(short, long)]
  output: Option<std::path::PathBuf>,

  /// Render without map background (black background, only geometries). Only with --output.
  #[arg(long)]
  no_map: bool,

  /// Image width in pixels (only with --output)
  #[arg(long, default_value_t = 1980)]
  width: u32,

  /// Image height in pixels (only with --output)
  #[arg(long, default_value_t = 1200)]
  height: u32,

  /// Files to parse. stdin is used if not provided.
  /// Append `:heatmap` to a file to render it as a heatmap (e.g. `points.geojson:heatmap`).
  files: Vec<FileArg>,
}

fn render_output(args: &Args) {
  use mapvas::{
    config::Config, headless::HeadlessRenderer, map::tile_renderer::init_style_config,
    parser::ContentAutoParser,
  };

  let config = Config::new();
  init_style_config(config.vector_style_file.as_deref());

  let renderer = if args.no_map {
    HeadlessRenderer::with_config(config)
      .without_map()
      .with_background_color(Color32::BLACK)
  } else {
    HeadlessRenderer::with_config(config)
  };

  let mut events: Vec<MapEvent> = Vec::new();
  if args.files.is_empty() {
    // Read from stdin
    let mut content = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut content) {
      eprintln!("Error: Failed to read from stdin: {e}");
      std::process::exit(1);
    }
    let content_parser = ContentAutoParser::new(content)
      .with_label_pattern(&args.label_pattern)
      .with_invert_coordinates(args.invert_coordinates);
    events.extend(content_parser.parse());
  } else {
    for file_arg in &args.files {
      let mut parser = AutoFileParser::new(&file_arg.path)
        .with_label_pattern(&args.label_pattern)
        .with_invert_coordinates(args.invert_coordinates);
      let file_events: Vec<MapEvent> = parser.parse().collect();
      if args.heatmap || file_arg.heatmap {
        events.extend(convert_to_heatmap(file_events));
      } else {
        events.extend(file_events);
      }
    }
  }

  if args.files.is_empty() && args.heatmap {
    events = convert_to_heatmap(events);
  }

  // Always focus on data in output mode
  events.push(MapEvent::Focus);

  let img = renderer.render(&events, args.width, args.height);

  let output_path = args.output.as_ref().expect("Headless path should be set");
  if let Err(e) = img.save(output_path) {
    eprintln!("Failed to save image: {e}");
    std::process::exit(1);
  }
  eprintln!(
    "Saved {}x{} image to {}",
    args.width,
    args.height,
    output_path.display()
  );
}

fn readers(files: &[FileArg]) -> Vec<(Box<dyn BufRead>, bool)> {
  let mut res: Vec<(Box<dyn BufRead>, bool)> = Vec::new();
  if files.is_empty() {
    res.push((Box::new(std::io::stdin().lock()), false));
  } else {
    for f in files {
      match File::open(&f.path) {
        Ok(file) => res.push((Box::new(BufReader::new(file)), f.heatmap)),
        Err(e) => {
          eprintln!("Error: Failed to open file '{}': {}", f.path.display(), e);
          std::process::exit(1);
        }
      }
    }
  }
  res
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let color = Color::from_str(&args.color).unwrap_or_default();

  env_logger::init();

  if args.output.is_some() {
    render_output(&args);
    return;
  }

  let (initial_sender, mapvas_was_spawned) = sender::MapSender::new().await;

  if args.reset && !mapvas_was_spawned {
    initial_sender.send_event(MapEvent::Clear);
    initial_sender.finalize().await;
  } else {
    drop(initial_sender);
  }

  let (sender, _) = sender::MapSender::new().await;

  let send_events = |events: Box<dyn Iterator<Item = MapEvent>>| {
    for event in events {
      sender.send_event(event);
    }
  };

  if args.parser == "auto" {
    if args.files.is_empty() {
      // Use content-based auto-parser for stdin
      log::info!("Auto parser analyzing stdin content...");
      let mut content = String::new();
      if let Err(e) = std::io::stdin().read_to_string(&mut content) {
        eprintln!("Error: Failed to read from stdin: {e}");
        std::process::exit(1);
      }

      let content_parser = mapvas::parser::ContentAutoParser::new(content)
        .with_label_pattern(&args.label_pattern)
        .with_invert_coordinates(args.invert_coordinates);
      let events: Vec<MapEvent> = content_parser.parse();
      if args.heatmap {
        send_events(Box::new(convert_to_heatmap(events).into_iter()));
      } else {
        send_events(Box::new(events.into_iter()));
      }
    } else {
      // Use file-based auto-parser for each file
      for file_arg in &args.files {
        let mut auto_parser = AutoFileParser::new(&file_arg.path)
          .with_label_pattern(&args.label_pattern)
          .with_invert_coordinates(args.invert_coordinates);
        let events: Vec<MapEvent> = auto_parser.parse().collect();
        if args.heatmap || file_arg.heatmap {
          send_events(Box::new(convert_to_heatmap(events).into_iter()));
        } else {
          send_events(Box::new(events.into_iter()));
        }
      }
    }
  } else {
    // Use specified parser for all inputs
    let parser = || -> Box<dyn FileParser> {
      match args.parser.as_str() {
        "ttjson" => Box::new(TTJsonParser::new().with_color(color)),
        "json" => Box::new(JsonParser::new()),
        "geojson" => Box::new(GeoJsonParser::new()),
        "gpx" => Box::new(GpxParser::new()),
        "kml" => Box::new(KmlParser::new()),
        "grep" => Box::new(
          GrepParser::new(args.invert_coordinates)
            .with_color(color)
            .with_label_pattern(&args.label_pattern),
        ),
        _ => {
          error!("Unknown parser: {}. Falling back to grep.", args.parser);
          Box::new(GrepParser::new(args.invert_coordinates))
        }
      }
    };

    let readers = readers(&args.files);
    for (reader, file_heatmap) in readers {
      let mut parser = parser();
      let events: Vec<MapEvent> = parser.parse(reader).collect();
      if args.heatmap || file_heatmap {
        send_events(Box::new(convert_to_heatmap(events).into_iter()));
      } else {
        send_events(Box::new(events.into_iter()));
      }
    }
  }

  if args.focus {
    sender.send_event(MapEvent::Focus);
  }

  if !args.screenshot.is_empty() {
    match std::path::absolute(Path::new(&args.screenshot.trim())) {
      Ok(path) => sender.send_event(MapEvent::Screenshot(path)),
      Err(e) => {
        eprintln!(
          "Error: Invalid screenshot path '{}': {}",
          args.screenshot, e
        );
        std::process::exit(1);
      }
    }
  }

  sender.finalize().await;
}
