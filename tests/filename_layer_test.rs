use mapvas::parser::AutoFileParser;
use mapvas::map::map_event::MapEvent;
use std::path::PathBuf;
use std::fs;

#[test]
fn test_filename_becomes_layer_name() {
    // Create a test GeoJSON file
    let content = r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [10.0, 52.0]}}"#;
    let filename = "my_test_file.geojson";
    fs::write(filename, content).expect("Failed to write test file");
    
    // Parse it with AutoFileParser
    let mut parser = AutoFileParser::new(PathBuf::from(filename));
    let events: Vec<_> = parser.parse().collect();
    
    // Check that we get events
    assert!(!events.is_empty(), "Should get at least one event");
    
    // Check the layer name
    if let Some(MapEvent::Layer(layer)) = events.first() {
        assert_eq!(layer.id, "my_test_file", "Layer name should be the filename without extension");
        assert_eq!(layer.geometries.len(), 1, "Should have one geometry");
    } else {
        panic!("Expected first event to be a Layer event");
    }
    
    // Clean up
    fs::remove_file(filename).expect("Failed to remove test file");
}

#[test]
fn test_different_file_extensions() {
    let test_cases = vec![
        ("test.geojson", "test"),
        ("my-data.kml", "my-data"),
        ("coordinates.gpx", "coordinates"),
        ("data.json", "data"),
    ];
    
    for (filename, expected_layer_name) in test_cases {
        let content = match filename.split('.').last().unwrap() {
            "geojson" | "json" => r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [10.0, 52.0]}}"#,
            "kml" => r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Placemark>
    <Point>
      <coordinates>10.0,52.0,0</coordinates>
    </Point>
  </Placemark>
</kml>"#,
            "gpx" => r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test">
  <wpt lat="52.0" lon="10.0">
    <name>Test Point</name>
  </wpt>
</gpx>"#,
            _ => "52.0, 10.0",
        };
        
        fs::write(filename, content).expect("Failed to write test file");
        
        let mut parser = AutoFileParser::new(PathBuf::from(filename));
        let events: Vec<_> = parser.parse().collect();
        
        assert!(!events.is_empty(), "Should get at least one event for {}", filename);
        
        if let Some(MapEvent::Layer(layer)) = events.first() {
            assert_eq!(
                layer.id, expected_layer_name,
                "Layer name should be '{}' for file '{}'", expected_layer_name, filename
            );
        } else {
            panic!("Expected first event to be a Layer event for {}", filename);
        }
        
        fs::remove_file(filename).expect("Failed to remove test file");
    }
}