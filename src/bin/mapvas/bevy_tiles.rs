use std::{
  collections::{HashMap, HashSet},
  sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
  },
};

use bevy::{
  asset::RenderAssetUsages,
  mesh::{Indices, PrimitiveTopology},
  prelude::*,
  render::render_resource::{Extent3d, TextureDimension, TextureFormat},
  sprite::Anchor,
  window::PrimaryWindow,
};
use mapvas::{
  config::{Config, TileProvider, TileType},
  map::{
    coordinates::{
      CANVAS_SIZE, PixelCoordinate, PixelPosition, PixelRect, Tile, TileCoordinate, TilePriority,
      generate_preload_tiles, tile_zoom_for_transform, tiles_in_box,
    },
    tile_loader::{CachedTileLoader, TileLoader, TileSource},
    tile_renderer::{
      RasterTileRenderer, StyleConfig, TileImage, TileRenderError, TileRenderer,
      VectorTileRenderer, background_color, get_fill_color, get_place_font_size, get_road_styling,
      should_show_place, style_config, style_version,
    },
    viewport::MapViewport,
  },
  task_tracker::{TaskCategory, TaskGuard},
};

use crate::bevy_map::BevyMapViewport;

const MAX_PRELOAD_TILES: usize = 20;
const TILE_LABEL_WIDTH: f32 = 100.0;
const TILE_LABEL_HEIGHT: f32 = 60.0;
const TILE_LABEL_TEXT_OFFSET_X: f32 = 8.0;
const TILE_LABEL_TEXT_OFFSET_Y: f32 = 8.0;
const TILE_LABEL_FONT_SIZE: f32 = 11.0;
const TILE_LABEL_BACKGROUND_Z: f32 = 40.0;
const TILE_LABEL_TEXT_Z: f32 = 41.0;
const OSM_ATTRIBUTION_TEXT: &str = "© OpenStreetMap contributors";
const OSM_ATTRIBUTION_WIDTH: f32 = 158.0;
const OSM_ATTRIBUTION_HEIGHT: f32 = 20.0;
const OSM_ATTRIBUTION_MARGIN: f32 = 8.0;
const OSM_ATTRIBUTION_FONT_SIZE: f32 = 11.0;
const OSM_ATTRIBUTION_BACKGROUND_Z: f32 = 50.0;
const OSM_ATTRIBUTION_TEXT_Z: f32 = 51.0;
const BASE_TILE_DETAIL_FACTOR: f32 = 2.0;
const MIN_TILE_DETAIL_FACTOR: f32 = 0.5;
const MAX_TILE_DETAIL_FACTOR: f32 = 2.0;
const NATIVE_VECTOR_BACKGROUND_Z: f32 = -30.0;
const NATIVE_VECTOR_LAND_Z: f32 = -29.0;
const NATIVE_VECTOR_WATER_Z: f32 = -28.0;
const NATIVE_VECTOR_BUILDING_Z: f32 = -27.0;
const NATIVE_VECTOR_ROAD_CASING_Z: f32 = -26.0;
const NATIVE_VECTOR_ROAD_INNER_Z: f32 = -25.0;
const NATIVE_VECTOR_LABEL_MARKER_Z: f32 = -24.0;
const NATIVE_VECTOR_LABEL_TEXT_Z: f32 = -23.0;

#[derive(Resource)]
pub struct BevyTileRuntime(pub tokio::runtime::Handle);

pub struct BevyTilePlugin;

