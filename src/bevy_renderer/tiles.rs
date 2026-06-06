use std::{
  collections::{HashMap, HashSet},
  sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
  },
};

use crate::{
  config::{Config, TileProvider, TileType},
  map::{
    coordinates::{
      PixelCoordinate, PixelPosition, Tile, TileCoordinate, TilePriority, generate_preload_tiles,
      tile_zoom_for_transform, tiles_in_box,
    },
    tile_loader::{CachedTileLoader, TileLoader, TileSource},
    tile_renderer::{
      RasterTileRenderer, TileImage, TileRenderer, VectorTileRenderer, style_version,
    },
    viewport::MapViewport,
  },
  task_tracker::{TaskCategory, TaskGuard},
};
use bevy::prelude::*;

mod detail;
mod fonts;
mod native_vector;
mod raster;
mod results;
mod screen;

use detail::{
  MAX_TILE_DETAIL_FACTOR, MIN_TILE_DETAIL_FACTOR, clamped_tile_detail_factor,
  effective_tile_detail_factor_label, native_vector_style_zoom, tile_detail_factor_label,
  tile_zoom_with_detail_factor,
};
use fonts::load_bevy_tile_label_fonts;

const MAX_PRELOAD_TILES: usize = 20;

#[derive(Resource)]
pub struct BevyTileRuntime(pub tokio::runtime::Handle);

pub struct BevyTilePlugin;

impl Plugin for BevyTilePlugin {
  fn build(&self, app: &mut App) {
    app
      .init_gizmo_group::<BevyTileOverlayGizmos>()
      .add_systems(
        Startup,
        (
          configure_bevy_tile_overlay_gizmos,
          load_bevy_tile_label_fonts,
        ),
      )
      .add_systems(
        Update,
        (
          results::collect_finished_tiles,
          raster::update_bevy_tiles,
          native_vector::update_native_vector_tiles,
          native_vector::update_native_vector_tile_labels,
          native_vector::draw_native_vector_debug_selection,
        )
          .chain(),
      );
  }
}

#[derive(Default, Reflect, GizmoConfigGroup)]
#[reflect(Default)]
struct BevyTileOverlayGizmos;

#[derive(Component)]
struct BevyTileSprite;

#[derive(Component)]
struct BevyGridTileSprite;

#[derive(Component)]
struct BevyTileLabelBackground;

#[derive(Component)]
struct BevyTileLabelText;

#[derive(Component)]
struct BevyOsmAttributionBackground;

#[derive(Component)]
struct BevyOsmAttributionText;

#[derive(Component)]
struct BevyNativeVectorTileMesh;

#[derive(Component)]
struct BevyNativeVectorTileLabelMarker;

#[derive(Component)]
struct BevyNativeVectorTileLabelText;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CoordinateDisplayMode {
  Off,
  Overlay,
  GridOnly,
}

struct BevyTileEntry {
  image: Handle<Image>,
  entities: Vec<Entity>,
}

#[derive(Clone, Copy)]
struct NativeVectorTileBounds {
  min_x: f32,
  min_y: f32,
  max_x: f32,
  max_y: f32,
}

impl NativeVectorTileBounds {
  fn from_min_max(min: PixelCoordinate, max: PixelCoordinate) -> Self {
    Self {
      min_x: min.x,
      min_y: min.y,
      max_x: max.x,
      max_y: max.y,
    }
  }

  fn include(&mut self, coord: PixelCoordinate) {
    self.min_x = self.min_x.min(coord.x);
    self.min_y = self.min_y.min(coord.y);
    self.max_x = self.max_x.max(coord.x);
    self.max_y = self.max_y.max(coord.y);
  }
}

struct NativeVectorTileMeshSpec {
  mesh: Mesh,
  color: Color,
  bounds: NativeVectorTileBounds,
  origin: PixelCoordinate,
  z: f32,
}

struct NativeVectorTileMeshInstance {
  mesh: Handle<Mesh>,
  material: Handle<ColorMaterial>,
  bounds: NativeVectorTileBounds,
  origin: PixelCoordinate,
  z: f32,
  entities: Vec<Entity>,
}

struct NativeVectorTileLabelSpec {
  coord: PixelCoordinate,
  text: String,
  base_font_size: f32,
  text_color: Color,
  show_marker: bool,
  centered: bool,
  max_point_zoom: Option<u8>,
  tile_world_size: f32,
}

