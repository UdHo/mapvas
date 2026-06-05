use std::{
  collections::{HashMap, HashSet},
  sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
  },
};

use bevy::{
  asset::RenderAssetUsages,
  prelude::*,
  render::render_resource::{Extent3d, TextureDimension, TextureFormat},
  sprite::Anchor,
  window::PrimaryWindow,
};
use mapvas::{
  config::{Config, TileProvider, TileType},
  map::{
    coordinates::{
      CANVAS_SIZE, PixelPosition, PixelRect, Tile, TileCoordinate, TilePriority,
      generate_preload_tiles, tile_zoom_for_transform, tiles_in_box,
    },
    tile_loader::{CachedTileLoader, TileLoader, TileSource},
    tile_renderer::{
      RasterTileRenderer, TileImage, TileRenderer, VectorTileRenderer, style_version,
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

#[derive(Resource)]
pub struct BevyTileRuntime(pub tokio::runtime::Handle);

pub struct BevyTilePlugin;

impl Plugin for BevyTilePlugin {
  fn build(&self, app: &mut App) {
    app
      .init_gizmo_group::<BevyTileOverlayGizmos>()
      .add_systems(Startup, configure_bevy_tile_overlay_gizmos)
      .add_systems(Update, (collect_finished_tiles, update_bevy_tiles).chain());
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum CoordinateDisplayMode {
  Off,
  Overlay,
  GridOnly,
}

struct BevyTileEntry {
  image: Handle<Image>,
  entity: Option<Entity>,
}

struct TileLabel {
  background_rect: PixelRect,
  background_size: Vec2,
  text_pos: PixelPosition,
  text: String,
}

enum BevyTileResult {
  Ready {
    generation: u64,
    tile: Tile,
    image: TileImage,
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
  loaded_tiles: HashMap<Tile, BevyTileEntry>,
  in_flight_tiles: HashSet<Tile>,
  stale_entities: Vec<Entity>,
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
      loaded_tiles: HashMap::new(),
      in_flight_tiles: HashSet::new(),
      stale_entities: Vec::new(),
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
      let tiles_rendering = tiles_in_flight.saturating_sub(tiles_downloading);

      ui.horizontal(|ui| {
        ui.label("Tiles loaded:");
        ui.label(tiles_loaded.to_string());
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
      if let Some(entity) = entry.entity {
        self.stale_entities.push(entity);
      }
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

  fn visible_tiles(&mut self, viewport: MapViewport) -> Vec<Tile> {
    let Some(tile_loader) = self.tile_loader() else {
      return Vec::new();
    };

    let calculated_zoom = tile_zoom_for_transform(&viewport.transform);
    let max_zoom = tile_loader.max_zoom();
    let tile_type = tile_loader.tile_type();
    let request_zoom = if tile_type == TileType::Vector && calculated_zoom > max_zoom {
      calculated_zoom.min(19)
    } else {
      calculated_zoom.min(max_zoom)
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

fn collect_finished_tiles(mut layer: ResMut<BevyTileLayer>, mut images: ResMut<Assets<Image>>) {
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
        layer.loaded_tiles.insert(
          tile,
          BevyTileEntry {
            image,
            entity: None,
          },
        );
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
  mut sprites: Query<(&mut Transform, &mut Sprite, &mut Visibility), With<BevyTileSprite>>,
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
  for entity in layer.stale_entities.drain(..) {
    commands.entity(entity).despawn();
  }

  if !layer.visible {
    hide_tile_sprites(&mut sprites);
    hide_grid_tile_sprites(&mut grid_sprites);
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
    hide_osm_attribution(&mut attribution_backgrounds, &mut attribution_texts);
    return;
  }

  let Some(viewport) = viewport.get() else {
    hide_tile_sprites(&mut sprites);
    hide_grid_tile_sprites(&mut grid_sprites);
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
    hide_osm_attribution(&mut attribution_backgrounds, &mut attribution_texts);
    return;
  };
  let Ok(window) = windows.single() else {
    hide_tile_sprites(&mut sprites);
    hide_grid_tile_sprites(&mut grid_sprites);
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
    hide_osm_attribution(&mut attribution_backgrounds, &mut attribution_texts);
    return;
  };

  let visible_tiles = layer.visible_tiles(viewport);
  let runtime_handle = runtime.0.clone();
  let show_osm_attribution = layer
    .tile_loader()
    .is_some_and(|loader| loader.requires_osm_attribution())
    && !layer.loaded_tiles.is_empty()
    && layer.coordinate_display_mode != CoordinateDisplayMode::GridOnly;

  for tile in &visible_tiles {
    if !layer.loaded_tiles.contains_key(tile) {
      layer.request_tile(*tile, TilePriority::Current, runtime_handle.clone());
    }
  }

  if layer.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
    layer.request_preload_tiles(&visible_tiles, runtime_handle);
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

    let Some(tile_rect) = tile_screen_rect(viewport, tile) else {
      continue;
    };
    let transform = transform_for_screen_rect(tile_rect, window, tile.zoom);
    let custom_size = Vec2::new(tile_rect.width(), tile_rect.height());
    let tint = tile_tint(layer.coordinate_display_mode, tile);

    let entry = layer.loaded_tiles.get_mut(&tile).expect("tile was checked");
    if let Some(entity) = entry.entity {
      if let Ok((mut sprite_transform, mut sprite, mut visibility)) = sprites.get_mut(entity) {
        *sprite_transform = transform;
        sprite.custom_size = Some(custom_size);
        sprite.color = tint;
        *visibility = Visibility::Visible;
      } else {
        entry.entity = None;
      }
    }

    if entry.entity.is_none() {
      let mut sprite = Sprite::from_image(entry.image.clone());
      sprite.custom_size = Some(custom_size);
      sprite.color = tint;
      entry.entity = Some(commands.spawn((sprite, transform, BevyTileSprite)).id());
    }
  }

  layer.request_preload_tiles(&visible_tiles, runtime_handle);

  for (tile, entry) in &mut layer.loaded_tiles {
    if draw_set.contains(tile) {
      continue;
    }
    if let Some(entity) = entry.entity
      && let Ok((_, _, mut visibility)) = sprites.get_mut(entity)
    {
      *visibility = Visibility::Hidden;
    }
  }
}

fn hide_tile_sprites(
  sprites: &mut Query<(&mut Transform, &mut Sprite, &mut Visibility), With<BevyTileSprite>>,
) {
  for (_, _, mut visibility) in sprites.iter_mut() {
    *visibility = Visibility::Hidden;
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
    let Some(tile_rect) = tile_screen_rect(viewport, *tile) else {
      continue;
    };
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

fn tile_screen_rect(viewport: MapViewport, tile: Tile) -> Option<PixelRect> {
  coordinate_tile_rects(viewport, tile).into_iter().next()
}

fn coordinate_tile_rects(viewport: MapViewport, tile: Tile) -> Vec<PixelRect> {
  let (nw, se) = tile.position();
  let left_x = viewport_left_x(viewport);
  let dx = tile_world_offset(nw.x, left_x);
  let mut tile_rects = Vec::with_capacity(3);

  for offset in [dx - CANVAS_SIZE, dx, dx + CANVAS_SIZE] {
    let nw_shifted = mapvas::map::coordinates::PixelCoordinate {
      x: nw.x + offset,
      y: nw.y,
    };
    let se_shifted = mapvas::map::coordinates::PixelCoordinate {
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

fn viewport_left_x(viewport: MapViewport) -> f32 {
  let inv = viewport.transform.invert();
  inv
    .apply(PixelPosition {
      x: viewport.min().x,
      y: 0.0,
    })
    .x
}

fn tile_world_offset(tile_x: f32, left_x: f32) -> f32 {
  ((left_x - tile_x) / CANVAS_SIZE - 1e-6).ceil() * CANVAS_SIZE
}
