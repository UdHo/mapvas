use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, RwLock};

use dirs::home_dir;
use log::{error, info};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tiny_skia::Color;

// =============================================================================
// STYLE CONFIG - Loaded from file, mutable at runtime
// =============================================================================

/// Global style configuration, can be modified at runtime
static STYLE_CONFIG: LazyLock<RwLock<StyleConfig>> =
  LazyLock::new(|| RwLock::new(StyleConfig::load_from(None)));

/// Path to the style config file (for saving)
static STYLE_CONFIG_PATH: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Version counter - incremented whenever style config changes
/// Used by renderers to detect when they need to invalidate caches
static STYLE_VERSION: AtomicU64 = AtomicU64::new(0);

/// Get the current style version number
/// This increments each time the style config is modified
#[must_use]
pub fn style_version() -> u64 {
  STYLE_VERSION.load(Ordering::Relaxed)
}

/// Initialize the style config with a specific path (call this early in app startup)
/// If not called, `style_config()` will use the default path.
pub fn init_style_config(path: Option<&Path>) {
  // Store the path for saving later
  if let Ok(mut path_guard) = STYLE_CONFIG_PATH.write() {
    *path_guard = path.map(PathBuf::from).or_else(StyleConfig::default_path);
  }

  // Load and set the config
  let config = StyleConfig::load_from(path);
  if let Ok(mut guard) = STYLE_CONFIG.write() {
    *guard = config;
  }

  // Increment version to invalidate caches
  STYLE_VERSION.fetch_add(1, Ordering::Relaxed);
  info!("Style config initialized, version {}", style_version());
}

/// Get a clone of the current style configuration
#[must_use]
pub fn style_config() -> StyleConfig {
  STYLE_CONFIG.read().map_or_else(|_| StyleConfig::default(), |g| g.clone())
}

/// Update the global style configuration
pub fn set_style_config(config: StyleConfig) {
  if let Ok(mut guard) = STYLE_CONFIG.write() {
    *guard = config;
  }
  // Increment version to invalidate caches
  STYLE_VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Save the current style configuration to the file
///
/// # Errors
/// Returns an error if the config path is unavailable or the file cannot be written.
pub fn save_style_config() -> Result<(), String> {
  let config = style_config();
  let path = STYLE_CONFIG_PATH
    .read()
    .ok()
    .and_then(|g| g.clone())
    .or_else(StyleConfig::default_path)
    .ok_or_else(|| "No style config path available".to_string())?;

  // Ensure parent directory exists
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
  }

  let content = config.to_documented_json5();
  std::fs::write(&path, content).map_err(|e| format!("Failed to write style config: {e}"))?;

  info!("Style configuration saved to {}", path.display());
  Ok(())
}

/// RGB color representation, serialized as hex string (e.g., "#FF8800")
#[derive(Debug, Clone, Copy)]
pub struct Rgb {
  pub r: u8,
  pub g: u8,
  pub b: u8,
}

impl Rgb {
  #[must_use]
  pub const fn new(r: u8, g: u8, b: u8) -> Self {
    Self { r, g, b }
  }

  #[must_use]
  pub fn to_color(self) -> Color {
    Color::from_rgba8(self.r, self.g, self.b, 255)
  }

  /// Parse from hex string (with or without #)
  #[must_use]
  pub fn from_hex(s: &str) -> Option<Self> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
      return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Self { r, g, b })
  }

  /// Convert to hex string with # prefix
  #[must_use]
  pub fn to_hex(self) -> String {
    format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
  }
}

impl Serialize for Rgb {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&self.to_hex())
  }
}

impl<'de> Deserialize<'de> for Rgb {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let s = String::deserialize(deserializer)?;
    Self::from_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid hex color: {s}")))
  }
}

/// Road styling configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoadStyle {
  pub casing: Rgb,
  pub inner: Rgb,
  pub casing_width: f32,
  pub inner_width: f32,
}

