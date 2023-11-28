use std::process::Stdio;

use log::debug;
use mapvas::{MapEvent, DEFAULT_PORT};

#[derive(Copy, Clone)]
pub struct MapSender {
  window: u16,
}

impl MapSender {
  pub async fn new(window: u16) -> MapSender {
    let sender = Self { window };
    sender.spawn_mapvas_if_needed().await;
    sender
  }

  async fn spawn_mapvas_if_needed(&self) {
    let window = self.window.to_string();
    if let Ok(_) = surf::get(format!(
      "http://localhost:{}/healthcheck",
      self.window + DEFAULT_PORT
    ))
    .send()
    .await
    {
      return;
    }

    let _ = std::process::Command::new("mapvas")
      .arg("-w")
      .arg(&window.as_str())
      .stderr(Stdio::null())
      .stdout(Stdio::null())
      .spawn();
    while let Err(e) = surf::get(format!(
      "http://localhost:{}/healthcheck",
      self.window + DEFAULT_PORT
    ))
    .send()
    .await
    {
      debug!("Healthcheck {}", e);
    }
  }

  pub async fn send_event(&self, event: &MapEvent) {
    let _ = surf::post(format!("http://localhost:{}/", &self.window + DEFAULT_PORT))
      .body_json(&event)
      .expect("Cannot serialize json")
      .await;
  }
}