impl Plugin for BevyTilePlugin {
  fn build(&self, app: &mut App) {
    app
      .init_gizmo_group::<BevyTileOverlayGizmos>()
      .add_systems(Startup, configure_bevy_tile_overlay_gizmos)
      .add_systems(
        Update,
        (
          collect_finished_tiles,
          update_bevy_tiles,
          update_native_vector_tiles,
          update_native_vector_tile_labels,
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
  tile_world_size: f32,
  entities: Vec<NativeVectorTileLabelCopy>,
}

struct BevyNativeVectorTileEntry {
  instances: Vec<NativeVectorTileMeshInstance>,
  labels: Vec<NativeVectorTileLabelInstance>,
}

struct NativeVectorTileContent {
  meshes: Vec<NativeVectorTileMeshSpec>,
  labels: Vec<NativeVectorTileLabelSpec>,
}

struct NativeVectorMeshBatch {
  color_key: [u8; 4],
  color: Color,
  z_key: i16,
  z: f32,
  positions: Vec<[f32; 3]>,
  indices: Vec<u32>,
  bounds: Option<NativeVectorTileBounds>,
}

struct TileLabel {
  background_rect: PixelRect,
  background_size: Vec2,
  text_pos: PixelPosition,
  text: String,
}

type NativeVectorLabelMarkerQuery<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static mut Transform,
    &'static mut Sprite,
    &'static mut Visibility,
  ),
  (
    With<BevyNativeVectorTileLabelMarker>,
    Without<BevyNativeVectorTileLabelText>,
  ),
>;

type NativeVectorLabelTextQuery<'w, 's> = Query<
  'w,
  's,
  (
    Entity,
    &'static mut Transform,
    &'static mut Text2d,
    &'static mut TextFont,
    &'static mut TextColor,
    &'static mut Visibility,
  ),
  (
    With<BevyNativeVectorTileLabelText>,
    Without<BevyNativeVectorTileLabelMarker>,
  ),
>;

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
    });
  }

  fn clear_tiles(&mut self) {
    self.generation = self.generation.wrapping_add(1);
    self.in_flight_tiles.clear();
    self.last_visible_tiles.clear();
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

      let (render_rx, _) = mapvas::render_scheduler::RENDER_SCHEDULER
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

      let (render_rx, _) = mapvas::render_scheduler::RENDER_SCHEDULER.submit(priority, move || {
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

      let (render_rx, _) = mapvas::render_scheduler::RENDER_SCHEDULER.submit(priority, move || {
        native_vector_tile_content(&tile, &tile_data)
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

fn collect_finished_tiles(
  mut layer: ResMut<BevyTileLayer>,
  mut images: ResMut<Assets<Image>>,
  mut meshes: ResMut<Assets<Mesh>>,
  mut materials: ResMut<Assets<ColorMaterial>>,
) {
  let mut results = Vec::new();
  if let Ok(receiver) = layer.receiver.lock() {
    while let Ok(result) = receiver.try_recv() {
      results.push(result);
    }
  }

  for result in results {
    match result {
      BevyTileResult::Ready {
        generation,
        tile,
        image,
      } => {
        if generation != layer.generation {
          continue;
        }
        layer.in_flight_tiles.remove(&tile);
        let image = images.add(tile_image_to_bevy(image));
        if let Some(entry) = layer.loaded_tiles.get_mut(&tile) {
          entry.image = image;
        } else {
          layer.loaded_tiles.insert(
            tile,
            BevyTileEntry {
              image,
              entities: Vec::new(),
            },
          );
        }
      }
      BevyTileResult::NativeVectorReady {
        generation,
        tile,
        meshes: mesh_specs,
        labels: label_specs,
      } => {
        if generation != layer.generation {
          continue;
        }
        layer.in_flight_tiles.remove(&tile);
        let instances = mesh_specs
          .into_iter()
          .map(|spec| NativeVectorTileMeshInstance {
            mesh: meshes.add(spec.mesh),
            material: materials.add(ColorMaterial::from(spec.color)),
            bounds: spec.bounds,
            origin: spec.origin,
            z: spec.z,
            entities: Vec::new(),
          })
          .collect::<Vec<_>>();
        let labels = label_specs
          .into_iter()
          .map(|spec| NativeVectorTileLabelInstance {
            coord: spec.coord,
            text: spec.text,
            base_font_size: spec.base_font_size,
            tile_world_size: spec.tile_world_size,
            entities: Vec::new(),
          })
          .collect::<Vec<_>>();
        if let Some(entry) = layer
          .loaded_native_vector_tiles
          .insert(tile, BevyNativeVectorTileEntry { instances, labels })
        {
          layer.stale_native_vector_entities.extend(
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
      BevyTileResult::Failed { generation, tile } => {
        if generation != layer.generation {
          continue;
        }
        layer.in_flight_tiles.remove(&tile);
      }
    }
  }
}

fn update_bevy_tiles(
  mut commands: Commands,
  mut layer: ResMut<BevyTileLayer>,
  viewport: Res<BevyMapViewport>,
  runtime: Res<BevyTileRuntime>,
  windows: Query<&Window, With<PrimaryWindow>>,
  mut sprites: Query<(Entity, &mut Transform, &mut Sprite, &mut Visibility), With<BevyTileSprite>>,
  mut grid_sprites: Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (With<BevyGridTileSprite>, Without<BevyTileSprite>),
  >,
  mut label_backgrounds: Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyTileLabelBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelText>,
    ),
  >,
  mut label_texts: Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyTileLabelText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
    ),
  >,
  mut attribution_backgrounds: Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyOsmAttributionBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionText>,
    ),
  >,
  mut attribution_texts: Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyOsmAttributionText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionBackground>,
    ),
  >,
  mut overlay_gizmos: Gizmos<BevyTileOverlayGizmos>,
) {
  let stale_entities = layer.stale_entities.drain(..).collect::<HashSet<_>>();
  for entity in &stale_entities {
    commands.entity(*entity).despawn();
  }

  if !layer.visible {
    hide_tile_sprites(&mut sprites);
    despawn_untracked_tile_sprites(
      &mut commands,
      &tracked_tile_entities(&layer),
      &stale_entities,
      &mut sprites,
    );
    hide_grid_tile_sprites(&mut grid_sprites);
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
    hide_osm_attribution(&mut attribution_backgrounds, &mut attribution_texts);
    return;
  }

  let Some(viewport) = viewport.get() else {
    hide_tile_sprites(&mut sprites);
    despawn_untracked_tile_sprites(
      &mut commands,
      &tracked_tile_entities(&layer),
      &stale_entities,
      &mut sprites,
    );
    hide_grid_tile_sprites(&mut grid_sprites);
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
    hide_osm_attribution(&mut attribution_backgrounds, &mut attribution_texts);
    return;
  };
  let Ok(window) = windows.single() else {
    hide_tile_sprites(&mut sprites);
    despawn_untracked_tile_sprites(
      &mut commands,
      &tracked_tile_entities(&layer),
      &stale_entities,
      &mut sprites,
    );
    hide_grid_tile_sprites(&mut grid_sprites);
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
    hide_osm_attribution(&mut attribution_backgrounds, &mut attribution_texts);
    return;
  };

  let visible_tiles = layer.visible_tiles(viewport);
  let runtime_handle = runtime.0.clone();
  let native_vector_active = layer.native_vector_tiles_active();
  let show_osm_attribution = layer
    .tile_loader()
    .is_some_and(|loader| loader.requires_osm_attribution())
    && (!layer.loaded_tiles.is_empty() || !layer.loaded_native_vector_tiles.is_empty())
    && layer.coordinate_display_mode != CoordinateDisplayMode::GridOnly;

  if !native_vector_active {
    for tile in &visible_tiles {
      if !layer.loaded_tiles.contains_key(tile) {
        layer.request_tile(*tile, TilePriority::Current, runtime_handle.clone());
      }
    }
  }

  if layer.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
    if native_vector_active {
      layer.request_preload_native_vector_tiles(&visible_tiles, runtime_handle);
    } else {
      layer.request_preload_tiles(&visible_tiles, runtime_handle);
    }
    hide_tile_sprites(&mut sprites);
    update_grid_tile_sprites(
      &mut commands,
      viewport,
      window,
      &visible_tiles,
      &mut grid_sprites,
    );
    draw_coordinate_tile_borders(
      viewport,
      window,
      &visible_tiles,
      layer.coordinate_display_mode,
      &mut overlay_gizmos,
    );
    update_tile_labels(
      &mut commands,
      viewport,
      window,
      &visible_tiles,
      &mut label_backgrounds,
      &mut label_texts,
    );
    update_osm_attribution(
      &mut commands,
      viewport,
      window,
      show_osm_attribution,
      &mut attribution_backgrounds,
      &mut attribution_texts,
    );
    return;
  }
  hide_grid_tile_sprites(&mut grid_sprites);

  draw_coordinate_tile_borders(
    viewport,
    window,
    &visible_tiles,
    layer.coordinate_display_mode,
    &mut overlay_gizmos,
  );
  if layer.coordinate_display_mode == CoordinateDisplayMode::Off {
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
  } else {
    update_tile_labels(
      &mut commands,
      viewport,
      window,
      &visible_tiles,
      &mut label_backgrounds,
      &mut label_texts,
    );
  }
  update_osm_attribution(
    &mut commands,
    viewport,
    window,
    show_osm_attribution,
    &mut attribution_backgrounds,
    &mut attribution_texts,
  );
  if native_vector_active {
    hide_tile_sprites(&mut sprites);
    return;
  }

  let mut tiles_to_draw = visible_tiles
    .iter()
    .filter_map(|tile| loaded_tile_or_parent(&layer, *tile))
    .collect::<Vec<_>>();
  tiles_to_draw.sort_unstable_by_key(|tile| tile.zoom);
  tiles_to_draw.dedup();
  tiles_to_draw.reverse();

  let draw_set: HashSet<Tile> = tiles_to_draw.iter().copied().collect();
  for tile in &tiles_to_draw {
    let tile = *tile;
    let tile_rects = coordinate_tile_rects(viewport, tile);
    let tint = tile_tint(layer.coordinate_display_mode, tile);

    let entry = layer.loaded_tiles.get_mut(&tile).expect("tile was checked");
    let mut visible_entity_count = 0;

    for tile_rect in tile_rects {
      let transform = transform_for_screen_rect(tile_rect, window, tile.zoom);
      let custom_size = Vec2::new(tile_rect.width(), tile_rect.height());
      let image = entry.image.clone();

      if let Some(entity) = entry.entities.get(visible_entity_count).copied() {
        if let Ok((_, mut sprite_transform, mut sprite, mut visibility)) = sprites.get_mut(entity) {
          *sprite_transform = transform;
          sprite.image = image;
          sprite.custom_size = Some(custom_size);
          sprite.color = tint;
          *visibility = Visibility::Visible;
        } else {
          entry.entities[visible_entity_count] =
            spawn_tile_sprite(&mut commands, image, transform, custom_size, tint);
        }
      } else {
        entry.entities.push(spawn_tile_sprite(
          &mut commands,
          image,
          transform,
          custom_size,
          tint,
        ));
      }

      visible_entity_count += 1;
    }

    for entity in entry.entities.iter().skip(visible_entity_count).copied() {
      if let Ok((_, _, _, mut visibility)) = sprites.get_mut(entity) {
        *visibility = Visibility::Hidden;
      }
    }
  }

  layer.request_preload_tiles(&visible_tiles, runtime_handle);

  for (tile, entry) in &mut layer.loaded_tiles {
    if draw_set.contains(tile) {
      continue;
    }
    for entity in &entry.entities {
      if let Ok((_, _, _, mut visibility)) = sprites.get_mut(*entity) {
        *visibility = Visibility::Hidden;
      }
    }
  }

  despawn_untracked_tile_sprites(
    &mut commands,
    &tracked_tile_entities(&layer),
    &stale_entities,
    &mut sprites,
  );
}

fn update_native_vector_tiles(
  mut commands: Commands,
  mut layer: ResMut<BevyTileLayer>,
  viewport: Res<BevyMapViewport>,
  runtime: Res<BevyTileRuntime>,
  windows: Query<&Window, With<PrimaryWindow>>,
  mut meshes: Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  let stale_entities = layer
    .stale_native_vector_entities
    .drain(..)
    .collect::<HashSet<_>>();
  for entity in &stale_entities {
    commands.entity(*entity).despawn();
  }

  if !layer.native_vector_tiles_active() {
    hide_native_vector_tile_meshes(&mut meshes);
    despawn_untracked_native_vector_tile_meshes(
      &mut commands,
      &tracked_native_vector_tile_entities(&layer),
      &stale_entities,
      &mut meshes,
    );
    return;
  }
  let Some(viewport) = viewport.get() else {
    hide_native_vector_tile_meshes(&mut meshes);
    despawn_untracked_native_vector_tile_meshes(
      &mut commands,
      &tracked_native_vector_tile_entities(&layer),
      &stale_entities,
      &mut meshes,
    );
    return;
  };
  let Ok(window) = windows.single() else {
    hide_native_vector_tile_meshes(&mut meshes);
    despawn_untracked_native_vector_tile_meshes(
      &mut commands,
      &tracked_native_vector_tile_entities(&layer),
      &stale_entities,
      &mut meshes,
    );
    return;
  };

  if layer.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
    hide_native_vector_tile_meshes(&mut meshes);
    return;
  }

  let visible_tiles = layer.visible_tiles(viewport);
  let runtime_handle = runtime.0.clone();
  for tile in &visible_tiles {
    if loaded_native_vector_tile_or_parent(&layer, *tile).is_none() {
      layer.request_native_vector_tile(*tile, TilePriority::Current, runtime_handle.clone());
    }
  }
  layer.request_preload_native_vector_tiles(&visible_tiles, runtime_handle);

  let mut tiles_to_draw = visible_tiles
    .iter()
    .filter_map(|tile| loaded_native_vector_tile_or_parent(&layer, *tile))
    .collect::<Vec<_>>();
  tiles_to_draw.sort_unstable_by_key(|tile| tile.zoom);
  tiles_to_draw.dedup();
  tiles_to_draw.reverse();

  let draw_set: HashSet<Tile> = tiles_to_draw.iter().copied().collect();
  for tile in &tiles_to_draw {
    let entry = layer
      .loaded_native_vector_tiles
      .get_mut(tile)
      .expect("native vector tile was checked");
    for instance in &mut entry.instances {
      update_native_vector_tile_mesh_instance(
        &mut commands,
        viewport,
        window,
        instance,
        &mut meshes,
      );
    }
  }

  for (tile, entry) in &mut layer.loaded_native_vector_tiles {
    if draw_set.contains(tile) {
      continue;
    }
    for instance in &entry.instances {
      hide_native_vector_tile_entities(&instance.entities, &mut meshes);
    }
  }

  despawn_untracked_native_vector_tile_meshes(
    &mut commands,
    &tracked_native_vector_tile_entities(&layer),
    &stale_entities,
    &mut meshes,
  );
}

fn update_native_vector_tile_labels(
  mut commands: Commands,
  mut layer: ResMut<BevyTileLayer>,
  viewport: Res<BevyMapViewport>,
  windows: Query<&Window, With<PrimaryWindow>>,
  mut markers: NativeVectorLabelMarkerQuery<'_, '_>,
  mut texts: NativeVectorLabelTextQuery<'_, '_>,
) {
  if !layer.native_vector_tiles_active() {
    hide_native_vector_tile_labels(&mut markers, &mut texts);
    despawn_untracked_native_vector_tile_labels(
      &mut commands,
      &tracked_native_vector_tile_label_entities(&layer),
      &mut markers,
      &mut texts,
    );
    return;
  }
  let Some(viewport) = viewport.get() else {
    hide_native_vector_tile_labels(&mut markers, &mut texts);
    despawn_untracked_native_vector_tile_labels(
      &mut commands,
      &tracked_native_vector_tile_label_entities(&layer),
      &mut markers,
      &mut texts,
    );
    return;
  };
  let Ok(window) = windows.single() else {
    hide_native_vector_tile_labels(&mut markers, &mut texts);
    despawn_untracked_native_vector_tile_labels(
      &mut commands,
      &tracked_native_vector_tile_label_entities(&layer),
      &mut markers,
      &mut texts,
    );
    return;
  };
  if layer.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
    hide_native_vector_tile_labels(&mut markers, &mut texts);
    despawn_untracked_native_vector_tile_labels(
      &mut commands,
      &tracked_native_vector_tile_label_entities(&layer),
      &mut markers,
      &mut texts,
    );
    return;
  }

  let visible_tiles = if layer.last_visible_tiles.is_empty() {
    layer.visible_tiles(viewport)
  } else {
    layer.last_visible_tiles.clone()
  };
  let mut tiles_to_draw = visible_tiles
    .iter()
    .filter_map(|tile| loaded_native_vector_tile_or_parent(&layer, *tile))
    .collect::<Vec<_>>();
  tiles_to_draw.sort_unstable_by_key(|tile| tile.zoom);
  tiles_to_draw.dedup();
  tiles_to_draw.reverse();

  let cfg = style_config();
  let label_responsibilities = native_vector_label_responsibilities(&layer, &visible_tiles);
  let request_zoom = visible_tiles
    .first()
    .map_or(layer.current_request_zoom, |tile| tile.zoom);
  let draw_set: HashSet<Tile> = tiles_to_draw.iter().copied().collect();
  for tile in &tiles_to_draw {
    let entry = layer
      .loaded_native_vector_tiles
      .get_mut(tile)
      .expect("native vector tile was checked");
    for label in &mut entry.labels {
      if !native_vector_label_is_responsible(label, request_zoom, label_responsibilities.get(tile))
      {
        hide_native_vector_tile_label_entities(&label.entities, &mut markers, &mut texts);
        continue;
      }
      update_native_vector_tile_label_instance(
        &mut commands,
        viewport,
        window,
        label,
        &cfg,
        &mut markers,
        &mut texts,
      );
    }
  }

  for (tile, entry) in &mut layer.loaded_native_vector_tiles {
    if draw_set.contains(tile) {
      continue;
    }
    for label in &entry.labels {
      hide_native_vector_tile_label_entities(&label.entities, &mut markers, &mut texts);
    }
  }

  despawn_untracked_native_vector_tile_labels(
    &mut commands,
    &tracked_native_vector_tile_label_entities(&layer),
    &mut markers,
    &mut texts,
  );
}

fn native_vector_label_responsibilities(
  layer: &BevyTileLayer,
  visible_tiles: &[Tile],
) -> HashMap<Tile, HashSet<Tile>> {
  let mut responsibilities = HashMap::<Tile, HashSet<Tile>>::new();
  for requested_tile in visible_tiles {
    let Some(draw_tile) = loaded_native_vector_tile_or_parent(layer, *requested_tile) else {
      continue;
    };
    responsibilities
      .entry(draw_tile)
      .or_default()
      .insert(*requested_tile);
  }
  responsibilities
}

fn native_vector_label_is_responsible(
  label: &NativeVectorTileLabelInstance,
  request_zoom: u8,
  responsible_tiles: Option<&HashSet<Tile>>,
) -> bool {
  let Some(responsible_tiles) = responsible_tiles else {
    return false;
  };
  let request_tile = native_vector_label_request_tile(label.coord, request_zoom);
  responsible_tiles.contains(&request_tile)
}

fn native_vector_label_request_tile(coord: PixelCoordinate, zoom: u8) -> Tile {
  Tile::from(TileCoordinate::from_pixel_position(
    PixelCoordinate {
      x: coord.x.rem_euclid(CANVAS_SIZE),
      y: coord.y,
    },
    zoom,
  ))
}

fn update_native_vector_tile_mesh_instance(
  commands: &mut Commands,
  viewport: MapViewport,
  window: &Window,
  instance: &mut NativeVectorTileMeshInstance,
  meshes: &mut Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  let wrap_offsets = native_vector_bounds_visible_wrap_offsets(instance.bounds, viewport);
  for (entity_index, wrap_offset) in wrap_offsets.iter().copied().enumerate() {
    let transform =
      native_vector_tile_transform(viewport, window, instance.origin, wrap_offset, instance.z);
    if let Some(entity) = instance.entities.get(entity_index).copied() {
      if let Ok((_, mut mesh_transform, mut visibility)) = meshes.get_mut(entity) {
        *mesh_transform = transform;
        *visibility = Visibility::Visible;
      } else {
        instance.entities[entity_index] =
          spawn_native_vector_tile_mesh(commands, instance, transform);
      }
    } else {
      let entity = spawn_native_vector_tile_mesh(commands, instance, transform);
      instance.entities.push(entity);
    }
  }

  hide_native_vector_tile_entities(&instance.entities[wrap_offsets.len()..], meshes);
}

fn update_native_vector_tile_label_instance(
  commands: &mut Commands,
  viewport: MapViewport,
  window: &Window,
  label: &mut NativeVectorTileLabelInstance,
  cfg: &StyleConfig,
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) {
  let positions = native_vector_label_screen_positions(label.coord, viewport);
  let scale = native_vector_label_scale(viewport, label);
  let font_size = native_vector_label_font_size(label.base_font_size, scale, cfg);
  let radius = native_vector_label_marker_radius(scale, cfg);
  let marker_size = Vec2::splat(radius * 2.0);
  let marker_color = native_vector_color(cfg.marker_dot.to_color());
  let text_color = native_vector_color(cfg.place_label.to_color());
  let text_offset_x = radius + cfg.markers.text_offset_x * scale;

  for (entity_index, screen_pos) in positions.iter().copied().enumerate() {
    let bevy_pos = screen_to_bevy_2d(screen_pos, window);
    let marker_transform =
      Transform::from_xyz(bevy_pos.x, bevy_pos.y, NATIVE_VECTOR_LABEL_MARKER_Z);
    let text_transform = Transform::from_xyz(
      bevy_pos.x + text_offset_x,
      bevy_pos.y,
      NATIVE_VECTOR_LABEL_TEXT_Z,
    );
    let existing = label.entities.get(entity_index).copied();
    let marker = update_native_vector_tile_label_marker(
      commands,
      existing.map(|copy| copy.marker),
      marker_transform,
      marker_color,
      marker_size,
      markers,
    );
    let text = update_native_vector_tile_label_text(
      commands,
      existing.map(|copy| copy.text),
      text_transform,
      &label.text,
      font_size,
      text_color,
      texts,
    );

    if let Some(copy) = label.entities.get_mut(entity_index) {
      copy.marker = marker;
      copy.text = text;
    } else {
      label
        .entities
        .push(NativeVectorTileLabelCopy { marker, text });
    }
  }

  hide_native_vector_tile_label_entities(&label.entities[positions.len()..], markers, texts);
}

fn update_native_vector_tile_label_marker(
  commands: &mut Commands,
  entity: Option<Entity>,
  transform: Transform,
  color: Color,
  size: Vec2,
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
) -> Entity {
  if let Some(entity) = entity {
    if let Ok((_, mut marker_transform, mut sprite, mut visibility)) = markers.get_mut(entity) {
      *marker_transform = transform;
      sprite.custom_size = Some(size);
      sprite.color = color;
      *visibility = Visibility::Visible;
      return entity;
    }
    commands.entity(entity).try_despawn();
  }

  commands
    .spawn((
      Sprite::from_color(color, size),
      transform,
      BevyNativeVectorTileLabelMarker,
    ))
    .id()
}

fn update_native_vector_tile_label_text(
  commands: &mut Commands,
  entity: Option<Entity>,
  transform: Transform,
  value: &str,
  font_size: f32,
  color: Color,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) -> Entity {
  if let Some(entity) = entity {
    if let Ok((_, mut text_transform, mut text, mut font, mut text_color, mut visibility)) =
      texts.get_mut(entity)
    {
      *text_transform = transform;
      text.0.clear();
      text.0.push_str(value);
      font.font_size = font_size;
      text_color.0 = color;
      *visibility = Visibility::Visible;
      return entity;
    }
    commands.entity(entity).try_despawn();
  }

  commands
    .spawn((
      Text2d::new(value.to_string()),
      TextFont::from_font_size(font_size),
      TextColor(color),
      TextLayout::new_with_justify(Justify::Left),
      Anchor::CENTER_LEFT,
      transform,
      BevyNativeVectorTileLabelText,
    ))
    .id()
}

fn spawn_native_vector_tile_mesh(
  commands: &mut Commands,
  instance: &NativeVectorTileMeshInstance,
  transform: Transform,
) -> Entity {
  commands
    .spawn((
      Mesh2d(instance.mesh.clone()),
      MeshMaterial2d(instance.material.clone()),
      transform,
      BevyNativeVectorTileMesh,
    ))
    .id()
}

fn hide_native_vector_tile_meshes(
  meshes: &mut Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  for (_, _, mut visibility) in meshes.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn hide_native_vector_tile_entities(
  entities: &[Entity],
  meshes: &mut Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  for entity in entities {
    if let Ok((_, _, mut visibility)) = meshes.get_mut(*entity) {
      *visibility = Visibility::Hidden;
    }
  }
}

fn hide_native_vector_tile_labels(
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) {
  for (_, _, _, mut visibility) in markers.iter_mut() {
    *visibility = Visibility::Hidden;
  }
  for (_, _, _, _, _, mut visibility) in texts.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn hide_native_vector_tile_label_entities(
  entities: &[NativeVectorTileLabelCopy],
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) {
  for copy in entities {
    if let Ok((_, _, _, mut visibility)) = markers.get_mut(copy.marker) {
      *visibility = Visibility::Hidden;
    }
    if let Ok((_, _, _, _, _, mut visibility)) = texts.get_mut(copy.text) {
      *visibility = Visibility::Hidden;
    }
  }
}

fn tracked_native_vector_tile_label_entities(layer: &BevyTileLayer) -> HashSet<Entity> {
  layer
    .loaded_native_vector_tiles
    .values()
    .flat_map(|entry| {
      entry.labels.iter().flat_map(|label| {
        label
          .entities
          .iter()
          .flat_map(|copy| [copy.marker, copy.text])
      })
    })
    .collect()
}

fn tracked_native_vector_tile_entities(layer: &BevyTileLayer) -> HashSet<Entity> {
  layer
    .loaded_native_vector_tiles
    .values()
    .flat_map(|entry| {
      entry
        .instances
        .iter()
        .flat_map(|instance| instance.entities.iter().copied())
    })
    .collect()
}

fn despawn_untracked_native_vector_tile_meshes(
  commands: &mut Commands,
  tracked_entities: &HashSet<Entity>,
  stale_entities: &HashSet<Entity>,
  meshes: &mut Query<(Entity, &mut Transform, &mut Visibility), With<BevyNativeVectorTileMesh>>,
) {
  for (entity, _, mut visibility) in meshes.iter_mut() {
    if tracked_entities.contains(&entity) || stale_entities.contains(&entity) {
      continue;
    }
    *visibility = Visibility::Hidden;
    commands.entity(entity).despawn();
  }
}

fn despawn_untracked_native_vector_tile_labels(
  commands: &mut Commands,
  tracked_entities: &HashSet<Entity>,
  markers: &mut NativeVectorLabelMarkerQuery<'_, '_>,
  texts: &mut NativeVectorLabelTextQuery<'_, '_>,
) {
  for (entity, _, _, mut visibility) in markers.iter_mut() {
    if tracked_entities.contains(&entity) {
      continue;
    }
    *visibility = Visibility::Hidden;
    commands.entity(entity).try_despawn();
  }
  for (entity, _, _, _, _, mut visibility) in texts.iter_mut() {
    if tracked_entities.contains(&entity) {
      continue;
    }
    *visibility = Visibility::Hidden;
    commands.entity(entity).try_despawn();
  }
}

fn hide_tile_sprites(
  sprites: &mut Query<(Entity, &mut Transform, &mut Sprite, &mut Visibility), With<BevyTileSprite>>,
) {
  for (_, _, _, mut visibility) in sprites.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn tracked_tile_entities(layer: &BevyTileLayer) -> HashSet<Entity> {
  layer
    .loaded_tiles
    .values()
    .flat_map(|entry| entry.entities.iter().copied())
    .collect()
}

fn spawn_tile_sprite(
  commands: &mut Commands,
  image: Handle<Image>,
  transform: Transform,
  custom_size: Vec2,
  tint: Color,
) -> Entity {
  let mut sprite = Sprite::from_image(image);
  sprite.custom_size = Some(custom_size);
  sprite.color = tint;
  commands.spawn((sprite, transform, BevyTileSprite)).id()
}

fn despawn_untracked_tile_sprites(
  commands: &mut Commands,
  tracked_entities: &HashSet<Entity>,
  stale_entities: &HashSet<Entity>,
  sprites: &mut Query<(Entity, &mut Transform, &mut Sprite, &mut Visibility), With<BevyTileSprite>>,
) {
  for (entity, _, _, mut visibility) in sprites.iter_mut() {
    if tracked_entities.contains(&entity) || stale_entities.contains(&entity) {
      continue;
    }
    *visibility = Visibility::Hidden;
    commands.entity(entity).despawn();
  }
}

fn hide_grid_tile_sprites(
  sprites: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (With<BevyGridTileSprite>, Without<BevyTileSprite>),
  >,
) {
  for (_, _, mut visibility) in sprites.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn hide_tile_labels(
  backgrounds: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyTileLabelBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelText>,
    ),
  >,
  texts: &mut Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyTileLabelText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
    ),
  >,
) {
  for (_, _, mut visibility) in backgrounds.iter_mut() {
    *visibility = Visibility::Hidden;
  }
  for (_, _, mut visibility) in texts.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn hide_osm_attribution(
  backgrounds: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyOsmAttributionBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionText>,
    ),
  >,
  texts: &mut Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyOsmAttributionText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionBackground>,
    ),
  >,
) {
  for (_, _, mut visibility) in backgrounds.iter_mut() {
    *visibility = Visibility::Hidden;
  }
  for (_, _, mut visibility) in texts.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn update_grid_tile_sprites(
  commands: &mut Commands,
  viewport: MapViewport,
  window: &Window,
  visible_tiles: &[Tile],
  sprites: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (With<BevyGridTileSprite>, Without<BevyTileSprite>),
  >,
) {
  let mut sprite_iter = sprites.iter_mut();
  for tile in visible_tiles {
    for tile_rect in coordinate_tile_rects(viewport, *tile) {
      let transform = transform_for_screen_rect(tile_rect, window, tile.zoom);
      let custom_size = Vec2::new(tile_rect.width(), tile_rect.height());
      let color = grid_tile_color(*tile);

      if let Some((mut sprite_transform, mut sprite, mut visibility)) = sprite_iter.next() {
        *sprite_transform = transform;
        sprite.custom_size = Some(custom_size);
        sprite.color = color;
        *visibility = Visibility::Visible;
      } else {
        commands.spawn((
          Sprite::from_color(color, custom_size),
          transform,
          BevyGridTileSprite,
        ));
      }
    }
  }

  for (_, _, mut visibility) in sprite_iter {
    *visibility = Visibility::Hidden;
  }
}

fn update_tile_labels(
  commands: &mut Commands,
  viewport: MapViewport,
  window: &Window,
  visible_tiles: &[Tile],
  backgrounds: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyTileLabelBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelText>,
    ),
  >,
  texts: &mut Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyTileLabelText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
    ),
  >,
) {
  let mut background_iter = backgrounds.iter_mut();
  let mut text_iter = texts.iter_mut();
  for tile in visible_tiles {
    for tile_rect in coordinate_tile_rects(viewport, *tile) {
      let Some(label) = tile_label(*tile, tile_rect) else {
        continue;
      };

      if let Some((mut transform, mut sprite, mut visibility)) = background_iter.next() {
        *transform =
          transform_for_screen_rect_at_z(label.background_rect, window, TILE_LABEL_BACKGROUND_Z);
        sprite.custom_size = Some(label.background_size);
        sprite.color = tile_label_background_color();
        *visibility = Visibility::Visible;
      } else {
        commands.spawn((
          Sprite::from_color(tile_label_background_color(), label.background_size),
          transform_for_screen_rect_at_z(label.background_rect, window, TILE_LABEL_BACKGROUND_Z),
          BevyTileLabelBackground,
        ));
      }

      if let Some((mut transform, mut text, mut visibility)) = text_iter.next() {
        *transform = transform_for_screen_pos(label.text_pos, window, TILE_LABEL_TEXT_Z);
        text.0 = label.text;
        *visibility = Visibility::Visible;
      } else {
        commands.spawn((
          Text2d::new(label.text),
          TextFont::from_font_size(TILE_LABEL_FONT_SIZE),
          TextColor(Color::WHITE),
          TextLayout::new_with_justify(Justify::Left),
          Anchor::TOP_LEFT,
          transform_for_screen_pos(label.text_pos, window, TILE_LABEL_TEXT_Z),
          BevyTileLabelText,
        ));
      }
    }
  }

  for (_, _, mut visibility) in background_iter {
    *visibility = Visibility::Hidden;
  }
  for (_, _, mut visibility) in text_iter {
    *visibility = Visibility::Hidden;
  }
}

fn update_osm_attribution(
  commands: &mut Commands,
  viewport: MapViewport,
  window: &Window,
  visible: bool,
  backgrounds: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyOsmAttributionBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionText>,
    ),
  >,
  texts: &mut Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyOsmAttributionText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionBackground>,
    ),
  >,
) {
  if !visible {
    hide_osm_attribution(backgrounds, texts);
    return;
  }

  let rect = osm_attribution_rect(viewport.rect);
  let size = Vec2::new(OSM_ATTRIBUTION_WIDTH, OSM_ATTRIBUTION_HEIGHT);

  let mut background_iter = backgrounds.iter_mut();
  if let Some((mut transform, mut sprite, mut visibility)) = background_iter.next() {
    *transform = transform_for_screen_rect_at_z(rect, window, OSM_ATTRIBUTION_BACKGROUND_Z);
    sprite.custom_size = Some(size);
    sprite.color = osm_attribution_background_color();
    *visibility = Visibility::Visible;
  } else {
    commands.spawn((
      Sprite::from_color(osm_attribution_background_color(), size),
      transform_for_screen_rect_at_z(rect, window, OSM_ATTRIBUTION_BACKGROUND_Z),
      BevyOsmAttributionBackground,
    ));
  }
  for (_, _, mut visibility) in background_iter {
    *visibility = Visibility::Hidden;
  }

  let mut text_iter = texts.iter_mut();
  if let Some((mut transform, mut text, mut visibility)) = text_iter.next() {
    *transform = transform_for_screen_rect_at_z(rect, window, OSM_ATTRIBUTION_TEXT_Z);
    text.0 = OSM_ATTRIBUTION_TEXT.to_string();
    *visibility = Visibility::Visible;
  } else {
    commands.spawn((
      Text2d::new(OSM_ATTRIBUTION_TEXT),
      TextFont::from_font_size(OSM_ATTRIBUTION_FONT_SIZE),
      TextColor(Color::srgb(35.0 / 255.0, 35.0 / 255.0, 35.0 / 255.0)),
      TextLayout::new_with_justify(Justify::Center),
      transform_for_screen_rect_at_z(rect, window, OSM_ATTRIBUTION_TEXT_Z),
      BevyOsmAttributionText,
    ));
  }
  for (_, _, mut visibility) in text_iter {
    *visibility = Visibility::Hidden;
  }
}

