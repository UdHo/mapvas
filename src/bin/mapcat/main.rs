use std::path::Path;
use std::str::FromStr;

use clap::Parser as CliParser;
use log::error;
use mapvas::map::map_event::{Color, MapEvent};
use mapvas::parser::{
  AutoFileParser, FileParser, GeoJsonParser, GpxParser, GrepParser, JsonParser, KmlParser,
  TTJsonParser,
};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};

mod sender;

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
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

  /// A file to parse. stdin is used if this is not provided.
  files: Vec<std::path::PathBuf>,
}

fn readers(paths: &[std::path::PathBuf]) -> Vec<Box<dyn BufRead>> {
  let mut res: Vec<Box<dyn BufRead>> = Vec::new();
  if paths.is_empty() {
    res.push(Box::new(std::io::stdin().lock()));
  } else {
    for f in paths {
      res.push(Box::new(BufReader::new(
        File::open(f).expect("File exists"),
      )));
    }
  }
  res
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let color = Color::from_str(&args.color).unwrap_or_default();

  env_logger::init();

  let (initial_sender, mapvas_was_spawned) = sender::MapSender::new().await;

  if args.reset && !mapvas_was_spawned {
    initial_sender.send_event(MapEvent::Clear);
    initial_sender.finalize().await;
  } else {
    drop(initial_sender);
  }

  let (sender, _) = sender::MapSender::new().await;

  if args.parser == "auto" {
    if args.files.is_empty() {
      // Use content-based auto-parser for stdin
      log::info!("Auto parser analyzing stdin content...");
      let mut content = String::new();
      std::io::stdin()
        .read_to_string(&mut content)
        .expect("Failed to read from stdin");

      let content_parser = mapvas::parser::ContentAutoParser::new(content)
        .with_label_pattern(&args.label_pattern)
        .with_invert_coordinates(args.invert_coordinates);
      for event in content_parser.parse() {
        sender.send_event(event);
      }
    } else {
      // Use file-based auto-parser for each file
      for file_path in &args.files {
        let mut auto_parser = AutoFileParser::new(file_path)
          .with_label_pattern(&args.label_pattern)
          .with_invert_coordinates(args.invert_coordinates);
        auto_parser.parse().for_each(|e| sender.send_event(e));
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
    for reader in readers {
      let mut parser = parser();
      parser.parse(reader).for_each(|e| sender.send_event(e));
    }
  }

  if args.focus {
    sender.send_event(MapEvent::Focus);
  }

  if !args.screenshot.is_empty() {
    sender.send_event(MapEvent::Screenshot(
      std::path::absolute(Path::new(&args.screenshot.trim())).unwrap(),
    ));
  }

  sender.finalize().await;
}
