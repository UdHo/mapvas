use std::sync::LazyLock;

/// Global thread pool for CPU-bound tile rendering.
/// Separate from tokio's thread pool to avoid blocking async I/O.
pub static RENDER_POOL: LazyLock<rayon::ThreadPool> = LazyLock::new(|| {
  rayon::ThreadPoolBuilder::new()
    // num_threads not specified - uses all available CPU cores by default
    .thread_name(|i| format!("render-{i}"))
    .build()
    .expect("Failed to create rayon render pool")
});

/// Submit a CPU-bound task to the render pool and return a future that resolves to its result.
///
/// If the awaiting task is dropped (e.g. timeout), the rayon task still runs to completion
/// but its result is silently discarded.
pub fn submit<F, R>(f: F) -> tokio::sync::oneshot::Receiver<R>
where
  F: FnOnce() -> R + Send + 'static,
  R: Send + 'static,
{
  let (tx, rx) = tokio::sync::oneshot::channel();
  RENDER_POOL.spawn(move || {
    let _ = tx.send(f());
  });
  rx
}