fn grid_tile_color(tile: Tile) -> Color {
  if (tile.x + tile.y).is_multiple_of(2) {
    Color::srgba(1.0, 240.0 / 255.0, 240.0 / 255.0, 120.0 / 255.0)
  } else {
    Color::srgba(240.0 / 255.0, 240.0 / 255.0, 1.0, 120.0 / 255.0)
  }
}

fn tile_label(tile: Tile, tile_rect: PixelRect) -> Option<TileLabel> {
  if tile_rect.width() <= TILE_LABEL_WIDTH || tile_rect.height() <= TILE_LABEL_HEIGHT {
    return None;
  }

  let background_size = Vec2::new(TILE_LABEL_WIDTH, TILE_LABEL_HEIGHT);
  let background_min = PixelPosition {
    x: tile_rect.center().x - TILE_LABEL_WIDTH * 0.5,
    y: tile_rect.center().y - TILE_LABEL_HEIGHT * 0.5,
  };
  let background_rect = PixelRect::from_min_size(
    background_min,
    PixelPosition {
      x: TILE_LABEL_WIDTH,
      y: TILE_LABEL_HEIGHT,
    },
  );
  let text_pos = PixelPosition {
    x: background_rect.min.x + TILE_LABEL_TEXT_OFFSET_X,
    y: background_rect.min.y + TILE_LABEL_TEXT_OFFSET_Y,
  };
  Some(TileLabel {
    background_rect,
    background_size,
    text_pos,
    text: format!("Z:{}\nX:{}\nY:{}", tile.zoom, tile.x, tile.y),
  })
}

