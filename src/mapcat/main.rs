#![feature(async_closure)]

use std::str::FromStr;

use clap::Parser as CliParser;
use log::error;
use mapvas::map::map_event::Color;
use mapvas::parser::{GrepParser, Parser, RandomParser, TTJsonParser};

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
  /// Sets the number of the map window input is drawn on.
  #[arg(short, long, default_value = "0")]
  window: u16,
}

#[tokio::main]
async fn main() {
  let args = Args::parse();
  let color = Color::from_str(&args.color).unwrap_or(Color::Green);

  env_logger::init();

  let sender = sender::MapSender::new(args.window).await;

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
      match parser.finalize() {
        Some(e) => {
          let _ = tasks.spawn(async move {
            let _ = &sender.send_event(&e).await;
          });
          ()
        }
        None => (),
      }
      break;
    }
    match parser.parse_line(&line) {
      Some(e) => {
        let _ = tasks.spawn(async move {
          let _ = &sender.send_event(&e).await;
        });
        ()
      }
      None => (),
    }
    line.clear();
  }
  // Waiting for all tasks to finish.
  while let Some(_) = tasks.join_next().await {}
}