/// Complete style configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleConfig {
  // Base colors
  pub background: Rgb,
  pub water: Rgb,
  pub land: Rgb,
  pub park: Rgb,
  pub building: Rgb,

  // Text colors
  pub marker_dot: Rgb,
  pub place_label: Rgb,
  pub road_label: Rgb,
  pub water_label: Rgb,

  // Font sizes
  pub font_sizes: FontSizeConfig,

  // Marker styling
  pub markers: MarkerConfig,

  // Visibility thresholds
  pub visibility: VisibilityConfig,

  // Rendering constants
  pub rendering: RenderingConfig,

  // Road styling by type
  pub roads: RoadStyleConfig,

  // Zoom scaling
  pub zoom_scale: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontSizeConfig {
  pub country: f32,
  pub capital: f32,
  pub megacity: f32,
  pub large_city: f32,
  pub city: f32,
  pub medium_city: f32,
  pub small_city: f32,
  pub city_no_rank: f32,
  pub town: f32,
  pub village: f32,
  pub default: f32,
  pub road_label: f32,
  pub water_label: f32,
  pub max_font_size: f32,
  pub max_label_scale: f32,
  pub char_width_ratio: f32,
  pub max_path_coverage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkerConfig {
  pub base_radius: f32,
  pub max_radius: f32,
  pub text_offset_x: f32,
  pub text_vertical_center_factor: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_field_names)]
