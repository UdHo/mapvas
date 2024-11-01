use axum::{extract::State, Json};
use tokio::sync::mpsc::Sender;

use crate::map::map_event::MapEvent;

pub const DEFAULT_PORT: u16 = 12345;

pub async fn serve_axum(
  State(sender): State<Sender<MapEvent>>,
  Json(event): Json<MapEvent>,
) -> String {
  let _ = sender.send(event).await;
  42.to_string()
}

struct Remote {
  pub layer: Sender<MapEvent>,
  pub focus: Sender<MapEvent>,
  pub clear: Sender<MapEvent>,
  pub shutdown: Sender<MapEvent>,
  pub screenshot: Sender<MapEvent>,
  pub tile_data: Sender<MapEvent>,
}

impl Remote {
  async fn handle_map_event(&mut self, event: MapEvent) {
    match event {
      l @ MapEvent::Layer(_) => {
        let _ = self.layer.send(l).await;
      }
      MapEvent::Focus => {
        let _ = self.focus.send(MapEvent::Focus).await;
      }
      MapEvent::Clear => {
        let _ = self.clear.send(MapEvent::Clear).await;
      }
      MapEvent::Shutdown => {
        let _ = self.shutdown.send(MapEvent::Shutdown).await;
      }
      e @ MapEvent::Screenshot(_) => {
        let _ = self.screenshot.send(e).await;
      }
      t @ MapEvent::TileDataArrived { .. } => {
        let _ = self.tile_data.send(t).await;
      }
    }
  }
}