#[derive(Clone, Copy)]
struct NativeVectorTileLabelCopy {
  marker: Entity,
  text: Entity,
}

struct NativeVectorTileLabelInstance {
  coord: PixelCoordinate,
  text: String,
  base_font_size: f32,
  text_color: Color,
  show_marker: bool,
  centered: bool,
  max_point_zoom: Option<u8>,
  tile_world_size: f32,
  entities: Vec<NativeVectorTileLabelCopy>,
}

#[derive(Clone)]
struct NativeVectorDebugFeature {
  tile: Tile,
  feature_index: usize,
  source_layer: String,
  geometry_type: String,
  feature_class: String,
  feature_kind: Option<String>,
  feature_kind_detail: Option<String>,
  feature_name: Option<String>,
  style_summary: String,
  bounds: NativeVectorTileBounds,
  geometry: NativeVectorDebugGeometry,
  properties: Vec<(String, String)>,
}

#[derive(Clone)]
enum NativeVectorDebugGeometry {
  Points(Vec<PixelCoordinate>),
  Lines(Vec<Vec<PixelCoordinate>>),
  Polygons(Vec<Vec<PixelCoordinate>>),
}

#[derive(Clone)]
struct NativeVectorDebugSelection {
  click: PixelCoordinate,
  feature: NativeVectorDebugFeature,
}

struct BevyNativeVectorTileEntry {
  instances: Vec<NativeVectorTileMeshInstance>,
  labels: Vec<NativeVectorTileLabelInstance>,
  debug_features: Vec<NativeVectorDebugFeature>,
}

struct NativeVectorTileContent {
  meshes: Vec<NativeVectorTileMeshSpec>,
  labels: Vec<NativeVectorTileLabelSpec>,
  debug_features: Vec<NativeVectorDebugFeature>,
}

enum BevyTileResult {
  Ready {
    generation: u64,
    tile: Tile,
    image: TileImage,
  },
  NativeVectorReady {
    generation: u64,
    tile: Tile,
    meshes: Vec<NativeVectorTileMeshSpec>,
    labels: Vec<NativeVectorTileLabelSpec>,
    debug_features: Vec<NativeVectorDebugFeature>,
  },
  Failed {
    generation: u64,
    tile: Tile,
  },
}

#[derive(Resource)]
pub struct BevyTileLayer {
  receiver: Mutex<Receiver<BevyTileResult>>,
  sender: Sender<BevyTileResult>,
  all_tile_loader: Vec<Arc<CachedTileLoader>>,
  tile_loader_index: usize,
  tile_providers: Vec<TileProvider>,
  tile_source: TileSource,
  visible: bool,
  native_vector_tiles_enabled: bool,
  tile_detail_factor: f32,
  loaded_tiles: HashMap<Tile, BevyTileEntry>,
  loaded_native_vector_tiles: HashMap<Tile, BevyNativeVectorTileEntry>,
  in_flight_tiles: HashSet<Tile>,
  stale_entities: Vec<Entity>,
  stale_native_vector_entities: Vec<Entity>,
  raster_renderer: Arc<dyn TileRenderer>,
  vector_renderer: Arc<dyn TileRenderer>,
  last_visible_tiles: Vec<Tile>,
  preload_enabled: bool,
  generation: u64,
  last_style_version: u64,
  current_ideal_zoom: u8,
  current_request_zoom: u8,
  current_max_zoom: u8,
  coordinate_display_mode: CoordinateDisplayMode,
  debug_elements_enabled: bool,
  selected_debug_feature: Option<NativeVectorDebugSelection>,
}

fn configure_bevy_tile_overlay_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
  let (config, _) = config_store.config_mut::<BevyTileOverlayGizmos>();
  config.line.width = 1.0;
  config.depth_bias = -1.0;
}