fn tile_label_background_color() -> Color {
  Color::srgba(0.0, 0.0, 0.0, 180.0 / 255.0)
}

fn osm_attribution_rect(clip_rect: PixelRect) -> PixelRect {
  PixelRect::from_min_size(
    PixelPosition {
      x: clip_rect.max.x - OSM_ATTRIBUTION_WIDTH - OSM_ATTRIBUTION_MARGIN,
      y: clip_rect.max.y - OSM_ATTRIBUTION_HEIGHT - OSM_ATTRIBUTION_MARGIN,
    },
    PixelPosition {
      x: OSM_ATTRIBUTION_WIDTH,
      y: OSM_ATTRIBUTION_HEIGHT,
    },
  )
}

fn osm_attribution_background_color() -> Color {
  Color::srgba(1.0, 1.0, 1.0, 210.0 / 255.0)
}

fn draw_coordinate_tile_borders(
  viewport: MapViewport,
  window: &Window,
  visible_tiles: &[Tile],
  mode: CoordinateDisplayMode,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  if mode == CoordinateDisplayMode::Off {
    return;
  }

  let color = tile_border_color(mode);
  for tile in visible_tiles {
    for tile_rect in coordinate_tile_rects(viewport, *tile) {
      draw_tile_border(tile_rect, window, color, gizmos);
    }
  }
}

