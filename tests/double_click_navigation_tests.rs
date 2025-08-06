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

fn create_test_app_with_nested_kml() -> MapApp {
  let config = Config::new();
  let ctx = egui::Context::default();
  let (map, remote, data_holder) = Map::new(ctx);

  // Use the existing popup test KML that has nested collections
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

#[test]
fn test_nested_path_collection() {
  use mapvas::map::{
    coordinates::PixelCoordinate,
    geometry_collection::{Geometry, Metadata},
    mapvas_egui::layer::ShapeLayer,
  };

  // Create a nested geometry structure similar to our KML:
  // Top-level GeometryCollection -> Intermediate Collection -> MultiGeometry with LineStrings
  let inner_geometries = vec![
    Geometry::LineString(
      vec![
        PixelCoordinate::new(100.0, 100.0),
        PixelCoordinate::new(200.0, 200.0),
      ],
      Metadata::default(),
    ),
    Geometry::LineString(
      vec![
        PixelCoordinate::new(150.0, 150.0),
        PixelCoordinate::new(250.0, 250.0),
      ],
      Metadata::default(),
    ),
  ];

  let intermediate_geometry = Geometry::GeometryCollection(inner_geometries, Metadata::default());
  let top_level_geometry =
    Geometry::GeometryCollection(vec![intermediate_geometry], Metadata::default());

  // Test the collect_nested_paths function
  let mut paths = Vec::new();
  ShapeLayer::collect_nested_paths(&top_level_geometry, &mut Vec::new(), &mut paths);

  // Should find two nested collection paths: [0] for intermediate, and none for the top level
  assert!(!paths.is_empty(), "Should find nested collection paths");
  println!("Found paths: {:?}", paths);
}

#[tokio::test]
async fn test_nested_collection_structure() {
  let app = create_test_app_with_nested_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Setup: get to the nested structure
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

  let kml_collection = harness.get_by_label("📁 kml (1)");
  kml_collection.click();
  harness.run();

  // Take snapshot showing nested collection structure is ready for double-click functionality
  harness.snapshot("nested_collection_ready_for_double_click");

  // Manually expand the main collection to simulate what double-click should do
  let main_collection = harness.get_by_label("📁 Collection (1 items)");
  main_collection.click();
  harness.run();

  // The nested structure should now be visible - this is what double-click should achieve automatically
  harness.snapshot("nested_collection_manually_expanded");
}

#[tokio::test]
async fn test_expand_all_context_menu() {
  let app = create_test_app_with_nested_kml();

  let mut harness = Harness::new_state(
    |ctx, app: &mut MapApp| {
      let mut frame = eframe::Frame::_new_kittest();
      app.update(ctx, &mut frame);
    },
    app,
  );

  harness.run();

  // Setup to get to the nested collections
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

  let kml_collection = harness.get_by_label("📁 kml (1)");
  kml_collection.click();
  harness.run();

  // Take snapshot showing initial state
  harness.snapshot("expand_all_initial_state");

  // Note: In a real test, you would right-click on the collection to access the context menu
  // and then click "📂 Expand All Nested". Since egui_kittest doesn't support right-click,
  // this test documents the expected behavior.

  // The context menu should contain:
  // - Hide/Show Geometry
  // - 📂 Expand All Nested (for collections only)
  // - 🗑 Delete Geometry
}
