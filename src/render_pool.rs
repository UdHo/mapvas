use std::sync::LazyLock;

/// Global thread pool for CPU-bound tile rendering
/// Separate from tokio's thread pool to avoid blocking async I/O
pub static RENDER_POOL: LazyLock<rayon::ThreadPool> = LazyLock::new(|| {
  rayon::ThreadPoolBuilder::new()
    // num_threads not specified - uses all available CPU cores by default
    .thread_name(|i| format!("render-{i}"))
    .build()
    .expect("Failed to create rayon render pool")
});