fn draw_tile_border(
  rect: PixelRect,
  window: &Window,
  color: Color,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  let min = rect.min;
  let max = rect.max;
  let top_left = screen_to_bevy_2d(min, window);
  let top_right = screen_to_bevy_2d(PixelPosition { x: max.x, y: min.y }, window);
  let bottom_right = screen_to_bevy_2d(max, window);
  let bottom_left = screen_to_bevy_2d(PixelPosition { x: min.x, y: max.y }, window);

  gizmos.line_2d(top_left, top_right, color);
  gizmos.line_2d(top_right, bottom_right, color);
  gizmos.line_2d(bottom_right, bottom_left, color);
  gizmos.line_2d(bottom_left, top_left, color);
}

fn tile_border_color(mode: CoordinateDisplayMode) -> Color {
  if mode == CoordinateDisplayMode::GridOnly {
    Color::srgb(80.0 / 255.0, 80.0 / 255.0, 80.0 / 255.0)
  } else {
    Color::srgb(100.0 / 255.0, 100.0 / 255.0, 100.0 / 255.0)
  }
}

fn loaded_tile_or_parent(layer: &BevyTileLayer, mut tile: Tile) -> Option<Tile> {
  loop {
    if layer.loaded_tiles.contains_key(&tile) {
      return Some(tile);
    }
    tile = tile.parent()?;
  }
}

fn loaded_native_vector_tile_or_parent(layer: &BevyTileLayer, mut tile: Tile) -> Option<Tile> {
  loop {
    if layer.loaded_native_vector_tiles.contains_key(&tile) {
      return Some(tile);
    }
    tile = tile.parent()?;
  }
}