pub struct VisibilityConfig {
  pub capital_min_zoom: u8,
  pub city_rank_14_min_zoom: u8,
  pub city_rank_15_min_zoom: u8,
  pub city_rank_16_min_zoom: u8,
  pub city_rank_17_min_zoom: u8,
  pub city_no_rank_min_zoom: u8,
  pub town_min_zoom: u8,
  pub village_min_zoom: u8,
  pub country_min_zoom: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderingConfig {
  pub tile_size: u32,
  pub mvt_extent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoadStyleConfig {
  pub motorway: RoadStyle,
  pub trunk: RoadStyle,
  pub primary: RoadStyle,
  pub secondary: RoadStyle,
  pub tertiary: RoadStyle,
  pub residential: RoadStyle,
  pub service: RoadStyle,
  pub path: RoadStyle,
  pub default_casing: Rgb,
  pub default_inner: Rgb,
  pub default_casing_width: f32,
  pub default_inner_width: f32,
  pub water_width: f32,
}

impl StyleConfig {
  /// Load style config from a specific path, or use default path if None
  pub fn load_from(path: Option<&Path>) -> Self {
    let config_path = path.map(PathBuf::from).or_else(Self::default_path);

    if let Some(path) = &config_path
      && path.exists()
    {
      match std::fs::read_to_string(path) {
        Ok(contents) => match json5::from_str(&contents) {
          Ok(config) => {
            info!("Loaded style config from {}", path.display());
            return config;
          }
          Err(e) => {
            error!("Failed to parse style config: {e}");
          }
        },
        Err(e) => {
          error!("Failed to read style config: {e}");
        }
      }
    }

    let config = Self::default();

    // Write default config if it doesn't exist
    if let Some(path) = &config_path
      && !path.exists()
    {
      if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
      }
      let documented = config.to_documented_json5();
      if std::fs::write(path, &documented).is_ok() {
        info!("Created default style config at {}", path.display());
      }
    }

    config
  }

  /// Generate a documented JSON5 config with comments explaining each field
  #[must_use]
  #[allow(clippy::too_many_lines)]
  pub fn to_documented_json5(&self) -> String {
    format!(
      r#"// MapVas Vector Tile Style Configuration
// =========================================
// This file uses JSON5 format: comments, trailing commas, and unquoted keys are supported.
// Colors use hex format: #RRGGBB

{{
  // ===================
  // Base Map Colors
  // ===================

  "background": "{background}",  // Default background color for empty areas
  "water": "{water}",            // Oceans, lakes, rivers
  "land": "{land}",              // General land/terrain color
  "park": "{park}",              // Parks, forests, green spaces
  "building": "{building}",      // Building footprints

  // ===================
  // Label Colors
  // ===================

  "marker_dot": "{marker_dot}",    // Small dot marker for place labels
  "place_label": "{place_label}",  // City/town/village names
  "road_label": "{road_label}",    // Street names
  "water_label": "{water_label}",  // River/lake names

  // ===================
  // Font Sizes
  // ===================
  // Base sizes before zoom scaling. Actual size = base * (tile_size / 256)

  "font_sizes": {{
    "country": {font_country},       // Country names
    "capital": {font_capital},       // Capital cities
    "megacity": {font_megacity},     // Cities with 10M+ population
    "large_city": {font_large_city}, // Cities with 5M+ population
    "city": {font_city},             // Cities with 1M+ population
    "medium_city": {font_medium_city}, // Cities with 500K+ population
    "small_city": {font_small_city},   // Smaller cities
    "city_no_rank": {font_city_no_rank}, // Cities without population data
    "town": {font_town},             // Towns
    "village": {font_village},       // Villages
    "default": {font_default},       // Fallback size
    "road_label": {font_road_label}, // Street name labels
    "water_label": {font_water_label}, // Water feature labels
    "max_font_size": {font_max},     // Maximum allowed font size
    "max_label_scale": {font_max_label_scale}, // Max scale multiplier for line labels
    "char_width_ratio": {font_char_width},     // Estimated char width as fraction of font size
    "max_path_coverage": {font_max_path}       // Max text length as fraction of path length
  }},

  // ===================
  // Place Markers
  // ===================

  "markers": {{
    "base_radius": {marker_base_radius},   // Base dot radius in pixels
    "max_radius": {marker_max_radius},     // Maximum dot radius
    "text_offset_x": {marker_text_offset}, // Horizontal offset from dot to label
    "text_vertical_center_factor": {marker_vcenter} // Divide font size by this for vertical centering
  }},

  // ===================
  // Zoom Visibility
  // ===================
  // Minimum zoom levels at which features become visible

  "visibility": {{
    "capital_min_zoom": {vis_capital},           // Capital cities
    "city_rank_14_min_zoom": {vis_rank14},       // Large cities (5M+ pop, rank <= 14)
    "city_rank_15_min_zoom": {vis_rank15},       // Cities (1M+ pop, rank <= 15)
    "city_rank_16_min_zoom": {vis_rank16},       // Medium cities (500K+ pop, rank <= 16)
    "city_rank_17_min_zoom": {vis_rank17},       // All ranked cities (rank <= 17)
    "city_no_rank_min_zoom": {vis_no_rank},      // Cities without population rank
    "town_min_zoom": {vis_town},                 // Towns
    "village_min_zoom": {vis_village},           // Villages
    "country_min_zoom": {vis_country}            // Country labels
  }},

  // ===================
  // Rendering
  // ===================

  "rendering": {{
    "tile_size": {render_tile_size},   // Output tile size in pixels (default 512)
    "mvt_extent": {render_mvt_extent}  // MVT coordinate extent (standard 4096)
  }},

  // ===================
  // Road Styles
  // ===================
  // Each road type has: casing (border), inner (fill), and widths
  // Widths are base values, scaled by zoom_scale factors

  "roads": {{
    "motorway": {{
      "casing": "{road_motorway_casing}",  // Border color
      "inner": "{road_motorway_inner}",    // Fill color
      "casing_width": {road_motorway_cw},  // Border width
      "inner_width": {road_motorway_iw}    // Fill width
    }},
    "trunk": {{
      "casing": "{road_trunk_casing}",
      "inner": "{road_trunk_inner}",
      "casing_width": {road_trunk_cw},
      "inner_width": {road_trunk_iw}
    }},
    "primary": {{
      "casing": "{road_primary_casing}",
      "inner": "{road_primary_inner}",
      "casing_width": {road_primary_cw},
      "inner_width": {road_primary_iw}
    }},
    "secondary": {{
      "casing": "{road_secondary_casing}",
      "inner": "{road_secondary_inner}",
      "casing_width": {road_secondary_cw},
      "inner_width": {road_secondary_iw}
    }},
    "tertiary": {{
      "casing": "{road_tertiary_casing}",
      "inner": "{road_tertiary_inner}",
      "casing_width": {road_tertiary_cw},
      "inner_width": {road_tertiary_iw}
    }},
    "residential": {{
      "casing": "{road_residential_casing}",
      "inner": "{road_residential_inner}",
      "casing_width": {road_residential_cw},
      "inner_width": {road_residential_iw}
    }},
    "service": {{
      "casing": "{road_service_casing}",
      "inner": "{road_service_inner}",
      "casing_width": {road_service_cw},
      "inner_width": {road_service_iw}
    }},
    "path": {{
      "casing": "{road_path_casing}",
      "inner": "{road_path_inner}",
      "casing_width": {road_path_cw},
      "inner_width": {road_path_iw}
    }},
    "default_casing": "{road_default_casing}",  // Fallback border color
    "default_inner": "{road_default_inner}",    // Fallback fill color
    "default_casing_width": {road_default_cw},
    "default_inner_width": {road_default_iw},
    "water_width": {road_water_width}           // Width for water linestrings
  }},

  // ===================
  // Zoom Scale Factors
  // ===================
  // Width multipliers by zoom level (index = zoom level)
  // Controls how road widths scale as you zoom in/out

  "zoom_scale": {zoom_scale}
}}
"#,
      // Base colors
      background = self.background.to_hex(),
      water = self.water.to_hex(),
      land = self.land.to_hex(),
      park = self.park.to_hex(),
      building = self.building.to_hex(),
      // Label colors
      marker_dot = self.marker_dot.to_hex(),
      place_label = self.place_label.to_hex(),
      road_label = self.road_label.to_hex(),
      water_label = self.water_label.to_hex(),
      // Font sizes
      font_country = self.font_sizes.country,
      font_capital = self.font_sizes.capital,
      font_megacity = self.font_sizes.megacity,
      font_large_city = self.font_sizes.large_city,
      font_city = self.font_sizes.city,
      font_medium_city = self.font_sizes.medium_city,
      font_small_city = self.font_sizes.small_city,
      font_city_no_rank = self.font_sizes.city_no_rank,
      font_town = self.font_sizes.town,
      font_village = self.font_sizes.village,
      font_default = self.font_sizes.default,
      font_road_label = self.font_sizes.road_label,
      font_water_label = self.font_sizes.water_label,
      font_max = self.font_sizes.max_font_size,
      font_max_label_scale = self.font_sizes.max_label_scale,
      font_char_width = self.font_sizes.char_width_ratio,
      font_max_path = self.font_sizes.max_path_coverage,
      // Markers
      marker_base_radius = self.markers.base_radius,
      marker_max_radius = self.markers.max_radius,
      marker_text_offset = self.markers.text_offset_x,
      marker_vcenter = self.markers.text_vertical_center_factor,
      // Visibility
      vis_capital = self.visibility.capital_min_zoom,
      vis_rank14 = self.visibility.city_rank_14_min_zoom,
      vis_rank15 = self.visibility.city_rank_15_min_zoom,
      vis_rank16 = self.visibility.city_rank_16_min_zoom,
      vis_rank17 = self.visibility.city_rank_17_min_zoom,
      vis_no_rank = self.visibility.city_no_rank_min_zoom,
      vis_town = self.visibility.town_min_zoom,
      vis_village = self.visibility.village_min_zoom,
      vis_country = self.visibility.country_min_zoom,
      // Rendering
      render_tile_size = self.rendering.tile_size,
      render_mvt_extent = self.rendering.mvt_extent,
      // Roads
      road_motorway_casing = self.roads.motorway.casing.to_hex(),
      road_motorway_inner = self.roads.motorway.inner.to_hex(),
      road_motorway_cw = self.roads.motorway.casing_width,
      road_motorway_iw = self.roads.motorway.inner_width,
      road_trunk_casing = self.roads.trunk.casing.to_hex(),
      road_trunk_inner = self.roads.trunk.inner.to_hex(),
      road_trunk_cw = self.roads.trunk.casing_width,
      road_trunk_iw = self.roads.trunk.inner_width,
      road_primary_casing = self.roads.primary.casing.to_hex(),
      road_primary_inner = self.roads.primary.inner.to_hex(),
      road_primary_cw = self.roads.primary.casing_width,
      road_primary_iw = self.roads.primary.inner_width,
      road_secondary_casing = self.roads.secondary.casing.to_hex(),
      road_secondary_inner = self.roads.secondary.inner.to_hex(),
      road_secondary_cw = self.roads.secondary.casing_width,
      road_secondary_iw = self.roads.secondary.inner_width,
      road_tertiary_casing = self.roads.tertiary.casing.to_hex(),
      road_tertiary_inner = self.roads.tertiary.inner.to_hex(),
      road_tertiary_cw = self.roads.tertiary.casing_width,
      road_tertiary_iw = self.roads.tertiary.inner_width,
      road_residential_casing = self.roads.residential.casing.to_hex(),
      road_residential_inner = self.roads.residential.inner.to_hex(),
      road_residential_cw = self.roads.residential.casing_width,
      road_residential_iw = self.roads.residential.inner_width,
      road_service_casing = self.roads.service.casing.to_hex(),
      road_service_inner = self.roads.service.inner.to_hex(),
      road_service_cw = self.roads.service.casing_width,
      road_service_iw = self.roads.service.inner_width,
      road_path_casing = self.roads.path.casing.to_hex(),
      road_path_inner = self.roads.path.inner.to_hex(),
      road_path_cw = self.roads.path.casing_width,
      road_path_iw = self.roads.path.inner_width,
      road_default_casing = self.roads.default_casing.to_hex(),
      road_default_inner = self.roads.default_inner.to_hex(),
      road_default_cw = self.roads.default_casing_width,
      road_default_iw = self.roads.default_inner_width,
      road_water_width = self.roads.water_width,
      // Zoom scale as JSON array
      zoom_scale = serde_json::to_string(&self.zoom_scale).unwrap_or_else(|_| "[]".to_string()),
    )
  }

  /// Get the default path for the style config file
  fn default_path() -> Option<PathBuf> {
    std::env::var("MAPVAS_CONFIG")
      .ok()
      .map(PathBuf::from)
      .or_else(|| home_dir().map(|p| p.join(".config").join("mapvas")))
      .map(|p| p.join("style.json5"))
  }

  /// Get zoom scale factor for a given zoom level
  #[must_use]
  pub fn get_zoom_scale(&self, zoom: u8) -> f32 {
    let idx = zoom as usize;
    if idx < self.zoom_scale.len() {
      self.zoom_scale[idx]
    } else {
      *self.zoom_scale.last().unwrap_or(&1.0)
    }
  }
}

impl Default for StyleConfig {
  fn default() -> Self {
    Self {
      // Base colors
      background: Rgb::new(242, 239, 233),
      water: Rgb::new(170, 211, 223),
      land: Rgb::new(232, 227, 216),
      park: Rgb::new(200, 230, 180),
      building: Rgb::new(200, 190, 180),

      // Text colors
      marker_dot: Rgb::new(50, 50, 50),
      place_label: Rgb::new(0, 0, 0),
      road_label: Rgb::new(80, 80, 80),
      water_label: Rgb::new(60, 100, 140),

      // Font sizes
      font_sizes: FontSizeConfig::default(),

      // Markers
      markers: MarkerConfig::default(),

      // Visibility
      visibility: VisibilityConfig::default(),

      // Rendering
      rendering: RenderingConfig::default(),

      // Roads
      roads: RoadStyleConfig::default(),

      // Zoom scale factors (index = zoom level, 0-15+)
      zoom_scale: vec![
        0.08, // 0 - Very zoomed out
        0.08, // 1
        0.08, // 2
        0.08, // 3
        0.08, // 4
        0.08, // 5
        0.12, // 6
        0.16, // 7
        0.22, // 8
        0.3,  // 9
        0.4,  // 10
        0.55, // 11
        0.7,  // 12
        0.85, // 13
        1.0,  // 14 - Normal width (street level)
        1.0,  // 15+
      ],
    }
  }
}

impl Default for FontSizeConfig {
  fn default() -> Self {
    Self {
      country: 14.0,
      capital: 12.0,
      megacity: 12.0,
      large_city: 11.0,
      city: 10.0,
      medium_city: 9.0,
      small_city: 8.5,
      city_no_rank: 9.0,
      town: 8.0,
      village: 7.5,
      default: 9.0,
      road_label: 10.0,
      water_label: 9.0,
      max_font_size: 20.0,
      max_label_scale: 2.0,
      char_width_ratio: 0.6,
      max_path_coverage: 0.8,
    }
  }
}

impl Default for MarkerConfig {
  fn default() -> Self {
    Self {
      base_radius: 2.0,
      max_radius: 4.0,
      text_offset_x: 2.0,
      text_vertical_center_factor: 3.0,
    }
  }
}

impl Default for VisibilityConfig {
  fn default() -> Self {
    Self {
      capital_min_zoom: 3,
      city_rank_14_min_zoom: 5,
      city_rank_15_min_zoom: 7,
      city_rank_16_min_zoom: 9,
      city_rank_17_min_zoom: 9,
      city_no_rank_min_zoom: 8,
      town_min_zoom: 11,
      village_min_zoom: 14,
      country_min_zoom: 3,
    }
  }
}

impl Default for RenderingConfig {
  fn default() -> Self {
    Self {
      tile_size: 512,
      mvt_extent: 4096.0,
    }
  }
}

impl Default for RoadStyleConfig {
  fn default() -> Self {
    Self {
      motorway: RoadStyle {
        casing: Rgb::new(160, 20, 20),
        inner: Rgb::new(235, 75, 65),
        casing_width: 10.0,
        inner_width: 7.0,
      },
      trunk: RoadStyle {
        casing: Rgb::new(170, 85, 20),
        inner: Rgb::new(255, 150, 90),
        casing_width: 8.0,
        inner_width: 5.5,
      },
      primary: RoadStyle {
        casing: Rgb::new(150, 110, 60),
        inner: Rgb::new(255, 200, 100),
        casing_width: 6.5,
        inner_width: 4.5,
      },
      secondary: RoadStyle {
        casing: Rgb::new(140, 140, 80),
        inner: Rgb::new(255, 240, 150),
        casing_width: 5.0,
        inner_width: 3.5,
      },
      tertiary: RoadStyle {
        casing: Rgb::new(120, 120, 120),
        inner: Rgb::new(255, 255, 255),
        casing_width: 3.5,
        inner_width: 2.5,
      },
      residential: RoadStyle {
        casing: Rgb::new(150, 150, 150),
        inner: Rgb::new(255, 255, 255),
        casing_width: 2.5,
        inner_width: 1.5,
      },
      service: RoadStyle {
        casing: Rgb::new(180, 180, 180),
        inner: Rgb::new(230, 230, 230),
        casing_width: 1.5,
        inner_width: 1.0,
      },
      path: RoadStyle {
        casing: Rgb::new(200, 150, 100),
        inner: Rgb::new(250, 220, 200),
        casing_width: 1.2,
        inner_width: 0.8,
      },
      default_casing: Rgb::new(180, 180, 180),
      default_inner: Rgb::new(255, 255, 255),
      default_casing_width: 2.5,
      default_inner_width: 1.5,
      water_width: 1.5,
    }
  }
}

// =============================================================================
// HELPER FUNCTIONS - Live values from config
// =============================================================================

/// Get the current background color (updated live when config changes)
#[must_use]
pub fn background_color() -> Color {
  style_config().background.to_color()
}

// =============================================================================
// STYLING FUNCTIONS
// =============================================================================

/// Get fill color for a polygon feature based on layer and class
#[must_use]
pub fn get_fill_color(layer_name: &str, feature_class: &str) -> Option<Color> {
  let cfg = style_config();

  match feature_class {
    "water" | "ocean" | "bay" | "lake" | "river" | "stream" => Some(cfg.water.to_color()),
    "grass" | "forest" | "wood" | "park" | "garden" | "meadow" | "scrub" => {
      Some(cfg.park.to_color())
    }
    "building" => Some(cfg.building.to_color()),
    _ => match layer_name {
      "water" | "waterway" => Some(cfg.water.to_color()),
      "landcover" | "landuse" => Some(cfg.land.to_color()),
      "park" | "green" => Some(cfg.park.to_color()),
      "building" | "buildings" => Some(cfg.building.to_color()),
      _ => None,
    },
  }
}

/// Returns (`casing_color`, `casing_width`, `inner_color`, `inner_width`) for roads
#[must_use]
pub fn get_road_styling(
  layer_name: &str,
  feature_class: &str,
  tile_zoom: u8,
) -> (Color, f32, Color, f32) {
  let cfg = style_config();

  // Handle water features
  if layer_name == "water" || layer_name == "waterway" {
    let water = cfg.water.to_color();
    return (water, 0.0, water, cfg.roads.water_width);
  }

  let zoom_scale = cfg.get_zoom_scale(tile_zoom);

  let style = match feature_class {
    "motorway" | "motorway_link" => &cfg.roads.motorway,
    "trunk" | "trunk_link" => &cfg.roads.trunk,
    "primary" | "primary_link" => &cfg.roads.primary,
    "secondary" | "secondary_link" => &cfg.roads.secondary,
    "tertiary" | "tertiary_link" => &cfg.roads.tertiary,
    "residential" | "unclassified" => &cfg.roads.residential,
    "service" | "minor" => &cfg.roads.service,
    "path" | "footway" | "pedestrian" | "cycleway" => &cfg.roads.path,
    _ => {
      return (
        cfg.roads.default_casing.to_color(),
        cfg.roads.default_casing_width * zoom_scale,
        cfg.roads.default_inner.to_color(),
        cfg.roads.default_inner_width * zoom_scale,
      );
    }
  };

  (
    style.casing.to_color(),
    style.casing_width * zoom_scale,
    style.inner.to_color(),
    style.inner_width * zoom_scale,
  )
}

/// Get font size for a place feature based on its type and importance
#[must_use]
pub fn get_place_font_size(
  feature_kind_detail: Option<&str>,
  is_capital: bool,
  population_rank: Option<i64>,
) -> f32 {
  let fonts = &style_config().font_sizes;

  match (feature_kind_detail, is_capital, population_rank) {
    (Some("country"), _, _) => fonts.country,
    (_, true, _) => fonts.capital,
    (Some("city"), _, Some(pr)) if pr <= 13 => fonts.megacity,
    (Some("city"), _, Some(pr)) if pr <= 14 => fonts.large_city,
    (Some("city"), _, Some(pr)) if pr <= 15 => fonts.city,
    (Some("city"), _, Some(pr)) if pr <= 16 => fonts.medium_city,
    (Some("city"), _, Some(pr)) if pr <= 17 => fonts.small_city,
    (Some("locality"), _, Some(pr)) if pr <= 14 => fonts.large_city,
    (Some("locality"), _, Some(pr)) if pr <= 15 => fonts.city,
    (Some("locality"), _, Some(pr)) if pr <= 16 => fonts.medium_city,
    (Some("locality"), _, Some(pr)) if pr <= 17 => fonts.small_city,
    (Some("city" | "locality"), _, None) => fonts.city_no_rank,
    (Some("town"), _, _) => fonts.town,
    (Some("village"), _, _) => fonts.village,
    _ => fonts.default,
  }
}

/// Check if a place feature should be shown at the given zoom level
#[must_use]
#[allow(clippy::match_same_arms)]
pub fn should_show_place(
  feature_kind_detail: Option<&str>,
  population_rank: Option<i64>,
  is_capital: bool,
  zoom: u8,
) -> bool {
  let vis = &style_config().visibility;

  match (feature_kind_detail, population_rank, is_capital, zoom) {
    (_, _, true, z) if z >= vis.capital_min_zoom => true,
    (Some("city"), Some(pr), _, z) if z < vis.city_rank_14_min_zoom => pr <= 14,
    (Some("city"), Some(pr), _, z) if z < vis.city_rank_15_min_zoom => pr <= 15,
    (Some("city"), Some(pr), _, z) if z < vis.city_rank_16_min_zoom => pr <= 16,
    (Some("city"), Some(pr), _, z) if z >= vis.city_rank_17_min_zoom => pr <= 17,
    (Some("city"), None, _, z) => z >= vis.city_no_rank_min_zoom,
    (Some("locality"), Some(pr), _, z) if z < vis.city_rank_14_min_zoom => pr <= 14,
    (Some("locality"), Some(pr), _, z) if z < vis.city_rank_15_min_zoom => pr <= 15,
    (Some("locality"), Some(pr), _, z) if z < vis.city_rank_16_min_zoom => pr <= 16,
    (Some("locality"), Some(pr), _, z) if z >= vis.city_rank_17_min_zoom => pr <= 17,
    (Some("locality"), None, _, z) => z >= vis.city_no_rank_min_zoom,
    (Some("town"), _, _, z) => z >= vis.town_min_zoom,
    (Some("village"), _, _, z) => z >= vis.village_min_zoom,
    (Some("country"), _, _, z) => z >= vis.country_min_zoom,
    _ => false,
  }
}
