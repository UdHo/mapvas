use mapvas::map::map_event::MapEvent;
use mapvas::parser::AutoFileParser;
use rstest::rstest;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_filename_becomes_layer_name() {
  let content =
    r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [10.0, 52.0]}}"#;
  let filename = "my_test_file.geojson";
  fs::write(filename, content).unwrap();

  let mut parser = AutoFileParser::new(&PathBuf::from(filename));
  let events: Vec<_> = parser.parse().collect();

  assert!(!events.is_empty());
  if let Some(MapEvent::Layer(layer)) = events.first() {
    assert_eq!(layer.id, "my_test_file");
    assert_eq!(layer.geometries.len(), 1);
  } else {
    panic!("Expected Layer event");
  }

  fs::remove_file(filename).unwrap();
}

const GEOJSON_CONTENT: &str =
  r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [10.0, 52.0]}}"#;
const KML_CONTENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Placemark>
    <Point>
      <coordinates>10.0,52.0,0</coordinates>
    </Point>
  </Placemark>
</kml>"#;
const GPX_CONTENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test">
  <wpt lat="52.0" lon="10.0">
    <name>Test Point</name>
  </wpt>
</gpx>"#;

#[rstest]
#[case("test.geojson", "test", GEOJSON_CONTENT)]
#[case("my-data.kml", "my-data", KML_CONTENT)]
#[case("coordinates.gpx", "coordinates", GPX_CONTENT)]
#[case("data.json", "data", GEOJSON_CONTENT)]
fn test_file_extension_layer_name(
  #[case] filename: &str,
  #[case] expected_layer_name: &str,
  #[case] content: &str,
) {
  fs::write(filename, content).unwrap();

  let mut parser = AutoFileParser::new(&PathBuf::from(filename));
  let events: Vec<_> = parser.parse().collect();

  assert!(!events.is_empty());
  if let Some(MapEvent::Layer(layer)) = events.first() {
    assert_eq!(layer.id, expected_layer_name);
  } else {
    panic!("Expected Layer event");
  }

  fs::remove_file(filename).unwrap();
}
