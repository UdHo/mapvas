use eframe::App;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use mapvas::{config::Config, map::mapvas_egui::Map, mapvas_ui::MapApp};

fn create_test_app() -> MapApp {
  let config = Config::new();
  let ctx = egui::Context::default();
  let (map, remote, data_holder) = Map::new(ctx);
  MapApp::new(map, remote, data_holder, config)
}

#[tokio::test]
async fn sidebar_toggle_functionality() {
  let app = create_test_app();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  // Initially, sidebar should be visible
  harness.run();

  // Look for sidebar elements - should find layers heading
  harness.get_by_label("Layers");

  // For now, we can't easily test key presses since the API is different
  // Just verify the UI structure is correct
  harness.get_by_label("Map Layers");
  harness.get_by_label("Tile Layer");
}

#[tokio::test]
async fn sidebar_close_button_functionality() {
  let app = create_test_app();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Test that basic UI elements are present
  harness.get_by_label("Layers");
}

#[tokio::test]
async fn map_layers_display() {
  let app = create_test_app();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Should find the Map Layers collapsing header
  harness.get_by_label("Map Layers");

  // Should find the Tile Layer
  harness.get_by_label("Tile Layer");
}

#[tokio::test]
async fn location_search_section() {
  let app = create_test_app();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Should find the Location Search section
  harness.get_by_label("🔍Location Search");
}

#[tokio::test]
async fn tile_layer_settings() {
  let app = create_test_app();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Find and click on tile layer to expand its settings
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();

  harness.run();

  // Should now see tile provider combo box (specifically the combo box, not the label)
  let combo_boxes: Vec<_> = harness.get_all_by_role(Role::ComboBox).collect();
  // Just verify we have at least one combo box after expanding
  assert!(!combo_boxes.is_empty());
}

#[tokio::test]
async fn empty_shape_layer() {
  let app = create_test_app();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Should have a shapes section (even if empty)
  // The shapes section is in the shape layer UI
  // For now, just verify the basic UI structure is there

  harness.get_by_label("Layers");
}

#[tokio::test]
async fn settings_dialog_button() {
  let app = create_test_app();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Should find the Settings button
  harness.get_by_label("Settings");
}

#[cfg(test)]
mod pagination_tests {
  use super::*;

  // Note: Testing pagination would require creating a map with many geometries
  // This would need a more complex setup to inject test data
  #[tokio::test]
  async fn pagination_with_large_dataset() {
    // This test would require injecting geometries into the shape layer
    // For now, just test the basic UI without data
    let app = create_test_app();

    let mut harness = Harness::new_state(
      |ctx, app: &mut MapApp| {
        let mut frame = eframe::Frame::_new_kittest();
        app.update(ctx, &mut frame);
      },
      app,
    );

    harness.run();

    // Basic verification that UI renders without crashing
    harness.get_by_label("Layers");
  }
}

// Integration test to verify the entire UI renders correctly
#[tokio::test]
async fn full_ui_integration_test() {
  let app = create_test_app();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Verify all major UI sections are present
  harness.get_by_label("Layers");
  harness.get_by_label("🔍Location Search");
  harness.get_by_label("Map Layers");
  harness.get_by_label("Tile Layer");
  harness.get_by_label("Settings");

  // Test clicking on tile layer
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();

  harness.run();

  // After clicking, should be able to find combo boxes
  let combo_boxes: Vec<_> = harness.get_all_by_role(Role::ComboBox).collect();
  // We should have at least 2 combo boxes (tile provider and tile source)
  assert!(combo_boxes.len() >= 2);
}
