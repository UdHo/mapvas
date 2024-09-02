use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use clap::Parser as CliParser;
use log::error;
use mapvas::map::map_event::{Color, MapEvent};
use mapvas::parser::{GrepParser, Parser, RandomParser, TTJsonParser};
use std::fs::File;
use std::io::{BufRead, BufReader};
use tokio::time::sleep;

mod sender;

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
  /// Which parser to use. Values: grep, random, ttjson.
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
  file: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let color = Color::from_str(&args.color).unwrap_or(Color::Green);

  env_logger::init();

  let sender = sender::MapSender::new().await;
  if args.reset {
    sender.send_event(MapEvent::Clear);
  }
  sender.finalize().await;
  let sender = sender::MapSender::new().await;

  let mut parser: Box<dyn Parser> = match args.parser.as_str() {
    "random" => Box::new(RandomParser::new()),
    "ttjson" => Box::new(TTJsonParser::new().with_color(color)),
    "grep" => Box::new(
      GrepParser::new(args.invert_coordinates)
        .with_color(color)
        .with_label_pattern(&args.label_pattern),
    ),
    _ => {
      error!("Unkown parser: {}. Falling back to grep.", args.parser);
      Box::new(GrepParser::new(args.invert_coordinates))
    }
  };

  let mut reader: Box<dyn BufRead> = if let Some(file) = args.file {
    let f = File::open(file);
    if let Ok(file) = f {
      Box::new(BufReader::new(file))
    } else {
      error!("File not found.");
      return;
    }
  } else {
    Box::new(std::io::stdin().lock())
  };

  let mut line = String::new();
  while let Ok(res) = reader.read_line(&mut line) {
    let parsed = if res == 0 {
      parser.finalize()
    } else {
      parser.parse_line(&line)
    };
    if let Some(e) = parsed {
      sender.send_event(e);
    }
    line.clear();
    if res == 0 {
      break;
    }
  }

  // Waiting for all tasks to finish.
  sender.finalize().await;

  if args.focus {
    let sender = sender::MapSender::new().await;
    sender.send_event(MapEvent::Focus);
    sender.finalize().await;
  }

  if !args.screenshot.is_empty() {
    sleep(Duration::from_millis(300)).await;
    let sender = sender::MapSender::new().await;
    sender.send_event(MapEvent::Screenshot(
      std::path::absolute(Path::new(&args.screenshot.trim())).unwrap(),
    ));
    sender.finalize().await;
  }
}
