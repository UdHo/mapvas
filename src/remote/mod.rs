use axum::{extract::State, Json};
use winit::event_loop::EventLoopProxy;

use crate::map::map_event::MapEvent;

pub async fn serve_axum(
  State(proxy): State<EventLoopProxy<MapEvent>>,
  Json(event): Json<MapEvent>,
) -> String {
  let _ = proxy.send_event(event);
  42.to_string()
}
