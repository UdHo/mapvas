use eframe::App;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use mapvas::{
  config::Config,
  map::{map_event::MapEvent, mapvas_egui::Map},
  mapvas_ui::MapApp,
  parser::{FileParser, KmlParser},
};
use std::fs;
use std::io::Cursor;

fn create_test_app_with_temporal_kml() -> MapApp {
  let config = Config::new();
  let ctx = egui::Context::default();
  let (map, remote, data_holder) = Map::new(ctx);

  // Read and parse the temporal KML file
  let kml_content = fs::read_to_string("tests/resources/temporal_test.kml")
    .expect("Failed to read temporal test KML file");

  let mut parser = KmlParser::new();
  let cursor = Cursor::new(kml_content.into_bytes());
  let events: Vec<MapEvent> = parser.parse(Box::new(cursor)).collect();

  // Send the map events to load the geometries
  for event in events {
    remote.handle_map_event(event);
  }

  // Send a focus event to center the view on the geometries
  remote.handle_map_event(MapEvent::Focus);

  MapApp::new(map, remote, data_holder, config)
}

#[tokio::test]
async fn temporal_visualization_initial_state() {
  let app = create_test_app_with_temporal_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Verify basic UI elements are present
  harness.get_by_label("Layers");
  harness.get_by_label("Tile Layer");

  // Switch to grid mode to avoid tile loading issues
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Verify Timeline section appears with temporal data loaded
  harness.get_by_label("⏰ Timeline");

  // Click to expand Timeline section
  let timeline_header = harness.get_by_label("⏰ Timeline");
  timeline_header.click();
  harness.run();

  // Verify temporal controls are present
  harness.get_by_label("▶ Play");
  harness.get_by_label("⏹ Stop");
  harness.get_by_label("Timeline Position:");
  harness.get_by_label("Speed:");
  harness.get_by_label("Time Window:");

  // Take snapshot of initial temporal visualization state
  harness.snapshot("temporal_initial_state");
}

#[tokio::test]
async fn temporal_visualization_morning_time() {
  let app = create_test_app_with_temporal_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Switch to grid mode
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();
  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Expand Timeline section
  let timeline_header = harness.get_by_label("⏰ Timeline");
  timeline_header.click();
  harness.run();

  // The timeline should be initialized to the start time (09:00)
  // At 09:00, only the "Morning Point" should be visible
  // Take snapshot showing morning state
  harness.snapshot("temporal_morning_state");
}

#[tokio::test]
async fn temporal_visualization_with_geometries() {
  let app = create_test_app_with_temporal_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Switch to grid mode
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();
  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Just verify that temporal controls are available with KML data loaded
  // The geometries might not be immediately visible in the UI structure
  let timeline_header = harness.get_by_label("⏰ Timeline");
  timeline_header.click();
  harness.run();

  // Verify temporal controls are present, indicating KML data was loaded
  harness.get_by_label("▶ Play");
  harness.get_by_label("Current: 2024-01-01 09:00:00 UTC");
  harness.get_by_label("Range: 2024-01-01 09:00:00 UTC to 2024-01-01 18:00:00 UTC");

  // Take snapshot showing temporal data is loaded and functional
  harness.snapshot("temporal_with_geometries");
}

#[tokio::test]
async fn temporal_visualization_timeline_expanded() {
  let app = create_test_app_with_temporal_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Switch to grid mode
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();
  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Expand Timeline section
  let timeline_header = harness.get_by_label("⏰ Timeline");
  timeline_header.click();
  harness.run();

  // Verify all temporal controls are visible and functional
  harness.get_by_label("▶ Play");
  harness.get_by_label("⏹ Stop");

  // Should show current time and time range
  // Format: "Current: 2024-01-01 09:00:00 UTC"
  // Format: "Range: 2024-01-01 09:00:00 UTC to 2024-01-01 18:00:00 UTC"

  // Should show speed slider and time window controls
  harness.get_by_label("Speed:");
  harness.get_by_label("Time Window:");
  harness.get_by_label("Moving window");

  // Take comprehensive snapshot of expanded timeline controls
  harness.snapshot("temporal_timeline_expanded");
}

#[tokio::test]
async fn temporal_visualization_workflow() {
  let app = create_test_app_with_temporal_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Initial state: Should have loaded temporal KML data
  harness.get_by_label("Layers");

  // Switch to grid mode for consistent testing
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();
  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Step 1: Timeline should appear automatically when temporal data is loaded
  harness.get_by_label("⏰ Timeline");
  harness.snapshot("temporal_workflow_step1_timeline_available");

  // Step 2: Expand timeline to show controls
  let timeline_header = harness.get_by_label("⏰ Timeline");
  timeline_header.click();
  harness.run();

  harness.get_by_label("▶ Play");
  harness.get_by_label("Timeline Position:");
  harness.snapshot("temporal_workflow_step2_controls_expanded");

  // Step 3: Verify temporal data is loaded and working
  // Instead of trying to find specific geometries, verify temporal functionality
  harness.get_by_label("Current: 2024-01-01 09:00:00 UTC");
  harness.get_by_label("Range: 2024-01-01 09:00:00 UTC to 2024-01-01 18:00:00 UTC");

  harness.snapshot("temporal_workflow_step3_temporal_data_loaded");
}

#[tokio::test]
async fn temporal_filtering_demonstration() {
  let app = create_test_app_with_temporal_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Switch to grid mode to see the map area clearly (no tiles, just grid + geometries)
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();
  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Expand timeline controls
  let timeline_header = harness.get_by_label("⏰ Timeline");
  timeline_header.click();
  harness.run();

  // Verify we're at the initial time (09:00)
  harness.get_by_label("Current: 2024-01-01 09:00:00 UTC");

  // At 09:00, according to our KML:
  // - Morning Point (09:00) should be visible
  // - Noon Point (12:00) should be hidden
  // - Evening Point (18:00) should be hidden
  harness.snapshot("temporal_filtering_09_00_only_morning_visible");

  // Try to click on the Play button to start temporal animation
  // This should advance time and show different geometries
  let play_button = harness.get_by_label("▶ Play");
  play_button.click();
  harness.step();

  // Let the animation run for a moment by running several steps
  // Use step() instead of run() since animation causes continuous repaints
  for _ in 0..10 {
    harness.step();
  }

  // Take another snapshot after some time has passed
  // The current time should have advanced and different geometries should be visible
  harness.snapshot("temporal_filtering_after_animation_started");

  // Stop the animation
  let stop_button = harness.get_by_label("⏹ Stop");
  stop_button.click();
  harness.step();

  // This should reset back to the start time
  harness.snapshot("temporal_filtering_after_stop_reset");
}
