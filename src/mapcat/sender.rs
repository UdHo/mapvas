use std::process::Stdio;

use log::debug;
use mapvas::MapEvent;
use single_instance::SingleInstance;

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
    let check = SingleInstance::new(&format!("MapVas: {}", self.window)).unwrap();
    if !check.is_single() {
      return;
    }
    drop(check);
    let window = self.window.to_string();
    let _ = std::process::Command::new("mapvas")
      .arg("-w")
      .arg(&window.as_str())
      .stderr(Stdio::null())
      .stdout(Stdio::null())
      .spawn();
    while let Err(e) = surf::get(format!(
      "http://localhost:{}/healthcheck",
      self.window + 8080
    ))
    .await
    {
      debug!("Healthcheck {}", e);
    }
  }

  pub async fn send_event(&self, event: &MapEvent) {
    let _ = surf::post(format!("http://localhost:{}/", &self.window + 8080))
      .body_json(&event)
      .expect("Cannot serialize json")
      .await;
  }
}