impl BevyTileLayer {
  #[must_use]
  pub fn new(config: Config) -> Self {
    let (sender, receiver) = std::sync::mpsc::channel();
    Self {
      receiver: Mutex::new(receiver),
      sender,
      all_tile_loader: CachedTileLoader::from_config(&config)
        .map(Arc::new)
        .collect(),
      tile_loader_index: 0,
      tile_providers: config.tile_provider.clone(),
      tile_source: TileSource::All,
      visible: true,
      native_vector_tiles_enabled: true,
      tile_detail_factor: 1.0,
      loaded_tiles: HashMap::new(),
      loaded_native_vector_tiles: HashMap::new(),
      in_flight_tiles: HashSet::new(),
      stale_entities: Vec::new(),
      stale_native_vector_entities: Vec::new(),
      raster_renderer: Arc::new(RasterTileRenderer::new()),
      vector_renderer: Arc::new(VectorTileRenderer::new()),
      last_visible_tiles: Vec::new(),
      preload_enabled: true,
      generation: 0,
      last_style_version: style_version(),
      current_ideal_zoom: 0,
      current_request_zoom: 0,
      current_max_zoom: 0,
      coordinate_display_mode: CoordinateDisplayMode::Off,
      debug_elements_enabled: false,
      selected_debug_feature: None,
    }
  }

  pub fn update_config(&mut self, config: &Config) {
    if self.tile_providers == config.tile_provider {
      return;
    }

    let selected_provider = self.tile_loader().map(|loader| loader.name().to_string());
    self.tile_providers.clone_from(&config.tile_provider);
    self.all_tile_loader = CachedTileLoader::from_config(config)
      .map(Arc::new)
      .collect();
    self.tile_loader_index = selected_provider
      .and_then(|selected| {
        self
          .all_tile_loader
          .iter()
          .position(|loader| loader.name() == selected)
      })
      .unwrap_or(0)
      .min(self.all_tile_loader.len().saturating_sub(1));
    self.clear_tiles();
  }

  pub fn refresh_style_version(&mut self) {
    let current_style_version = style_version();
    if current_style_version == self.last_style_version {
      return;
    }

    self.last_style_version = current_style_version;
    self.clear_tiles();
  }

  pub fn set_visible(&mut self, visible: bool) {
    if self.visible == visible {
      return;
    }

    self.visible = visible;
    self.clear_tiles();
  }

  pub fn set_preload_enabled(&mut self, preload_enabled: bool) {
    self.preload_enabled = preload_enabled;
  }

  #[must_use]
  pub fn has_pending_work(&self) -> bool {
    !self.in_flight_tiles.is_empty()
      || self
        .tile_loader()
        .is_some_and(|loader| loader.tiles_downloading() > 0 || loader.tiles_queued() > 0)
  }

