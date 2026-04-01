//! Headless map renderer using `egui_kittest` Harness.
//!
//! Renders map tiles + geometries to an image without spawning a window.
//! Reuses the existing `MapApp` rendering pipeline via the `egui_kittest` Harness.

use eframe::App;
use egui_kittest::Harness;

use crate::{
  config::Config,
  map::{map_event::MapEvent, mapvas_egui::Map},
  mapvas_ui::MapApp,
};

/// Headless map renderer - no window needed.
///
/// Uses the `egui_kittest` Harness to drive the full `MapApp` rendering
/// pipeline headlessly, producing an `image::RgbaImage`.
pub struct HeadlessRenderer {
  config: Config,
  wait_for_tiles: bool,
}

impl HeadlessRenderer {
  /// Create a new headless renderer with default config.
  #[must_use]
  pub fn new() -> Self {
    Self {
      config: Config::new(),
      wait_for_tiles: true,
    }
  }

  /// Create a new headless renderer with a specific config.
  #[must_use]
  pub fn with_config(config: Config) -> Self {
    Self {
      config,
      wait_for_tiles: true,
    }
  }

  #[must_use]
  pub fn without_tiles(mut self) -> Self {
    self.wait_for_tiles = false;
    self
  }

  /// Render map with the given events to an image.
  ///
  /// Loads geometries from the provided `MapEvent`s, focuses the view,
  /// and waits for tiles to finish loading before producing the final image.
  /// Tile completion is detected via egui's repaint mechanism: tile loading
  /// triggers `request_repaint()`, so the harness keeps running until no
  /// more immediate repaints are requested.
  ///
  /// # Arguments
  /// * `events` - Map events to load (geometries, focus, etc.)
  /// * `width` - Image width in pixels
  /// * `height` - Image height in pixels
  #[must_use]
  pub fn render(&self, events: &[MapEvent], width: u32, height: u32) -> image::RgbaImage {
    let ctx = egui::Context::default();
    let (mut map, remote, data_holder) = Map::new(ctx);
    map.set_headless();

    for event in events {
      remote.handle_map_event(event.clone());
    }

    let mut app = MapApp::new(map, remote, data_holder, self.config.clone(), None);
    app.set_headless();

    #[allow(clippy::cast_precision_loss)]
    let mut harness = Harness::builder()
      .with_size(egui::vec2(width as f32, height as f32))
      // Time between frames — gives async tile downloads time to complete
      .with_step_dt(0.1)
      // Upper bound on frames to prevent infinite loops
      .with_max_steps(200)
      .build_ui_state(
        |ui, app: &mut MapApp| {
          let mut frame = eframe::Frame::_new_kittest();
          app.ui(ui, &mut frame);
        },
        app,
      );

    // Hide the sidebar
    harness.key_press(egui::Key::F1);
    harness.step();

    // Run frames until all tiles are loaded. Each frame:
    // 1. Steps the harness (runs one egui frame)
    // 2. Sleeps step_dt to let async tile downloads progress
    // 3. Checks if any layer still has pending work (tiles in flight)
    // We keep going as long as tiles are still downloading/rendering,
    // even if egui isn't requesting immediate repaints.
    if self.wait_for_tiles {
      let step_dt = std::time::Duration::from_secs_f32(0.1);
      let max_steps = 600; // 60 seconds max
      for _ in 0..max_steps {
        harness.step();
        std::thread::sleep(step_dt);
        if !harness.state().has_pending_work() {
          break;
        }
      }
      // One final run to render any last tiles that arrived
      let _ = harness.try_run_realtime();
    }

    harness.render().expect("Failed to render headless image")
  }
}

impl Default for HeadlessRenderer {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;

  use crate::parser::{FileParser, GeoJsonParser};

  use super::*;

  #[tokio::test]
  async fn test_headless_render_empty() {
    let renderer = HeadlessRenderer::new().without_tiles();
    let img = renderer.render(&[], 800, 600);
    assert_eq!(img.width(), 800);
    assert_eq!(img.height(), 600);
  }

  #[tokio::test]
  async fn test_headless_render_with_geojson() {
    let geojson = r#"{
      "type": "FeatureCollection",
      "features": [
        {
          "type": "Feature",
          "geometry": {
            "type": "Point",
            "coordinates": [13.4, 52.5]
          },
          "properties": {}
        },
        {
          "type": "Feature",
          "geometry": {
            "type": "LineString",
            "coordinates": [[13.3, 52.4], [13.5, 52.6]]
          },
          "properties": {}
        }
      ]
    }"#;

    let mut parser = GeoJsonParser::default();
    let cursor = Cursor::new(geojson.as_bytes().to_vec());
    let mut events: Vec<MapEvent> = parser.parse(Box::new(cursor)).collect();
    events.push(MapEvent::Focus);

    let renderer = HeadlessRenderer::new().without_tiles();
    let img = renderer.render(&events, 1600, 1200);
    assert_eq!(img.width(), 1600);
    assert_eq!(img.height(), 1200);

    // Verify it's not entirely blank (should have some non-white pixels from UI)
    let has_content = img.pixels().any(|p| p.0 != [255, 255, 255, 255]);
    assert!(has_content, "Rendered image should not be entirely white");
  }
}
