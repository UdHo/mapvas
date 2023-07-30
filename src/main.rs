#![feature(async_closure)]
#![feature(async_fn_in_trait)]
mod coordinates;
mod mapvas;
mod tile_loader;

use axum::{routing::get, routing::post, Router};
use std::{env, error::Error, net::SocketAddr, sync::Arc};
use tokio::sync::oneshot::channel;
use tower_http::trace::{self, TraceLayer};

async fn root() -> &'static str {
  "Hello, World!"
}
async fn shutdown_signal() {
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

  eprintln!("signal received, starting graceful shutdown");
}

#[tokio::main]
async fn main() {
  tracing_subscriber::fmt()
    .with_target(false)
    .compact()
    .init();

  let app = Router::new().route("/", post(root)).with_state(42).layer(
    TraceLayer::new_for_http()
      .make_span_with(trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
      .on_response(trace::DefaultOnResponse::new().level(tracing::Level::INFO)),
  );

  // run our app with hyper
  // `axum::Server` is a re-export of `hyper::Server`
  tokio::spawn((async move || {
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let _ = axum::Server::bind(&addr)
      .serve(app.into_make_service())
      .with_graceful_shutdown(shutdown_signal())
      .await;
  })());

  mapvas::MapVas::new().await;
}