  pub fn ui(&mut self, ui: &mut egui::Ui) {
    ui.collapsing("Bevy Tile Layer", |ui| {
      ui.checkbox(&mut self.visible, "visible");

      let selected_provider = self
        .tile_loader()
        .map_or_else(|| "none".to_string(), |loader| loader.name().to_string());
      let mut tile_loader_index = self.tile_loader_index;
      egui::ComboBox::from_label("tile provider")
        .selected_text(selected_provider)
        .show_ui(ui, |ui| {
          for (i, tile_loader) in self.all_tile_loader.iter().enumerate() {
            ui.selectable_value(&mut tile_loader_index, i, tile_loader.name().to_string());
          }
        });
      if tile_loader_index != self.tile_loader_index {
        self.tile_loader_index = tile_loader_index;
        self.clear_tiles();
      }

      let mut tile_source = self.tile_source;
      egui::ComboBox::from_label("tile source")
        .selected_text(tile_source.to_string())
        .show_ui(ui, |ui| {
          for source in [TileSource::All, TileSource::Cache, TileSource::Download] {
            ui.selectable_value(&mut tile_source, source, source.to_string());
          }
        });
      if tile_source != self.tile_source {
        self.tile_source = tile_source;
        self.clear_tiles();
      }

      let mut tile_detail_factor = self.tile_detail_factor;
      ui.horizontal(|ui| {
        ui.label("tile detail factor");
        if ui
          .add(
            egui::Slider::new(
              &mut tile_detail_factor,
              MIN_TILE_DETAIL_FACTOR..=MAX_TILE_DETAIL_FACTOR,
            )
            .step_by(0.05)
            .custom_formatter(|value, _| tile_detail_factor_label(value as f32)),
          )
          .changed()
        {
          self.tile_detail_factor = clamped_tile_detail_factor(tile_detail_factor);
          self.clear_tiles();
        }
      });

      if self
        .tile_loader()
        .is_some_and(|loader| loader.tile_type() == TileType::Vector)
        && ui
          .checkbox(
            &mut self.native_vector_tiles_enabled,
            "native vector geometry",
          )
          .changed()
      {
        self.clear_tiles();
      }

      ui.checkbox(&mut self.preload_enabled, "preload adjacent tiles");
      let debug_changed = ui
        .checkbox(&mut self.debug_elements_enabled, "debug map elements")
        .changed();
      if debug_changed && !self.debug_elements_enabled {
        self.selected_debug_feature = None;
      }

      ui.separator();
      ui.label("Tile Coordinate Display:");
      ui.radio_value(
        &mut self.coordinate_display_mode,
        CoordinateDisplayMode::Off,
        "Off",
      );
      ui.radio_value(
        &mut self.coordinate_display_mode,
        CoordinateDisplayMode::Overlay,
        "Overlay",
      );
      ui.radio_value(
        &mut self.coordinate_display_mode,
        CoordinateDisplayMode::GridOnly,
        "Grid Only",
      );

      ui.separator();
      ui.label("Statistics:");
      ui.horizontal(|ui| {
        ui.label("Ideal zoom:");
        ui.label(self.current_ideal_zoom.to_string());
      });
      ui.horizontal(|ui| {
        ui.label("Request zoom:");
        ui.label(self.current_request_zoom.to_string());
      });
      ui.horizontal(|ui| {
        ui.label("Effective detail:");
        ui.label(effective_tile_detail_factor_label(self.tile_detail_factor));
      });
      ui.horizontal(|ui| {
        ui.label("Max zoom:");
        ui.label(self.current_max_zoom.to_string());
      });

      ui.separator();
      let tiles_downloading = self
        .tile_loader()
        .map_or(0, |loader| loader.tiles_downloading());
      let tiles_queued = self.tile_loader().map_or(0, |loader| loader.tiles_queued());
      let tiles_in_flight = self.in_flight_tiles.len();
      let tiles_loaded = self.loaded_tiles.len();
      let native_tiles_loaded = self.loaded_native_vector_tiles.len();
      let tiles_rendering = tiles_in_flight.saturating_sub(tiles_downloading);

      ui.horizontal(|ui| {
        ui.label("Tiles loaded:");
        ui.label(tiles_loaded.to_string());
      });
      ui.horizontal(|ui| {
        ui.label("Native vector tiles:");
        ui.label(native_tiles_loaded.to_string());
      });
      ui.horizontal(|ui| {
        ui.label("Tiles downloading:");
        ui.label(tiles_downloading.to_string());
      });
      ui.horizontal(|ui| {
        ui.label("Tiles queued:");
        ui.label(tiles_queued.to_string());
      });
      ui.horizontal(|ui| {
        ui.label("Tiles in flight:");
        ui.label(tiles_in_flight.to_string());
      });
      ui.horizontal(|ui| {
        ui.label("Tiles rendering:");
        ui.label(tiles_rendering.to_string());
      });

      self.debug_ui(ui);
    });
  }

  pub fn select_debug_feature(&mut self, screen_pos: PixelPosition, viewport: Option<MapViewport>) {
    if !self.debug_elements_enabled {
      return;
    }
    self.selected_debug_feature =
      viewport.and_then(|viewport| native_vector::select_debug_feature(self, viewport, screen_pos));
  }

  fn debug_ui(&self, ui: &mut egui::Ui) {
    if !self.debug_elements_enabled {
      return;
    }

    ui.separator();
    ui.label("Selected map element:");
    let Some(selection) = &self.selected_debug_feature else {
      ui.label("None");
      return;
    };
    let feature = &selection.feature;
    stat_row(
      ui,
      "Click:",
      format!("{:.2}, {:.2}", selection.click.x, selection.click.y),
    );
    stat_row(
      ui,
      "Tile:",
      format!(
        "{}/{}/{}",
        feature.tile.zoom, feature.tile.x, feature.tile.y
      ),
    );
    stat_row(ui, "Feature:", feature.feature_index.to_string());
    stat_row(ui, "Layer:", feature.source_layer.clone());
    stat_row(ui, "Geometry:", feature.geometry_type.clone());
    if let Some(name) = &feature.feature_name {
      stat_row(ui, "Name:", name.clone());
    }
    if !feature.feature_class.is_empty() {
      stat_row(ui, "Class:", feature.feature_class.clone());
    }
    if let Some(kind) = &feature.feature_kind {
      stat_row(ui, "Kind:", kind.clone());
    }
    if let Some(kind_detail) = &feature.feature_kind_detail {
      stat_row(ui, "Kind detail:", kind_detail.clone());
    }
    stat_row(ui, "Style:", feature.style_summary.clone());
    stat_row(
      ui,
      "Bounds:",
      format!(
        "{:.2},{:.2} - {:.2},{:.2}",
        feature.bounds.min_x, feature.bounds.min_y, feature.bounds.max_x, feature.bounds.max_y
      ),
    );
    ui.collapsing("Raw properties", |ui| {
      for (key, value) in &feature.properties {
        stat_row(ui, key, value.clone());
      }
    });
  }

