use std::path::PathBuf;

use dirs::home_dir;
use log::error;

use crate::search::SearchProviderConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum HeadingStyle {
  #[default]
  Arrow,
  Line,
  Chevron,
  Needle,
  Sector,
  Rectangle,
}

impl HeadingStyle {
  #[must_use]
  pub fn name(&self) -> &'static str {
    match self {
      HeadingStyle::Arrow => "Arrow",
      HeadingStyle::Line => "Line",
      HeadingStyle::Chevron => "Chevron",
      HeadingStyle::Needle => "Needle",
      HeadingStyle::Sector => "Sector",
      HeadingStyle::Rectangle => "Rectangle",
    }
  }

  #[must_use]
  pub fn all() -> &'static [HeadingStyle] {
    &[
      HeadingStyle::Arrow,
      HeadingStyle::Line,
      HeadingStyle::Chevron,
      HeadingStyle::Needle,
      HeadingStyle::Sector,
      HeadingStyle::Rectangle,
    ]
  }
}

/// Type of tile data served by a tile provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum TileType {
  /// Raster tiles (PNG, JPEG) - rendered images from the server.
  #[default]
  Raster,
  /// Vector tiles (MVT/PBF) - raw geometry data rendered client-side.
  Vector,
}

impl TileType {
  #[must_use]
  pub fn name(&self) -> &'static str {
    match self {
      TileType::Raster => "Raster",
      TileType::Vector => "Vector",
    }
  }

  #[must_use]
  pub fn all() -> &'static [TileType] {
    &[TileType::Raster, TileType::Vector]
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct TileProvider {
  pub name: String,
  pub url: String,
  #[serde(default)]
  pub tile_type: TileType,
  #[serde(default)]
  pub max_zoom: Option<u8>,
}

impl TileProvider {
  /// Get the maximum zoom level for this provider, using defaults if not specified.
  /// Returns 19 for raster tiles and 15 for vector tiles.
  #[must_use]
  pub fn get_max_zoom(&self) -> u8 {
    self.max_zoom.unwrap_or(match self.tile_type {
      TileType::Raster => 19,
      TileType::Vector => 15,
    })
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
  pub config_path: Option<PathBuf>,
  pub tile_provider: Vec<TileProvider>,
  pub tile_cache_dir: Option<PathBuf>,
  pub commands_dir: Option<PathBuf>,
  pub search_providers: Vec<SearchProviderConfig>,
  pub heading_style: HeadingStyle,
}

const DEFAULT_TILE_URL: &str = "https://tile.openstreetmap.org/{zoom}/{x}/{y}.png";
const DEFAULT_VECTOR_TILE_URL: &str =
  "https://tiles.openfreemap.org/planet/20251231_001001_pt/{zoom}/{x}/{y}.pbf";

impl Config {
  #[must_use]
  pub fn new() -> Self {
    let from_env = Self::from_env();
    let from_file = Self::from_file();
    let default = Self::default();

    let mut merged = from_env;
    if let Some(from_file) = &from_file {
      merged = merged.merge(from_file);
    }
    merged = merged.merge(&default);

    if merged.config_path.is_some() && from_file.is_none() {
      merged.init_cfg_file();
    }

    merged
  }

  fn from_env() -> Self {
    let config_path = std::env::var("MAPVAS_CONFIG").ok().map(PathBuf::from);

    let tile_urls: Vec<TileProvider> =
      std::env::var("MAPVAS_TILE_URL")
        .ok()
        .map_or_else(Vec::new, |v| {
          vec![TileProvider {
            name: "ENV".to_string(),
            url: v,
            tile_type: TileType::default(),
            max_zoom: None,
          }]
        });

    let tile_cache_dir = std::env::var("MAPVAS_TILE_CACHE_DIR")
      .ok()
      .map(PathBuf::from);

    let commands_dir = config_path.as_ref().map(|p| p.join("commands"));

    Self {
      config_path,
      tile_provider: tile_urls,
      tile_cache_dir,
      commands_dir,
      search_providers: Vec::new(),
      heading_style: HeadingStyle::default(),
    }
  }

  fn merge(mut self, other: &Self) -> Self {
    self.config_path = self.config_path.or(other.config_path.clone());
    for tile in &other.tile_provider {
      if !self.tile_provider.iter().any(|t| t == tile) {
        self.tile_provider.push(tile.clone());
      }
    }

    self.tile_cache_dir = self.tile_cache_dir.or(other.tile_cache_dir.clone());
    self.commands_dir = self.commands_dir.or(other.commands_dir.clone());

    // Merge search providers (avoid duplicates)
    for provider in &other.search_providers {
      if !self.search_providers.iter().any(|p| p == provider) {
        self.search_providers.push(provider.clone());
      }
    }

    // Use other's heading style if we don't have one set (or if other is not default)
    if self.heading_style == HeadingStyle::default()
      || other.heading_style != HeadingStyle::default()
    {
      self.heading_style = other.heading_style;
    }

    self
  }

  fn from_file() -> Option<Self> {
    let config_path = std::env::var("MAPVAS_CONFIG")
      .ok()
      .map(PathBuf::from)
      .or_else(|| home_dir().map(|p| p.join(".config").join("mapvas")))?;
    let config_path = config_path.join("config.json");

    serde_json::from_str(&std::fs::read_to_string(&config_path).ok()?)
      .inspect_err(|e| error!("Failed to read config file: {e}"))
      .ok()?
  }

  fn init_cfg_file(&self) {
    if let Some(path) = &self.config_path {
      if !path.exists() {
        let _ = std::fs::create_dir_all(path).inspect_err(|e| {
          error!("Failed to create config directory: {e}");
        });
      }

      if let Some(path) = &self.tile_cache_dir
        && !path.exists()
      {
        let _ = std::fs::create_dir_all(path).inspect_err(|e| {
          error!("Failed to create tile cache directory: {e}");
        });
      }
    }

    if let Some(path) = &self.commands_dir
      && !path.exists()
    {
      let _ = std::fs::create_dir_all(path).inspect_err(|e| {
        error!("Failed to create commands directory: {e}");
      });
    }

    if let Some(path) = &self.config_path {
      let path = path.join("config.json");
      if !path.exists() {
        let config = serde_json::to_string_pretty(self);
        if let Ok(config) = config {
          let _ = std::fs::write(path, config).inspect_err(|e| {
            error!("Failed to write config file: {e}");
          });
        } else {
          error!("Failed to serialize config");
        }
      }
    }
  }
}

impl Default for Config {
  fn default() -> Self {
    let config_path = home_dir().map(|p| p.join(".config").join("mapvas"));
    let commands_dir = config_path.as_ref().map(|p| p.join("commands"));
    Self {
      config_path,
      tile_provider: vec![
        TileProvider {
          name: "OpenStreetMap".to_string(),
          url: DEFAULT_TILE_URL.to_string(),
          tile_type: TileType::Raster,
          max_zoom: Some(19),
        },
        TileProvider {
          name: "OpenFreeMap Vector (experimental)".to_string(),
          url: DEFAULT_VECTOR_TILE_URL.to_string(),
          tile_type: TileType::Vector,
          max_zoom: Some(15),
        },
      ],
      tile_cache_dir: home_dir().map(|p| p.join(".mapvas_tile_cache")),
      commands_dir,
      search_providers: vec![
        SearchProviderConfig::Coordinate,
        SearchProviderConfig::Nominatim { base_url: None },
      ],
      heading_style: HeadingStyle::default(),
    }
  }
}
