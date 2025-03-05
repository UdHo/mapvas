use axum::{
  extract::{DefaultBodyLimit, State},
  routing::{get, post},
  Json, Router,
};
use std::{net::SocketAddr, sync::mpsc::Sender};
use tower_http::trace::{self, TraceLayer};

use crate::map::map_event::MapEvent;

pub const DEFAULT_PORT: u16 = 12345;

pub async fn serve_axum(
  State(sender): State<Sender<MapEvent>>,
  Json(event): Json<MapEvent>,
) -> String {
  let _ = sender.send(event);
  42.to_string()
}

pub async fn mapvas_remote_handler(
  State(remote): State<Remote>,
  Json(event): Json<MapEvent>,
) -> String {
  let _ = remote.handle_map_event(event).await;
  42.to_string()
}

pub fn spawn_remote_runner(runtime: tokio::runtime::Runtime, remote: Remote) {
  // start tokio on another thread.
  let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
  let _enter = rt.enter();

  std::thread::spawn(move || {
    runtime.block_on(async {
      remote_runner(remote).await;
    });
  });
}

async fn healthcheck() {}

pub async fn remote_runner(remote: Remote) {
  let app = Router::new()
    .route("/", post(mapvas_remote_handler))
    .route("/healtcheck", get(healthcheck))
    .with_state(remote)
    .layer(DefaultBodyLimit::max(10_000_000_000_000))
    .layer(
      TraceLayer::new_for_http()
        .make_span_with(trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
        .on_response(trace::DefaultOnResponse::new().level(tracing::Level::INFO)),
    );

  tokio::spawn(async {
    let addr = SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let _ = axum::serve(listener, app).await;
  });

  // Keep it running.
  loop {
    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
  }
}

#[derive(Clone)]
pub struct Remote {
  pub layer: Sender<MapEvent>,
  pub focus: Sender<MapEvent>,
  pub clear: Sender<MapEvent>,
  pub shutdown: Sender<MapEvent>,
  pub screenshot: Sender<MapEvent>,
  /// TODO: keep egui out of here.
  pub update: egui::Context,
}

impl Remote {
  pub async fn handle_map_event(&self, event: MapEvent) {
    match event {
      l @ MapEvent::Layer(_) => {
        let _ = self.layer.send(l);
      }
      MapEvent::Focus => {
        let _ = self.focus.send(MapEvent::Focus);
      }
      MapEvent::Clear => {
        let _ = self.clear.send(MapEvent::Clear);
      }
      MapEvent::Shutdown => {
        let _ = self.shutdown.send(MapEvent::Shutdown);
      }
      e @ MapEvent::Screenshot(_) => {
        let _ = self.screenshot.send(e);
      }
    }
    self.update.request_repaint();
  }
}