fn tile_image_to_bevy(image: TileImage) -> Image {
  Image::new(
    Extent3d {
      width: image.size[0] as u32,
      height: image.size[1] as u32,
      depth_or_array_layers: 1,
    },
    TextureDimension::D2,
    image.rgba,
    TextureFormat::Rgba8UnormSrgb,
    RenderAssetUsages::RENDER_WORLD,
  )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn native_vector_bounds_visible_wrap_offsets(
  bounds: NativeVectorTileBounds,
  viewport: MapViewport,
) -> Vec<f32> {
  let inv = viewport.transform.invert();
  let viewport_start = inv.apply(viewport.min());
  let viewport_end = inv.apply(viewport.max());
  let viewport_min_y = viewport_start.y.min(viewport_end.y);
  let viewport_max_y = viewport_start.y.max(viewport_end.y);
  if bounds.max_y < viewport_min_y || bounds.min_y > viewport_max_y {
    return Vec::new();
  }

  let viewport_min_x = viewport_start.x.min(viewport_end.x);
  let viewport_max_x = viewport_start.x.max(viewport_end.x);
  let min_copy = ((viewport_min_x - bounds.max_x) / CANVAS_SIZE - 1e-6).ceil() as i32;
  let max_copy = ((viewport_max_x - bounds.min_x) / CANVAS_SIZE + 1e-6).floor() as i32;
  if min_copy > max_copy {
    return Vec::new();
  }

  (min_copy..=max_copy)
    .map(|copy| copy as f32 * CANVAS_SIZE)
    .collect()
}

fn native_vector_tile_transform(
  viewport: MapViewport,
  window: &Window,
  origin: PixelCoordinate,
  wrap_offset: f32,
  z: f32,
) -> Transform {
  Transform::from_xyz(
    viewport.transform.trans.x + (origin.x + wrap_offset) * viewport.transform.zoom
      - window.width() / 2.0,
    window.height() / 2.0 - viewport.transform.trans.y - origin.y * viewport.transform.zoom,
    z,
  )
  .with_scale(Vec3::new(
    viewport.transform.zoom,
    -viewport.transform.zoom,
    1.0,
  ))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn native_vector_label_screen_positions(
  coord: PixelCoordinate,
  viewport: MapViewport,
) -> Vec<PixelPosition> {
  let inv = viewport.transform.invert();
  let viewport_start = inv.apply(viewport.min());
  let viewport_end = inv.apply(viewport.max());
  let viewport_min_y = viewport_start.y.min(viewport_end.y);
  let viewport_max_y = viewport_start.y.max(viewport_end.y);
  if coord.y < viewport_min_y || coord.y > viewport_max_y {
    return Vec::new();
  }

  let viewport_min_x = viewport_start.x.min(viewport_end.x);
  let viewport_max_x = viewport_start.x.max(viewport_end.x);
  let min_copy = ((viewport_min_x - coord.x) / CANVAS_SIZE - 1e-6).ceil() as i32;
  let max_copy = ((viewport_max_x - coord.x) / CANVAS_SIZE + 1e-6).floor() as i32;
  if min_copy > max_copy {
    return Vec::new();
  }

  let mut positions = Vec::with_capacity((max_copy - min_copy + 1) as usize);
  for copy in min_copy..=max_copy {
    let shifted = PixelCoordinate {
      x: coord.x + copy as f32 * CANVAS_SIZE,
      y: coord.y,
    };
    let screen = viewport.transform.apply(shifted);
    if viewport.rect.contains(screen) {
      positions.push(screen);
    }
  }
  positions
}

fn native_vector_label_scale(viewport: MapViewport, label: &NativeVectorTileLabelInstance) -> f32 {
  (label.tile_world_size * viewport.transform.zoom / 256.0).max(0.0)
}

fn native_vector_label_font_size(base_font_size: f32, scale: f32, cfg: &StyleConfig) -> f32 {
  (base_font_size * scale)
    .max(1.0)
    .min(cfg.font_sizes.max_font_size)
}

fn native_vector_label_marker_radius(scale: f32, cfg: &StyleConfig) -> f32 {
  let max_radius = cfg.markers.max_radius.max(cfg.markers.base_radius);
  (cfg.markers.base_radius * scale).clamp(0.0, max_radius)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn coordinate_tile_rects(viewport: MapViewport, tile: Tile) -> Vec<PixelRect> {
  let (nw, se) = tile.position();
  let inv = viewport.transform.invert();
  let viewport_start = inv.apply(viewport.min());
  let viewport_end = inv.apply(viewport.max());
  let viewport_min_x = viewport_start.x.min(viewport_end.x);
  let viewport_max_x = viewport_start.x.max(viewport_end.x);
  let min_copy = ((viewport_min_x - se.x) / CANVAS_SIZE - 1e-6).ceil() as i32;
  let max_copy = ((viewport_max_x - nw.x) / CANVAS_SIZE + 1e-6).floor() as i32;

  if min_copy > max_copy {
    return Vec::new();
  }

  let mut tile_rects = Vec::with_capacity((max_copy - min_copy + 1) as usize);
  for copy in min_copy..=max_copy {
    let offset = copy as f32 * CANVAS_SIZE;
    let nw_shifted = PixelCoordinate {
      x: nw.x + offset,
      y: nw.y,
    };
    let se_shifted = PixelCoordinate {
      x: se.x + offset,
      y: se.y,
    };
    let (nw_screen, se_screen) = (
      viewport.transform.apply(nw_shifted),
      viewport.transform.apply(se_shifted),
    );
    let tile_rect = PixelRect::from_min_max(nw_screen, se_screen);
    if tile_rect.intersects(viewport.rect) {
      tile_rects.push(tile_rect);
    }
  }

  tile_rects
}

fn transform_for_screen_rect(rect: PixelRect, window: &Window, tile_zoom: u8) -> Transform {
  let z = -10.0 + f32::from(tile_zoom) * 0.001;
  transform_for_screen_rect_at_z(rect, window, z)
}

fn transform_for_screen_rect_at_z(rect: PixelRect, window: &Window, z: f32) -> Transform {
  let center = rect.center();
  transform_for_screen_pos(center, window, z)
}

fn transform_for_screen_pos(pos: PixelPosition, window: &Window, z: f32) -> Transform {
  let bevy_pos = screen_to_bevy_2d(pos, window);
  Transform::from_xyz(bevy_pos.x, bevy_pos.y, z)
}

fn screen_to_bevy_2d(screen: PixelPosition, window: &Window) -> Vec2 {
  Vec2::new(
    screen.x - window.width() / 2.0,
    window.height() / 2.0 - screen.y,
  )
}

fn tile_tint(mode: CoordinateDisplayMode, tile: Tile) -> Color {
  if mode != CoordinateDisplayMode::Overlay {
    return Color::WHITE;
  }

  if (tile.x + tile.y).is_multiple_of(2) {
    Color::srgba(1.0, 240.0 / 255.0, 240.0 / 255.0, 1.0)
  } else {
    Color::srgba(240.0 / 255.0, 240.0 / 255.0, 1.0, 1.0)
  }
}

fn tile_zoom_with_detail_factor(
  transform: mapvas::map::coordinates::Transform,
  detail_factor: f32,
) -> u8 {
  let mut detail_transform = transform;
  detail_transform.zoom *= effective_tile_detail_factor(detail_factor);
  tile_zoom_for_transform(&detail_transform)
}

fn effective_tile_detail_factor(detail_factor: f32) -> f32 {
  BASE_TILE_DETAIL_FACTOR * clamped_tile_detail_factor(detail_factor)
}

fn clamped_tile_detail_factor(detail_factor: f32) -> f32 {
  if detail_factor.is_finite() {
    detail_factor.clamp(MIN_TILE_DETAIL_FACTOR, MAX_TILE_DETAIL_FACTOR)
  } else {
    1.0
  }
}

fn tile_detail_factor_label(detail_factor: f32) -> String {
  format!("{:.2}x", clamped_tile_detail_factor(detail_factor))
}

fn effective_tile_detail_factor_label(detail_factor: f32) -> String {
  format!("{:.2}x", effective_tile_detail_factor(detail_factor))
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn native_vector_tile_content(
  tile: &Tile,
  data: &[u8],
) -> Result<NativeVectorTileContent, TileRenderError> {
  let reader = mvt_reader::Reader::new(data.to_vec())
    .map_err(|e| TileRenderError::ParseError(format!("Failed to parse MVT for {tile:?}: {e}")))?;
  let layer_names = reader
    .get_layer_names()
    .map_err(|e| TileRenderError::ParseError(format!("Failed to get layers: {e}")))?;

  let cfg = style_config();
  let (tile_nw, tile_se) = tile.position();
  let tile_world_size = tile_se.x - tile_nw.x;
  let mvt_to_world = tile_world_size / cfg.rendering.mvt_extent;
  let style_pixel_to_world = tile_world_size / cfg.rendering.tile_size as f32;

  let mut batches = Vec::new();
  let mut labels = Vec::new();
  append_native_vector_rect(
    &mut batches,
    background_color(),
    NATIVE_VECTOR_BACKGROUND_Z,
    tile_nw,
    tile_world_size,
    tile_world_size,
  );

  let layer_order = [
    "landcover",
    "landuse",
    "park",
    "water",
    "waterway",
    "building",
    "buildings",
    "transportation",
    "road",
    "highway",
  ];

  for layer_name in &layer_order {
    if let Some(index) = layer_names.iter().position(|name| name == *layer_name) {
      append_native_vector_layer(
        &mut batches,
        &mut labels,
        &reader,
        index,
        layer_name,
        tile,
        tile_nw,
        tile_world_size,
        mvt_to_world,
        style_pixel_to_world,
      );
    }
  }

  for (index, layer_name) in layer_names.iter().enumerate() {
    if layer_order.contains(&layer_name.as_str()) {
      continue;
    }
    append_native_vector_layer(
      &mut batches,
      &mut labels,
      &reader,
      index,
      layer_name,
      tile,
      tile_nw,
      tile_world_size,
      mvt_to_world,
      style_pixel_to_world,
    );
  }

  Ok(NativeVectorTileContent {
    meshes: native_vector_batches_to_mesh_specs(batches, tile_nw),
    labels,
  })
}

#[allow(clippy::too_many_arguments)]
fn append_native_vector_layer(
  batches: &mut Vec<NativeVectorMeshBatch>,
  labels: &mut Vec<NativeVectorTileLabelSpec>,
  reader: &mvt_reader::Reader,
  layer_index: usize,
  layer_name: &str,
  tile: &Tile,
  tile_origin: PixelCoordinate,
  tile_world_size: f32,
  mvt_to_world: f32,
  style_pixel_to_world: f32,
) {
  let Ok(features) = reader.get_features(layer_index) else {
    return;
  };

  for feature in features {
    let feature_class = feature_string_property(&feature, &["class", "type"]).unwrap_or("");
    let feature_kind = feature_string_property(&feature, &["kind"]);
    let feature_kind_detail = feature_string_property(&feature, &["kind_detail"]);
    let feature_name = feature_string_property(&feature, &["name", "name:en", "name_en"]);
    let population_rank = feature_i64_property(&feature, "population_rank");
    let is_capital = feature_has_property(&feature, "capital");

    match &feature.geometry {
      geo_types::Geometry::Polygon(polygon) => {
        let polygon_class = polygon_feature_class(layer_name, feature_class, feature_kind);
        append_native_vector_polygon(
          batches,
          polygon,
          layer_name,
          polygon_class,
          tile_origin,
          mvt_to_world,
        );
      }
      geo_types::Geometry::MultiPolygon(multi) => {
        let polygon_class = polygon_feature_class(layer_name, feature_class, feature_kind);
        for polygon in multi.iter() {
          append_native_vector_polygon(
            batches,
            polygon,
            layer_name,
            polygon_class,
            tile_origin,
            mvt_to_world,
          );
        }
      }
      geo_types::Geometry::LineString(line) => {
        let line_class =
          line_feature_class(layer_name, feature_class, feature_kind, feature_kind_detail);
        append_native_vector_line(
          batches,
          line,
          layer_name,
          line_class,
          tile.zoom,
          tile_origin,
          mvt_to_world,
          style_pixel_to_world,
        );
      }
      geo_types::Geometry::MultiLineString(multi) => {
        let line_class =
          line_feature_class(layer_name, feature_class, feature_kind, feature_kind_detail);
        for line in multi.iter() {
          append_native_vector_line(
            batches,
            line,
            layer_name,
            line_class,
            tile.zoom,
            tile_origin,
            mvt_to_world,
            style_pixel_to_world,
          );
        }
      }
      geo_types::Geometry::Point(point) => {
        append_native_vector_label(
          labels,
          point,
          layer_name,
          feature_kind,
          feature_kind_detail,
          feature_name,
          population_rank,
          is_capital,
          tile.zoom,
          tile_origin,
          tile_world_size,
          mvt_to_world,
        );
      }
      geo_types::Geometry::MultiPoint(multi) => {
        for point in multi.iter() {
          append_native_vector_label(
            labels,
            point,
            layer_name,
            feature_kind,
            feature_kind_detail,
            feature_name,
            population_rank,
            is_capital,
            tile.zoom,
            tile_origin,
            tile_world_size,
            mvt_to_world,
          );
        }
      }
      _ => {}
    }
  }
}

fn feature_string_property<'a>(
  feature: &'a mvt_reader::feature::Feature,
  keys: &[&str],
) -> Option<&'a str> {
  let properties = feature.properties.as_ref()?;
  keys.iter().find_map(|key| match properties.get(*key) {
    Some(mvt_reader::feature::Value::String(value)) => Some(value.as_str()),
    _ => None,
  })
}

fn feature_i64_property(feature: &mvt_reader::feature::Feature, key: &str) -> Option<i64> {
  let properties = feature.properties.as_ref()?;
  match properties.get(key)? {
    mvt_reader::feature::Value::UInt(value) => i64::try_from(*value).ok(),
    mvt_reader::feature::Value::Int(value) | mvt_reader::feature::Value::SInt(value) => {
      Some(*value)
    }
    _ => None,
  }
}

fn feature_has_property(feature: &mvt_reader::feature::Feature, key: &str) -> bool {
  feature
    .properties
    .as_ref()
    .is_some_and(|properties| properties.contains_key(key))
}

fn polygon_feature_class<'a>(
  layer_name: &str,
  feature_class: &'a str,
  feature_kind: Option<&'a str>,
) -> &'a str {
  if layer_name == "landcover" || layer_name == "landuse" {
    feature_kind.unwrap_or(feature_class)
  } else {
    feature_class
  }
}

