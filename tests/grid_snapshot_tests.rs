use eframe::App;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use mapvas::{
  config::Config,
  map::{map_event::MapEvent, mapvas_egui::Map},
  mapvas_ui::MapApp,
  parser::{FileParser, GeoJsonParser},
};
use std::fs;
use std::io::Cursor;

fn create_test_app_with_geojson_grid_mode(geojson_content: String) -> MapApp {
  let config = Config::new();
  let ctx = egui::Context::default();
  let (map, remote, data_holder) = Map::new(ctx);

  // Parse the GeoJSON content and inject it as a map event
  let mut parser = GeoJsonParser::default();
  let cursor = Cursor::new(geojson_content.into_bytes());
  let events: Vec<MapEvent> = parser.parse(Box::new(cursor)).collect();

  // Send the map events to load the geometries
  for event in events {
    remote.handle_map_event(event);
  }

  // Send a focus event to center the view on the geometries
  remote.handle_map_event(MapEvent::Focus);

  MapApp::new(map, remote, data_holder, config)
}

fn create_test_app_grid_mode() -> MapApp {
  let config = Config::new();
  let ctx = egui::Context::default();
  let (map, remote, data_holder) = Map::new(ctx);
  MapApp::new(map, remote, data_holder, config)
}

#[tokio::test]
async fn grid_mode_basic_functionality() {
  let app = create_test_app_grid_mode();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Verify initial UI elements are present
  harness.get_by_label("Layers");
  harness.get_by_label("Tile Layer");

  // Immediately switch to grid mode to avoid tile loading
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  // Verify coordinate display options are available
  harness.get_by_label("Off");
  harness.get_by_label("Overlay");
  harness.get_by_label("Grid Only");

  // Click on "Grid Only" radio button to switch to grid mode (avoiding tiles)
  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Take snapshot: Pure grid mode without any tile dependencies
  harness.snapshot("grid_only_mode_basic");

  // Verify the UI is still functional after switching to grid mode
  harness.get_by_label("Layers");
  harness.get_by_label("Map Layers");
}

#[tokio::test]
async fn grid_mode_with_geojson_data() {
  let geojson_content = fs::read_to_string("tests/resources/feature_collection.geojson")
    .expect("Failed to read test GeoJSON file");

  let app = create_test_app_with_geojson_grid_mode(geojson_content);

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  // Run initial frame to load the geometries
  harness.run();

  // Immediately switch to grid mode to avoid tile loading delays
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Take snapshot: GeoJSON geometries displayed on coordinate grid
  harness.snapshot("geojson_on_grid_mode");

  // Verify the UI is functional with loaded geometries in grid mode
  harness.get_by_label("Layers");
  harness.get_by_label("Map Layers");
  harness.get_by_label("🔍Location Search");

  // Collapse the tile layer settings for cleaner view
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  // Take snapshot: Clean grid mode view with geometries
  harness.snapshot("geojson_grid_clean_view");

  harness.get_by_label("Layers");
}

#[tokio::test]
async fn grid_mode_coordinate_display_comparison() {
  let geojson_content = fs::read_to_string("tests/resources/color_formats.geojson")
    .expect("Failed to read test GeoJSON file");

  let app = create_test_app_with_geojson_grid_mode(geojson_content);

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Open tile layer settings
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  // Test coordinate display mode: Off (no coordinates, no tiles in test)
  let off_button = harness.get_by_label("Off");
  off_button.click();
  harness.run();
  harness.snapshot("coordinates_off_mode");

  // Test coordinate display mode: Grid Only (the main focus)
  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();
  harness.snapshot("coordinates_grid_only_mode");

  // Verify UI still works
  harness.get_by_label("Layers");
}

#[tokio::test]
async fn grid_mode_empty_state() {
  // Test grid mode with no geometries loaded - pure grid visualization
  let app = create_test_app_grid_mode();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Immediately switch to grid mode
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Take snapshot: Pure coordinate grid without any data
  harness.snapshot("empty_grid_mode");

  // Collapse settings for cleaner view
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  harness.snapshot("empty_grid_mode_clean");

  // Verify all UI elements are functional
  harness.get_by_label("Layers");
  harness.get_by_label("🔍Location Search");
  harness.get_by_label("Settings");
}

#[tokio::test]
async fn grid_mode_ui_workflow() {
  // Test the complete workflow focusing on grid mode UI interactions
  let geojson_content = fs::read_to_string("tests/resources/styled_feature.geojson")
    .expect("Failed to read test GeoJSON file");

  let app = create_test_app_with_geojson_grid_mode(geojson_content);

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  // Step 1: Initial state with loaded data
  harness.run();
  harness.snapshot("workflow_initial_with_data");

  // Step 2: Open tile layer settings
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();
  harness.snapshot("workflow_settings_opened");

  // Step 3: Switch directly to grid mode (avoiding tile dependencies)
  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();
  harness.snapshot("workflow_grid_mode_active");

  // Step 4: Collapse settings for final view
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();
  harness.snapshot("workflow_final_grid_view");

  // Verify all UI elements remain functional
  harness.get_by_label("Layers");
  harness.get_by_label("🔍Location Search");
  harness.get_by_label("Settings");
}

#[tokio::test]
async fn grid_mode_sidebar_interactions() {
  // Test grid mode with various sidebar interactions
  let app = create_test_app_grid_mode();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Switch to grid mode immediately
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Test that all sidebar sections work in grid mode
  harness.get_by_label("Map Layers");
  harness.get_by_label("🔍Location Search");

  // Take snapshot showing sidebar functionality in grid mode
  harness.snapshot("grid_mode_sidebar_full");

  // Collapse tile layer
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  harness.snapshot("grid_mode_sidebar_collapsed");

  // Verify search functionality is available
  harness.get_by_label("🔍Location Search");
}
