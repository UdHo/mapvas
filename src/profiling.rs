/// Profiling utilities using puffin
#[cfg(feature = "profiling")]
pub use puffin;

/// Initialize profiling if enabled
pub fn init_profiling() {
  #[cfg(feature = "profiling")]
  {
    puffin::set_scopes_on(true);

    // Start the puffin server for external viewing
    let server_addr = "127.0.0.1:8585";
    match puffin_http::Server::new(server_addr) {
      Ok(puffin_server) => {
        log::info!("Puffin profiling server started at http://{server_addr}");
        log::info!("Run `puffin_viewer` or open the URL in a browser to view profiles");
        log::info!("You can also install puffin_viewer with: cargo install puffin_viewer");

        // Try to launch puffin_viewer if available
        std::process::Command::new("puffin_viewer")
          .arg("--url")
          .arg(server_addr)
          .spawn()
          .ok(); // Don't fail if puffin_viewer isn't installed

        // Keep the server alive by moving it to a static
        std::mem::forget(puffin_server);
      }
      Err(e) => {
        log::warn!("Failed to start puffin server: {e}");
      }
    }
  }

  #[cfg(not(feature = "profiling"))]
  {
    log::debug!("Profiling disabled - compile with --features profiling to enable");
  }
}

/// Convenience macro for profiling scopes when profiling is enabled
#[macro_export]
macro_rules! profile_scope {
  ($name:expr) => {
    #[cfg(feature = "profiling")]
    $crate::profiling::puffin::profile_scope!($name);
  };
  ($name:expr, $data:expr) => {
    #[cfg(feature = "profiling")]
    $crate::profiling::puffin::profile_scope!($name, $data);
  };
}

/// Function-level profiling macro
#[macro_export]
macro_rules! profile_function {
  () => {
    profile_scope!(concat!(module_path!(), "::", function_name!()));
  };
}

/// Frame marker for profiling
pub fn new_frame() {
  #[cfg(feature = "profiling")]
  puffin::GlobalProfiler::lock().new_frame();
}

/// Check if profiling is enabled
#[must_use]
pub const fn is_profiling_enabled() -> bool {
  cfg!(feature = "profiling")
}
