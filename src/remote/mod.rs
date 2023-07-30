use axum::{extract::State, Json};
use winit::event_loop::EventLoopProxy;

use crate::map::mapvas::MapEvent;

#[derive(serde::Serialize, serde::Deserialize)]
pub enum Query {
  Shutdown,
}
pub async fn serve_axum(
  State(_proxy): State<EventLoopProxy<MapEvent>>,
  Json(_): Json<Query>,
) -> String {
  42.to_string()
}