  fn clear_tiles(&mut self) {
    self.generation = self.generation.wrapping_add(1);
    self.in_flight_tiles.clear();
    self.last_visible_tiles.clear();
    self.selected_debug_feature = None;
    for entry in self.loaded_tiles.drain().map(|(_, entry)| entry) {
      self.stale_entities.extend(entry.entities);
    }
    for entry in self
      .loaded_native_vector_tiles
      .drain()
      .map(|(_, entry)| entry)
    {
      self.stale_native_vector_entities.extend(
        entry
          .instances
          .into_iter()
          .flat_map(|instance| instance.entities)
          .chain(entry.labels.into_iter().flat_map(|label| {
            label
              .entities
              .into_iter()
              .flat_map(|copy| [copy.marker, copy.text])
          })),
      );
    }
  }

  fn tile_loader(&self) -> Option<Arc<CachedTileLoader>> {
    self.all_tile_loader.get(self.tile_loader_index).cloned()
  }

  fn renderer_for_tile_type(&self, tile_type: TileType) -> Arc<dyn TileRenderer> {
    match tile_type {
      TileType::Raster => self.raster_renderer.clone(),
      TileType::Vector => self.vector_renderer.clone(),
    }
  }

  fn native_vector_tiles_active(&self) -> bool {
    self.visible
      && self.native_vector_tiles_enabled
      && self
        .tile_loader()
        .is_some_and(|loader| loader.tile_type() == TileType::Vector)
  }

  fn visible_tiles(&mut self, viewport: MapViewport) -> Vec<Tile> {
    let Some(tile_loader) = self.tile_loader() else {
      return Vec::new();
    };

    let calculated_zoom = tile_zoom_for_transform(&viewport.transform);
    let detail_zoom = tile_zoom_with_detail_factor(viewport.transform, self.tile_detail_factor);
    let max_zoom = tile_loader.max_zoom();
    let tile_type = tile_loader.tile_type();
    let request_zoom = if tile_type == TileType::Vector && self.native_vector_tiles_enabled {
      detail_zoom.min(max_zoom)
    } else if tile_type == TileType::Vector && detail_zoom > max_zoom {
      detail_zoom.min(19)
    } else {
      detail_zoom.min(max_zoom)
    };
    self.current_ideal_zoom = calculated_zoom;
    self.current_request_zoom = request_zoom;
    self.current_max_zoom = max_zoom;

    let inv = viewport.transform.invert();
    let vp_min = inv.apply(viewport.min());
    let vp_max = inv.apply(viewport.max());
    let min_pos = TileCoordinate::from_pixel_position(vp_min, request_zoom);
    let max_pos = TileCoordinate::from_pixel_position(vp_max, request_zoom);

    let visible_tiles: Vec<Tile> = tiles_in_box(min_pos, max_pos).collect();
    self.last_visible_tiles.clone_from(&visible_tiles);
    visible_tiles
  }

