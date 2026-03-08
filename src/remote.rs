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
    let listener = tokio::net::TcpListener::bind(addr)
      .await
      .expect("Port is free.");
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

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;
  use std::sync::mpsc;

  struct TestRemote {
    remote: Remote,
    layer_rx: mpsc::Receiver<MapEvent>,
    focus_rx: mpsc::Receiver<MapEvent>,
    clear_rx: mpsc::Receiver<MapEvent>,
    shutdown_rx: mpsc::Receiver<MapEvent>,
  }

  fn create_remote() -> TestRemote {
    let (layer_tx, layer_rx) = mpsc::channel();
    let (focus_tx, focus_rx) = mpsc::channel();
    let (clear_tx, clear_rx) = mpsc::channel();
    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let (screenshot_tx, _screenshot_rx) = mpsc::channel();
    let (command_tx, _command_rx) = mpsc::channel();
    let remote = Remote {
      layer: layer_tx,
      focus: focus_tx,
      clear: clear_tx,
      shutdown: shutdown_tx,
      screenshot: screenshot_tx,
      command: command_tx,
      update: egui::Context::default(),
    };
    TestRemote {
      remote,
      layer_rx,
      focus_rx,
      clear_rx,
      shutdown_rx,
    }
  }

  #[test]
  fn handle_layer_event() {
    let t = create_remote();
    let layer = crate::map::map_event::Layer::new("test".to_string());
    t.remote.handle_map_event(MapEvent::Layer(layer));
    let received = t
      .layer_rx
      .recv_timeout(std::time::Duration::from_secs(1))
      .unwrap();
    assert!(matches!(received, MapEvent::Layer(_)));
  }

  #[test]
  fn handle_focus_event() {
    let t = create_remote();
    t.remote.handle_map_event(MapEvent::Focus);
    let received = t
      .focus_rx
      .recv_timeout(std::time::Duration::from_secs(1))
      .unwrap();
    assert!(matches!(received, MapEvent::Focus));
  }

  #[test]
  fn handle_focus_on_event() {
    let t = create_remote();
    let event = MapEvent::FocusOn {
      coordinate: crate::map::coordinates::WGS84Coordinate::new(52.5, 13.4),
      zoom_level: Some(10),
    };
    t.remote.handle_map_event(event);
    let received = t
      .focus_rx
      .recv_timeout(std::time::Duration::from_secs(1))
      .unwrap();
    assert!(matches!(received, MapEvent::FocusOn { .. }));
  }

  #[test]
  fn handle_clear_event() {
    let t = create_remote();
    t.remote.handle_map_event(MapEvent::Clear);
    let received = t
      .clear_rx
      .recv_timeout(std::time::Duration::from_secs(1))
      .unwrap();
    assert!(matches!(received, MapEvent::Clear));
  }

  #[test]
  fn handle_shutdown_event() {
    let t = create_remote();
    t.remote.handle_map_event(MapEvent::Shutdown);
    let received = t
      .shutdown_rx
      .recv_timeout(std::time::Duration::from_secs(1))
      .unwrap();
    assert!(matches!(received, MapEvent::Shutdown));
  }

  #[test]
  fn handle_screenshot_event_goes_to_focus() {
    let t = create_remote();
    t.remote
      .handle_map_event(MapEvent::Screenshot(PathBuf::from("/tmp/test.png")));
    let received = t
      .focus_rx
      .recv_timeout(std::time::Duration::from_secs(1))
      .unwrap();
    assert!(matches!(received, MapEvent::Screenshot(_)));
  }
}
