use mapvas::MapEvent;
use parser::{GrepParser, Parser};
use single_instance::SingleInstance;

pub mod parser;

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

#[tokio::main]
async fn main() {
  env_logger::init();

  spawn_mapvas_if_needed().await;

  let mut tasks = tokio::task::JoinSet::new();
  let mut parser = GrepParser::default();
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