  fn request_tile(&mut self, tile: Tile, priority: TilePriority, runtime: tokio::runtime::Handle) {
    if self.loaded_tiles.contains_key(&tile) || self.in_flight_tiles.contains(&tile) {
      return;
    }

    let Some(tile_loader) = self.tile_loader() else {
      return;
    };
    let max_zoom = tile_loader.max_zoom();
    let tile_type = tile_loader.tile_type();
    if tile.zoom > max_zoom {
      if tile_type == TileType::Vector && tile.zoom <= 19 {
        self.request_super_resolution_tile(tile, priority, runtime);
      }
      return;
    }

    self.in_flight_tiles.insert(tile);

    let sender = self.sender.clone();
    let renderer = self.renderer_for_tile_type(tile_type);
    let tile_source = self.tile_source;
    let generation = self.generation;
    runtime.spawn(async move {
      let task_name = format!("bevy-tile-{}-{}-{}", tile.zoom, tile.x, tile.y);
      let _guard = TaskGuard::new(task_name, TaskCategory::TileLoad);

      let Ok(tile_data) = tile_loader
        .tile_data_with_priority(&tile, tile_source, priority)
        .await
      else {
        let _ = sender.send(BevyTileResult::Failed { generation, tile });
        return;
      };

      let (render_rx, _) = crate::render_scheduler::RENDER_SCHEDULER
        .submit(priority, move || renderer.render(&tile, &tile_data));

      let render_result = tokio::time::timeout(std::time::Duration::from_secs(30), render_rx).await;
      let image = match render_result {
        Ok(Ok(Ok(image))) => image,
        _ => {
          let _ = sender.send(BevyTileResult::Failed { generation, tile });
          return;
        }
      };

      let _ = sender.send(BevyTileResult::Ready {
        generation,
        tile,
        image,
      });
    });
  }

  #[allow(clippy::cast_possible_truncation)]
  fn request_super_resolution_tile(
    &mut self,
    tile: Tile,
    priority: TilePriority,
    runtime: tokio::runtime::Handle,
  ) {
    let Some(tile_loader) = self.tile_loader() else {
      return;
    };
    let max_zoom = tile_loader.max_zoom();
    let zoom_diff = tile.zoom - max_zoom;

    let mut parent_tile = tile;
    for _ in 0..zoom_diff {
      let Some(parent) = parent_tile.parent() else {
        return;
      };
      parent_tile = parent;
    }

    let grid_size = 1usize << zoom_diff;
    let scale = grid_size as u32;
    let base_x = parent_tile.x << zoom_diff;
    let base_y = parent_tile.y << zoom_diff;

    let mut child_tiles = Vec::with_capacity(grid_size * grid_size);
    for ty in 0..grid_size {
      for tx in 0..grid_size {
        child_tiles.push(Tile {
          x: base_x + tx as u32,
          y: base_y + ty as u32,
          zoom: tile.zoom,
        });
      }
    }

    if child_tiles
      .iter()
      .any(|tile| self.loaded_tiles.contains_key(tile) || self.in_flight_tiles.contains(tile))
    {
      return;
    }

    for child_tile in &child_tiles {
      self.in_flight_tiles.insert(*child_tile);
    }

    let sender = self.sender.clone();
    let renderer = self.vector_renderer.clone();
    let tile_source = self.tile_source;
    let generation = self.generation;
    runtime.spawn(async move {
      let task_name = format!(
        "bevy-tile-superres-{}-{}-{}",
        parent_tile.zoom, parent_tile.x, parent_tile.y
      );
      let _guard = TaskGuard::new(task_name, TaskCategory::TileSuperRes);

      let Ok(tile_data) = tile_loader
        .tile_data_with_priority(&parent_tile, tile_source, priority)
        .await
      else {
        send_failed_tiles(&sender, generation, &child_tiles);
        return;
      };

      let (render_rx, _) = crate::render_scheduler::RENDER_SCHEDULER.submit(priority, move || {
        renderer.render_scaled(&parent_tile, &tile_data, scale)
      });

      let render_result = tokio::time::timeout(std::time::Duration::from_mins(1), render_rx).await;
      let image = match render_result {
        Ok(Ok(Ok(image))) => image,
        _ => {
          send_failed_tiles(&sender, generation, &child_tiles);
          return;
        }
      };

      for (tile, image) in child_tiles
        .iter()
        .copied()
        .zip(split_image_into_tiles(&image, grid_size))
      {
        let _ = sender.send(BevyTileResult::Ready {
          generation,
          tile,
          image,
        });
      }
    });
  }

