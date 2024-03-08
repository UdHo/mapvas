#![feature(async_closure)]

use std::str::FromStr;

use clap::Parser as CliParser;
use log::error;
use mapvas::map::map_event::Color;
use mapvas::parser::{GrepParser, Parser, RandomParser, TTJsonParser};
use mapvas::MapEvent;

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
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let color = Color::from_str(&args.color).unwrap_or(Color::Green);

  env_logger::init();

  let sender = sender::MapSender::new().await;
  if args.reset {
    sender.send_event(&MapEvent::Clear).await;
  }

  let mut tasks = tokio::task::JoinSet::new();

  let mut parser: Box<dyn Parser> = match args.parser.as_str() {
    "random" => Box::new(RandomParser::new()),
    "ttjson" => Box::new(TTJsonParser::new().with_color(color)),
    "grep" => Box::new(GrepParser::new(args.invert_coordinates).with_color(color)),
    _ => {
      error!("Unkown parser: {}. Falling back to grep.", args.parser);
      Box::new(GrepParser::new(args.invert_coordinates))
    }
  };
  let stdin = async_std::io::stdin();
  let mut line = String::new();
  while let Ok(res) = stdin.read_line(&mut line).await {
    if res == 0 {
      if let Some(e) = parser.finalize() {
        let _ = tasks.spawn(async move {
          let _ = &sender.send_event(&e).await;
        });
      }
      break;
    }
    if let Some(e) = parser.parse_line(&line) {
      let _ = tasks.spawn(async move {
        let _ = &sender.send_event(&e).await;
      });
    }
    line.clear();
  }

  // Waiting for all tasks to finish.
  while (tasks.join_next().await).is_some() {}

  if args.focus {
    sender.send_event(&MapEvent::Focus).await;
  }

  // Waiting for all tasks to finish.
  while (tasks.join_next().await).is_some() {}
}
