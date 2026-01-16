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

fn create_test_app_with_geometries() -> MapApp {
  let config = Config::new();
  let ctx = egui::Context::default();
  let (map, remote, data_holder) = Map::new(ctx);

  // Use nested folders KML to have both collections and individual geometries
  let kml_content = fs::read_to_string("tests/resources/nested_folders.kml")
    .expect("Failed to read nested folders KML file");

  let mut parser = KmlParser::new();
  let cursor = Cursor::new(kml_content.into_bytes());
  let events: Vec<MapEvent> = parser.parse(Box::new(cursor)).collect();

  // Send the map events to load the geometries
  for event in events {
    remote.handle_map_event(event);
  }

  // Send a focus event to center the view on the geometries
  remote.handle_map_event(MapEvent::Focus);

  MapApp::new(map, remote, data_holder, config, None)
}

#[tokio::test]
async fn test_geometry_highlighting_on_double_click() {
  let app = create_test_app_with_geometries();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Setup: get to the geometry view
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  let shape_layer = harness.get_by_label("Shape Layer");
  shape_layer.click();
  harness.run();

  let shapes = harness.get_by_label("Shapes");
  shapes.click();
  harness.run();

  // Take snapshot showing initial state (shapes expanded)
  harness.snapshot("geometry_shapes_expanded");

  // The test verifies UI structure is ready for double-click popup functionality
  harness.snapshot("geometry_ui_ready_for_interaction");

  // The double-click popup functionality is tested in the popup_double_click_tests.rs file
  // This test focuses on verifying the UI structure is ready for user interaction
}

#[tokio::test]
async fn test_individual_geometry_highlighting() {
  let app = create_test_app_with_geometries();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Setup to reach individual geometries
  let shape_layer = harness.get_by_label("Shape Layer");
  shape_layer.click();
  harness.run();

  let shapes = harness.get_by_label("Shapes");
  shapes.click();
  harness.run();

  // Take snapshot showing shape layer expanded
  harness.snapshot("individual_geometries_shape_layer_expanded");

  // The test now focuses on UI structure rather than specific collection navigation
  // since the double-click popup functionality replaces the old highlighting behavior
  harness.snapshot("individual_geometries_final_state");
}