  fn request_native_vector_tile(
    &mut self,
    tile: Tile,
    priority: TilePriority,
    runtime: tokio::runtime::Handle,
  ) {
    let Some(tile) = self.native_vector_request_tile(tile) else {
      return;
    };
    if self.loaded_native_vector_tiles.contains_key(&tile) || self.in_flight_tiles.contains(&tile) {
      return;
    }
    let Some(tile_loader) = self.tile_loader() else {
      return;
    };
    if tile_loader.tile_type() != TileType::Vector {
      return;
    }

    self.in_flight_tiles.insert(tile);

    let sender = self.sender.clone();
    let tile_source = self.tile_source;
    let generation = self.generation;
    let style_zoom = native_vector_style_zoom(tile.zoom, self.tile_detail_factor);
    runtime.spawn(async move {
      let task_name = format!(
        "bevy-native-vector-tile-{}-{}-{}",
        tile.zoom, tile.x, tile.y
      );
      let _guard = TaskGuard::new(task_name, TaskCategory::TileLoad);

      let Ok(tile_data) = tile_loader
        .tile_data_with_priority(&tile, tile_source, priority)
        .await
      else {
        let _ = sender.send(BevyTileResult::Failed { generation, tile });
        return;
      };

      let (render_rx, _) = crate::render_scheduler::RENDER_SCHEDULER.submit(priority, move || {
        native_vector::native_vector_tile_content(&tile, style_zoom, &tile_data)
      });

      let render_result = tokio::time::timeout(std::time::Duration::from_secs(30), render_rx).await;
      let content = match render_result {
        Ok(Ok(Ok(content))) => content,
        _ => {
          let _ = sender.send(BevyTileResult::Failed { generation, tile });
          return;
        }
      };

      let _ = sender.send(BevyTileResult::NativeVectorReady {
        generation,
        tile,
        meshes: content.meshes,
        labels: content.labels,
        debug_features: content.debug_features,
      });
    });
  }

  fn native_vector_request_tile(&self, mut tile: Tile) -> Option<Tile> {
    let max_zoom = self.tile_loader()?.max_zoom();
    while tile.zoom > max_zoom {
      tile = tile.parent()?;
    }
    Some(tile)
  }

  fn request_preload_tiles(&mut self, visible_tiles: &[Tile], runtime: tokio::runtime::Handle) {
    let Some(tile_loader) = self.tile_loader() else {
      return;
    };
    if !self.preload_enabled || !tile_loader.allows_preloading() {
      return;
    }

    for (tile, priority) in generate_preload_tiles(visible_tiles)
      .into_iter()
      .take(MAX_PRELOAD_TILES)
    {
      self.request_tile(tile, priority, runtime.clone());
    }
  }

  fn request_preload_native_vector_tiles(
    &mut self,
    visible_tiles: &[Tile],
    runtime: tokio::runtime::Handle,
  ) {
    let Some(tile_loader) = self.tile_loader() else {
      return;
    };
    if !self.preload_enabled || !tile_loader.allows_preloading() {
      return;
    }

    for (tile, priority) in generate_preload_tiles(visible_tiles)
      .into_iter()
      .take(MAX_PRELOAD_TILES)
    {
      self.request_native_vector_tile(tile, priority, runtime.clone());
    }
  }
}

fn send_failed_tiles(sender: &Sender<BevyTileResult>, generation: u64, tiles: &[Tile]) {
  for tile in tiles {
    let _ = sender.send(BevyTileResult::Failed {
      generation,
      tile: *tile,
    });
  }
}

fn split_image_into_tiles(image: &TileImage, grid_size: usize) -> Vec<TileImage> {
  let size = image.size[0];
  let tile_size = size / grid_size;
  let mut tiles = Vec::with_capacity(grid_size * grid_size);

  for tile_y in 0..grid_size {
    for tile_x in 0..grid_size {
      let mut tile_rgba = Vec::with_capacity(tile_size * tile_size * 4);

      for y in 0..tile_size {
        for x in 0..tile_size {
          let src_x = tile_x * tile_size + x;
          let src_y = tile_y * tile_size + y;
          let src_idx = (src_y * size + src_x) * 4;
          tile_rgba.extend_from_slice(&image.rgba[src_idx..src_idx + 4]);
        }
      }

      tiles.push(TileImage::from_rgba_unmultiplied(
        [tile_size, tile_size],
        tile_rgba,
      ));
    }
  }

  tiles
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: String) {
  ui.horizontal(|ui| {
    ui.label(label);
    ui.label(value);
  });
}
