use clap::Parser as CliParser;
use log::error;
use mapvas::parser::{GrepParser, Parser, RandomParser};
use mapvas::MapEvent;
use single_instance::SingleInstance;

async fn send_event(event: &MapEvent) {
  let _ = surf::post("http://localhost:8080/")
    .body_json(&event)
    .expect("Cannot serialize json")
    .await;
}

async fn spawn_mapvas_if_needed() {
  let check = SingleInstance::new("MapVas").unwrap();
  if !check.is_single() {
    return;
  }
  drop(check);
  let _ = std::process::Command::new("mapvas").spawn();
  while let Err(_) = surf::get("http://localhost:8080/healthcheck").await {}
}

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
  /// Which parser to use. Values: grep, random.
  #[arg(short, long, default_value = "grep")]
  parser: String,
  /// Inverts the normal lat/lon when using grep as parser.
  #[arg(short, long, default_value_t = false)]
  invert_coordinates: bool,
}

#[tokio::main]
async fn main() {
  let args = Args::parse();

  env_logger::init();

  spawn_mapvas_if_needed().await;

  let mut tasks = tokio::task::JoinSet::new();

  let mut parser: Box<dyn Parser> = match args.parser.as_str() {
    "random" => Box::new(RandomParser::new()),
    "grep" => Box::new(GrepParser::new(args.invert_coordinates)),
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
            send_event(&e).await;
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
          send_event(&e).await;
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
