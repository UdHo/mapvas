use std::path::Path;
use std::str::FromStr;

use clap::Parser as CliParser;
use log::error;
use mapvas::map::map_event::{Color, MapEvent};
use mapvas::parser::{FileParser, GrepParser, JsonParser, TTJsonParser};
use std::fs::File;
use std::io::{BufRead, BufReader};

mod sender;

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
  /// Which parser to use. Values: grep, ttjson.
  #[arg(short, long, default_value = "grep")]
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

  // Step 1: Clear map if requested - use separate sender for immediate execution
  if args.reset {
    let sender = sender::MapSender::new().await;
    sender.send_event(MapEvent::Clear);
    sender.finalize().await;
  }

  // Step 2: Parse and send data - use separate sender
  let sender = sender::MapSender::new().await;

  let parser = || -> Box<dyn FileParser> {
    match args.parser.as_str() {
      "ttjson" => Box::new(TTJsonParser::new().with_color(color)),
      "json" => Box::new(JsonParser::new()),
      "grep" => Box::new(
        GrepParser::new(args.invert_coordinates)
          .with_color(color)
          .with_label_pattern(&args.label_pattern),
      ),
      _ => {
        error!("Unkown parser: {}. Falling back to grep.", args.parser);
        Box::new(GrepParser::new(args.invert_coordinates))
      }
    }
  };

  let readers = readers(&args.files);
  for reader in readers {
    let mut parser = parser();
    parser.parse(reader).for_each(|e| sender.send_event(e));
  }
  sender.finalize().await;

  // Step 3: Focus if requested - use separate sender after data is sent
  if args.focus {
    let sender = sender::MapSender::new().await;
    sender.send_event(MapEvent::Focus);
    sender.finalize().await;
  }

  // Step 4: Screenshot if requested - use separate sender after focus
  if !args.screenshot.is_empty() {
    let sender = sender::MapSender::new().await;
    sender.send_event(MapEvent::Screenshot(
      std::path::absolute(Path::new(&args.screenshot.trim())).unwrap(),
    ));
    sender.finalize().await;
  }
}
