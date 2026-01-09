use axum::{
  Json, Router,
  extract::{DefaultBodyLimit, State},
  routing::{get, post},
};
use log::warn;
use std::{net::SocketAddr, sync::mpsc::Sender};
use tower_http::trace::{self, TraceLayer};

use crate::map::{map_event::MapEvent, mapvas_egui::ParameterUpdate};

pub const DEFAULT_PORT: u16 = 12345;

pub async fn mapvas_remote_handler(
  State(remote): State<Remote>,
  Json(event): Json<MapEvent>,
) -> String {
  remote.handle_map_event(event);
  42.to_string()
}

pub fn spawn_remote_runner(runtime: tokio::runtime::Runtime, remote: Remote) {
  std::thread::spawn(move || {
    log::info!("Mapcat server starting on dedicated runtime");
    runtime.block_on(async {
      remote_runner(remote).await;
    });
  });
}

async fn healthcheck() {}

pub async fn remote_runner(remote: Remote) {
  let app = Router::new()
    .route("/", post(mapvas_remote_handler))
    .route("/healthcheck", get(healthcheck))
    .with_state(remote)
    .layer(DefaultBodyLimit::max(10_000_000_000_000))
    .layer(
      TraceLayer::new_for_http()
        .make_span_with(trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
        .on_response(trace::DefaultOnResponse::new().level(tracing::Level::INFO)),
    );

  tokio::spawn(async {
    let addr = SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT));
    log::info!("Mapcat server binding to {addr}");
    let listener = tokio::net::TcpListener::bind(addr)
      .await
      .expect("Port is free.");
    log::info!("Mapcat server listening on port {DEFAULT_PORT}");
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
  pub command: Sender<ParameterUpdate>,
  /// TODO: keep egui out of here.
  pub update: egui::Context,
}

impl Remote {
  pub fn handle_map_event(&self, event: MapEvent) {
    match event {
      l @ MapEvent::Layer(_) => {
        self
          .layer
          .send(l)
          .inspect_err(|e| warn!("Failed to send layer event: {e}"))
          .ok();
      }
      MapEvent::Focus => {
        self
          .focus
          .send(MapEvent::Focus)
          .inspect_err(|e| warn!("Failed to send focus event: {e}"))
          .ok();
      }
      f @ MapEvent::FocusOn { .. } => {
        self
          .focus
          .send(f)
          .inspect_err(|e| warn!("Failed to send focus_on event: {e}"))
          .ok();
      }
      MapEvent::Clear => {
        self
          .clear
          .send(MapEvent::Clear)
          .inspect_err(|e| warn!("Failed to send clear event: {e}"))
          .ok();
      }
      MapEvent::Shutdown => {
        self
          .shutdown
          .send(MapEvent::Shutdown)
          .inspect_err(|e| warn!("Failed to send shutdown event: {e}"))
          .ok();
      }
      e @ MapEvent::Screenshot(_) => {
        // Send screenshot events through the main event channel for proper ordering
        self
          .focus
          .send(e)
          .inspect_err(|e| warn!("Failed to send screenshot event: {e}"))
          .ok();
      }
    }
    // Delay repaint request to avoid deadlock during UI event processing
    let ctx = self.update.clone();
    std::thread::spawn(move || {
      std::thread::sleep(std::time::Duration::from_millis(1));
      ctx.request_repaint();
    });
  }

  /// Get a sender that properly routes all event types
  #[must_use]
  pub fn sender(&self) -> RoutingSender {
    RoutingSender {
      remote: self.clone(),
    }
  }
}

/// A sender that routes events through Remote's `handle_map_event`
pub struct RoutingSender {
  remote: Remote,
}

impl RoutingSender {
  /// Send a map event through the remote handler.
  ///
  /// # Errors
  ///
  /// This function currently never returns an error, but the signature is kept
  /// for future compatibility with potential channel send failures.
  pub fn send(&self, event: MapEvent) -> Result<(), std::sync::mpsc::SendError<MapEvent>> {
    self.remote.handle_map_event(event);
    Ok(())
  }
}
