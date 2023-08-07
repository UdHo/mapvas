#![feature(async_closure)]
#![feature(async_fn_in_trait)]

use axum::extract::DefaultBodyLimit;
use axum::{routing::get, routing::post, Router};
use mapvas::remote::serve_axum;
use mapvas::{map::mapvas::MapVas, MapEvent};
use std::net::SocketAddr;
use tokio::sync::mpsc::Sender;
use tracing_subscriber::EnvFilter;

use tower_http::trace::{self, TraceLayer};

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
      _ = ctrl_c => {},
      _ = terminate => {},
  }

  let _ = sender.send(MapEvent::Shutdown);
}

async fn healthcheck() {}

#[tokio::main]
async fn main() {
  let instance = single_instance::SingleInstance::new("MapVas").unwrap();
  if !instance.is_single() {
    return;
  }
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
    .layer(DefaultBodyLimit::max(10000000000000))
    .layer(
      TraceLayer::new_for_http()
        .make_span_with(trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
        .on_response(trace::DefaultOnResponse::new().level(tracing::Level::INFO)),
    );

  tokio::spawn((async move || {
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let _ = axum::Server::bind(&addr)
      .serve(app.into_make_service())
      .with_graceful_shutdown(shutdown_signal(sender))
      .await;
  })());

  widget.run().await;
}