fn line_feature_class<'a>(
  layer_name: &str,
  feature_class: &'a str,
  feature_kind: Option<&'a str>,
  feature_kind_detail: Option<&'a str>,
) -> &'a str {
  if matches!(layer_name, "road" | "roads" | "transportation" | "highway") {
    feature_kind_detail.unwrap_or(feature_kind.unwrap_or(feature_class))
  } else {
    feature_class
  }
}

#[allow(clippy::too_many_arguments)]
fn append_native_vector_label(
  labels: &mut Vec<NativeVectorTileLabelSpec>,
  point: &geo_types::Point<f32>,
  layer_name: &str,
  feature_kind: Option<&str>,
  feature_kind_detail: Option<&str>,
  feature_name: Option<&str>,
  population_rank: Option<i64>,
  is_capital: bool,
  tile_zoom: u8,
  tile_origin: PixelCoordinate,
  tile_world_size: f32,
  mvt_to_world: f32,
) {
  let Some(label_text) = feature_name else {
    return;
  };
  let label_text = label_text.trim();
  if label_text.is_empty() {
    return;
  }

  if layer_name == "water" || feature_kind == Some("ocean") || feature_kind == Some("sea") {
    return;
  }
  if !should_show_place(feature_kind_detail, population_rank, is_capital, tile_zoom) {
    return;
  }

  labels.push(NativeVectorTileLabelSpec {
    coord: PixelCoordinate {
      x: tile_origin.x + point.x() * mvt_to_world,
      y: tile_origin.y + point.y() * mvt_to_world,
    },
    text: label_text.to_string(),
    base_font_size: get_place_font_size(feature_kind_detail, is_capital, population_rank),
    tile_world_size,
  });
}

fn append_native_vector_polygon(
  batches: &mut Vec<NativeVectorMeshBatch>,
  polygon: &geo_types::Polygon<f32>,
  layer_name: &str,
  feature_class: &str,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
) {
  let Some(color) = get_fill_color(layer_name, feature_class) else {
    return;
  };
  if color.alpha() <= 0.0 {
    return;
  }
  let mut vertices = native_vector_ring_points(polygon.exterior(), mvt_to_world);
  if vertices.len() < 3 {
    return;
  }

  let mut hole_indices = Vec::new();
  for interior in polygon.interiors() {
    let points = native_vector_ring_points(interior, mvt_to_world);
    if points.len() < 3 {
      continue;
    }
    hole_indices.push(vertices.len() as u32);
    vertices.extend(points);
  }

  let mut indices = Vec::new();
  let mut earcut = earcut::Earcut::new();
  earcut.earcut(vertices.iter().copied(), &hole_indices, &mut indices);
  if indices.is_empty() {
    return;
  }

  let z = native_vector_polygon_z(layer_name);
  let batch = native_vector_mesh_batch(batches, color, z);
  let base_index = batch.positions.len() as u32;
  batch
    .positions
    .extend(vertices.iter().map(|coord| [coord[0], coord[1], 0.0]));
  batch
    .indices
    .extend(indices.into_iter().map(|index| base_index + index));
  for coord in vertices {
    include_native_vector_local_point(batch, tile_origin, coord[0], coord[1]);
  }
}

