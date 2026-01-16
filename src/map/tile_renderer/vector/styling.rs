use std::sync::LazyLock;
use tiny_skia::Color;

// Helper function to create colors (since from_rgba8 is not const)
pub fn color(r: u8, g: u8, b: u8) -> Color {
  Color::from_rgba8(r, g, b, 255)
}

// Color constants for OSM-style rendering
pub static BACKGROUND_COLOR: LazyLock<Color> = LazyLock::new(|| color(242, 239, 233)); // Light cream
pub static WATER_COLOR: LazyLock<Color> = LazyLock::new(|| color(170, 211, 223)); // Blue
pub static LAND_COLOR: LazyLock<Color> = LazyLock::new(|| color(232, 227, 216)); // Slightly darker cream
pub static PARK_COLOR: LazyLock<Color> = LazyLock::new(|| color(200, 230, 180)); // Green
pub static BUILDING_COLOR: LazyLock<Color> = LazyLock::new(|| color(200, 190, 180)); // Tan/brown
pub static ROAD_COLOR: LazyLock<Color> = LazyLock::new(|| color(255, 255, 255)); // White
pub static ROAD_CASING_COLOR: LazyLock<Color> = LazyLock::new(|| color(180, 180, 180)); // Gray

/// Get fill color for a polygon feature based on layer and class
pub fn get_fill_color(layer_name: &str, feature_class: &str) -> Option<Color> {
  // Check feature class first for specific types
  match feature_class {
    // Water features
    "water" | "ocean" | "bay" | "lake" | "river" | "stream" => Some(*WATER_COLOR),
    // Green/park features
    "grass" | "forest" | "wood" | "park" | "garden" | "meadow" | "scrub" => Some(*PARK_COLOR),
    // Buildings
    "building" => Some(*BUILDING_COLOR),
    // Default to layer-based coloring
    _ => match layer_name {
      "water" | "waterway" => Some(*WATER_COLOR),
      "landcover" | "landuse" => Some(*LAND_COLOR),
      "park" | "green" => Some(*PARK_COLOR),
      "building" => Some(*BUILDING_COLOR),
      _ => None,
    },
  }
}

/// Returns (`casing_color`, `casing_width`, `inner_color`, `inner_width`) for roads
#[allow(clippy::match_same_arms)]
pub fn get_road_styling(
  layer_name: &str,
  feature_class: &str,
  tile_zoom: u8,
) -> (Color, f32, Color, f32) {
  // Handle water features
  if layer_name == "water" || layer_name == "waterway" {
    return (*WATER_COLOR, 0.0, *WATER_COLOR, 1.5);
  }

  // Calculate zoom-based width multiplier
  // At low zoom (3-8), roads should be MUCH thinner
  // At medium zoom (9-12), roads gradually increase
  // At high zoom (14+), roads are normal width
  let zoom_scale = match tile_zoom {
    0..=5 => 0.08, // Very zoomed out (continents) - extremely thin
    6 => 0.12,
    7 => 0.16,
    8 => 0.22,
    9 => 0.3,
    10 => 0.4,
    11 => 0.55,
    12 => 0.7,
    13 => 0.85,
    14 => 1.0,   // Normal width (street level)
    15.. => 1.0, // Keep at normal width
  };

  // Handle transportation/roads with more pronounced width and color differences
  let (casing_color, base_casing_width, inner_color, base_inner_width) = match feature_class {
    // Motorways/highways - widest, bright red (OSM style)
    "motorway" | "motorway_link" => (
      color(160, 20, 20), // Dark red casing
      10.0,               // Very wide casing for highways
      color(235, 75, 65), // Bright red inner
      7.0,                // Wide inner
    ),

    // Trunk roads - very wide, orange
    "trunk" | "trunk_link" => (
      color(170, 85, 20),  // Dark orange casing
      8.0,                 // Very wide
      color(255, 150, 90), // Bright orange inner
      5.5,
    ),

    // Primary roads - wide, yellow-orange
    "primary" | "primary_link" => (
      color(150, 110, 60),  // Brown-orange casing
      6.5,                  // Wide
      color(255, 200, 100), // Golden/tan inner
      4.5,
    ),

    // Secondary roads - medium width, yellow
    "secondary" | "secondary_link" => (
      color(140, 140, 80),  // Olive casing
      5.0,                  // Medium width
      color(255, 240, 150), // Light yellow inner
      3.5,
    ),

    // Tertiary roads - medium-small, light with gray border
    "tertiary" | "tertiary_link" => (
      color(120, 120, 120), // Medium gray casing
      3.5,
      color(255, 255, 255), // White inner
      2.5,
    ),

    // Residential/unclassified - small, white with gray border
    "residential" | "unclassified" => (
      color(150, 150, 150), // Light gray casing
      2.5,
      color(255, 255, 255), // White inner
      1.5,
    ),

    // Service roads - smallest, light gray
    "service" | "minor" => (
      color(180, 180, 180), // Very light gray casing
      1.5,
      color(230, 230, 230), // Almost white inner
      1.0,
    ),

    // Paths/footways - very thin
    "path" | "footway" | "pedestrian" | "cycleway" => (
      color(200, 150, 100), // Brown casing
      1.2,
      color(250, 220, 200), // Light brown inner
      0.8,
    ),

    // Default for unknown road types
    _ => (*ROAD_CASING_COLOR, 2.5, *ROAD_COLOR, 1.5),
  };

  // Apply zoom-based scaling to widths
  (
    casing_color,
    base_casing_width * zoom_scale,
    inner_color,
    base_inner_width * zoom_scale,
  )
}
