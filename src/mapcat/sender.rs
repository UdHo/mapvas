use std::process::Stdio;

use log::debug;
use mapvas::{MapEvent, DEFAULT_PORT};

#[derive(Copy, Clone)]
pub struct MapSender {}

impl MapSender {
  pub async fn new() -> MapSender {
    let sender = Self {};
    sender.spawn_mapvas_if_needed().await;
    sender
  }

  async fn spawn_mapvas_if_needed(&self) {
    if surf::get(format!("http://localhost:{DEFAULT_PORT}/healthcheck"))
      .send()
      .await
      .is_ok()
    {
      return;
    }

    let _ = std::process::Command::new("mapvas")
      .arg("-w")
      .stderr(Stdio::null())
      .stdout(Stdio::null())
      .spawn();
    while let Err(e) = surf::get(format!("http://localhost:{DEFAULT_PORT}/healthcheck",))
      .send()
      .await
    {
      debug!("Healthcheck {}", e);
    }
  }

  pub async fn send_event(&self, event: &MapEvent) {
    let _ = surf::post(format!("http://localhost:{DEFAULT_PORT}/"))
      .body_json(&event)
      .expect("Cannot serialize json")
      .await;
  }
}