fn native_vector_ring_points(
  ring: &geo_types::LineString<f32>,
  mvt_to_world: f32,
) -> Vec<[f32; 2]> {
  let mut points = ring
    .coords()
    .map(|coord| [coord.x * mvt_to_world, coord.y * mvt_to_world])
    .collect::<Vec<_>>();
  if points
    .first()
    .zip(points.last())
    .is_some_and(|(first, last)| first[0] == last[0] && first[1] == last[1])
  {
    points.pop();
  }
  points
}

#[allow(clippy::too_many_arguments)]
fn append_native_vector_line(
  batches: &mut Vec<NativeVectorMeshBatch>,
  line: &geo_types::LineString<f32>,
  layer_name: &str,
  feature_class: &str,
  tile_zoom: u8,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
  style_pixel_to_world: f32,
) {
  if line.coords().count() < 2 {
    return;
  }
  let (casing_color, casing_width, inner_color, inner_width) =
    get_road_styling(layer_name, feature_class, tile_zoom);
  if casing_width > 0.0 {
    append_native_vector_line_mesh(
      batches,
      line,
      casing_color,
      casing_width * style_pixel_to_world,
      NATIVE_VECTOR_ROAD_CASING_Z,
      tile_origin,
      mvt_to_world,
    );
  }
  if inner_width > 0.0 {
    append_native_vector_line_mesh(
      batches,
      line,
      inner_color,
      inner_width * style_pixel_to_world,
      NATIVE_VECTOR_ROAD_INNER_Z,
      tile_origin,
      mvt_to_world,
    );
  }
}

fn append_native_vector_line_mesh(
  batches: &mut Vec<NativeVectorMeshBatch>,
  line: &geo_types::LineString<f32>,
  color: tiny_skia::Color,
  width: f32,
  z: f32,
  tile_origin: PixelCoordinate,
  mvt_to_world: f32,
) {
  if color.alpha() <= 0.0 || width <= 0.0 {
    return;
  }
  let batch = native_vector_mesh_batch(batches, color, z);
  let half_width = width * 0.5;
  let points = line
    .coords()
    .map(|coord| [coord.x * mvt_to_world, coord.y * mvt_to_world])
    .collect::<Vec<_>>();

  for segment in points.windows(2) {
    let [x0, y0] = segment[0];
    let [x1, y1] = segment[1];
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f32::EPSILON {
      continue;
    }

    let nx = -dy / len * half_width;
    let ny = dx / len * half_width;
    let base_index = batch.positions.len() as u32;
    batch.positions.extend([
      [x0 + nx, y0 + ny, 0.0],
      [x1 + nx, y1 + ny, 0.0],
      [x1 - nx, y1 - ny, 0.0],
      [x0 - nx, y0 - ny, 0.0],
    ]);
    batch.indices.extend([
      base_index,
      base_index + 1,
      base_index + 2,
      base_index,
      base_index + 2,
      base_index + 3,
    ]);
    include_native_vector_local_point_with_padding(batch, tile_origin, x0, y0, half_width);
    include_native_vector_local_point_with_padding(batch, tile_origin, x1, y1, half_width);
  }
}

fn append_native_vector_rect(
  batches: &mut Vec<NativeVectorMeshBatch>,
  color: tiny_skia::Color,
  z: f32,
  tile_origin: PixelCoordinate,
  width: f32,
  height: f32,
) {
  let batch = native_vector_mesh_batch(batches, color, z);
  let base_index = batch.positions.len() as u32;
  batch.positions.extend([
    [0.0, 0.0, 0.0],
    [width, 0.0, 0.0],
    [width, height, 0.0],
    [0.0, height, 0.0],
  ]);
  batch.indices.extend([
    base_index,
    base_index + 1,
    base_index + 2,
    base_index,
    base_index + 2,
    base_index + 3,
  ]);
  include_native_vector_local_point(batch, tile_origin, 0.0, 0.0);
  include_native_vector_local_point(batch, tile_origin, width, height);
}

fn native_vector_mesh_batch(
  batches: &mut Vec<NativeVectorMeshBatch>,
  color: tiny_skia::Color,
  z: f32,
) -> &mut NativeVectorMeshBatch {
  let color_key = native_vector_color_key(color);
  let z_key = native_vector_z_key(z);
  if let Some(index) = batches
    .iter()
    .position(|batch| batch.color_key == color_key && batch.z_key == z_key)
  {
    return &mut batches[index];
  }

  batches.push(NativeVectorMeshBatch {
    color_key,
    color: native_vector_color(color),
    z_key,
    z,
    positions: Vec::new(),
    indices: Vec::new(),
    bounds: None,
  });
  batches.last_mut().expect("batch was pushed")
}

fn include_native_vector_local_point(
  batch: &mut NativeVectorMeshBatch,
  tile_origin: PixelCoordinate,
  x: f32,
  y: f32,
) {
  let coord = PixelCoordinate {
    x: tile_origin.x + x,
    y: tile_origin.y + y,
  };
  if let Some(bounds) = &mut batch.bounds {
    bounds.include(coord);
  } else {
    batch.bounds = Some(NativeVectorTileBounds::from_min_max(coord, coord));
  }
}

fn include_native_vector_local_point_with_padding(
  batch: &mut NativeVectorMeshBatch,
  tile_origin: PixelCoordinate,
  x: f32,
  y: f32,
  padding: f32,
) {
  include_native_vector_local_point(batch, tile_origin, x - padding, y - padding);
  include_native_vector_local_point(batch, tile_origin, x + padding, y + padding);
}

fn native_vector_batches_to_mesh_specs(
  batches: Vec<NativeVectorMeshBatch>,
  origin: PixelCoordinate,
) -> Vec<NativeVectorTileMeshSpec> {
  batches
    .into_iter()
    .filter_map(|batch| {
      if batch.positions.is_empty() || batch.indices.is_empty() {
        return None;
      }
      let bounds = batch.bounds?;
      let normals = vec![[0.0, 0.0, 1.0]; batch.positions.len()];
      let uvs = vec![[0.0, 0.0]; batch.positions.len()];
      let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
      )
      .with_inserted_indices(Indices::U32(batch.indices))
      .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, batch.positions)
      .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
      .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs);

      Some(NativeVectorTileMeshSpec {
        mesh,
        color: batch.color,
        bounds,
        origin,
        z: batch.z,
      })
    })
    .collect()
}

fn native_vector_polygon_z(layer_name: &str) -> f32 {
  match layer_name {
    "landcover" | "landuse" | "park" => NATIVE_VECTOR_LAND_Z,
    "water" | "waterway" => NATIVE_VECTOR_WATER_Z,
    "building" | "buildings" => NATIVE_VECTOR_BUILDING_Z,
    _ => NATIVE_VECTOR_LAND_Z,
  }
}

fn native_vector_z_key(z: f32) -> i16 {
  (z * 10.0).round() as i16
}

fn native_vector_color_key(color: tiny_skia::Color) -> [u8; 4] {
  let color = color.to_color_u8();
  [color.red(), color.green(), color.blue(), color.alpha()]
}

fn native_vector_color(color: tiny_skia::Color) -> Color {
  let color = color.to_color_u8();
  Color::srgba(
    f32::from(color.red()) / 255.0,
    f32::from(color.green()) / 255.0,
    f32::from(color.blue()) / 255.0,
    f32::from(color.alpha()) / 255.0,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tile_detail_factor_maps_to_adjacent_zoom_levels() {
    let mut transform = mapvas::map::coordinates::Transform::default();
    transform.zoom = 2f32.powi(10);

    assert_eq!(tile_zoom_with_detail_factor(transform, 0.5), 12);
    assert_eq!(tile_zoom_with_detail_factor(transform, 1.0), 13);
    assert_eq!(tile_zoom_with_detail_factor(transform, 2.0), 14);
  }

  #[test]
  fn tile_detail_factor_is_clamped_to_supported_range() {
    assert_eq!(clamped_tile_detail_factor(0.25), 0.5);
    assert_eq!(clamped_tile_detail_factor(2.5), 2.0);
    assert_eq!(clamped_tile_detail_factor(f32::NAN), 1.0);
  }

  #[test]
  fn tile_detail_factor_label_formats_float_factor() {
    assert_eq!(tile_detail_factor_label(0.5), "0.50x");
    assert_eq!(tile_detail_factor_label(1.0), "1.00x");
    assert_eq!(tile_detail_factor_label(1.25), "1.25x");
    assert_eq!(tile_detail_factor_label(2.0), "2.00x");
  }

  #[test]
  fn effective_tile_detail_factor_includes_base_factor() {
    assert_eq!(effective_tile_detail_factor(0.5), 1.0);
    assert_eq!(effective_tile_detail_factor(1.0), 2.0);
    assert_eq!(effective_tile_detail_factor(2.0), 4.0);
  }
}
