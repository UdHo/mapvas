use mapvas::{
  map::{map_event::MapEvent, mapvas::MapVas},
  remote::{serve_axum, DEFAULT_PORT},
};

use std::net::SocketAddr;

use axum::extract::DefaultBodyLimit;
use axum::{routing::get, routing::post, Router};
use tokio::sync::mpsc::Sender;
use tower_http::trace::{self, TraceLayer};
use tracing_subscriber::EnvFilter;

async fn shutdown_signal(sender: Sender<MapEvent>) {
  let ctrl_c = async {
    tokio::signal::ctrl_c()
      .await
      .expect("failed to install Ctrl+C handler");
  };

  #[cfg(unix)]
  let terminate = async {
    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
      .expect("failed to install signal handler")
      .recv()
      .await;
  };

  #[cfg(not(unix))]
  let terminate = std::future::pending::<()>();

  tokio::select! {
      () = ctrl_c => {},
      () = terminate => {},
  }

  let _ = sender.send(MapEvent::Shutdown).await;
}

async fn healthcheck() {}

#[tokio::main]
async fn main() {
  tracing_subscriber::fmt()
    .with_target(false)
    .with_env_filter(EnvFilter::from_default_env())
    .compact()
    .init();

  let widget: MapVas = MapVas::new();
  let sender = widget.get_event_sender();
  let app = Router::new()
    .route("/", post(serve_axum))
    .route("/healtcheck", get(healthcheck))
    .with_state(sender.clone())
    .layer(DefaultBodyLimit::max(10_000_000_000_000))
    .layer(
      TraceLayer::new_for_http()
        .make_span_with(trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
        .on_response(trace::DefaultOnResponse::new().level(tracing::Level::INFO)),
    );

  tokio::spawn(async {
    let addr = SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let _ = axum::serve(listener, app)
      .with_graceful_shutdown(shutdown_signal(sender))
      .await;
  });

  widget.run();
}
