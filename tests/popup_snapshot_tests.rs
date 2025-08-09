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

fn create_test_app_with_popup_kml() -> MapApp {
  let config = Config::new();
  let ctx = egui::Context::default();
  let (map, remote, data_holder) = Map::new(ctx);

  // Read and parse the test KML file with MultiGeometry
  let kml_content = fs::read_to_string("tests/resources/popup_test.kml")
    .expect("Failed to read popup test KML file");

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
async fn test_popup_initial_state() {
  let app = create_test_app_with_popup_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Switch to grid mode to avoid tile loading issues
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Take initial screenshot showing the sidebar with collections
  harness.snapshot("popup_initial_sidebar");
}

#[tokio::test]
async fn test_multigeometry_popup_functionality() {
  let app = create_test_app_with_popup_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Switch to grid mode to avoid tile loading issues
  let tile_layer = harness.get_by_label("Tile Layer");
  tile_layer.click();
  harness.run();

  let grid_only_button = harness.get_by_label("Grid Only");
  grid_only_button.click();
  harness.run();

  // Take screenshot showing the sidebar with all collections
  harness.snapshot("popup_sidebar_with_collections");

  // Look for Shape Layer to expand it
  harness.get_by_label("Shape Layer").click();
  harness.run();

  // Take screenshot showing expanded shape layer
  harness.snapshot("popup_shape_layer_expanded");
}
