use axum::{extract::State, Json};
use tokio::sync::mpsc::Sender;

use crate::map::map_event::MapEvent;

pub async fn serve_axum(
  State(sender): State<Sender<MapEvent>>,
  Json(event): Json<MapEvent>,
) -> String {
  let _ = sender.send(event).await;
  42.to_string()
}
