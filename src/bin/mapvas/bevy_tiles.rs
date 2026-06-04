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
  window::PrimaryWindow,
};
use mapvas::{
  config::{Config, TileProvider, TileType},
  map::{
    coordinates::{
      CANVAS_SIZE, TILE_SIZE, Tile, TileCoordinate, TilePriority, Transform as MapTransform,
      generate_preload_tiles, tiles_in_box,
    },
    mapvas_egui::MapViewport,
    tile_loader::{CachedTileLoader, TileLoader, TileSource},
    tile_renderer::{RasterTileRenderer, TileRenderer, VectorTileRenderer, style_version},
  },
  task_tracker::{TaskCategory, TaskGuard},
};

#[derive(Resource)]
pub struct NativeTileRuntime(pub tokio::runtime::Handle);

#[derive(Resource, Default)]
pub struct NativeMapViewport {
  viewport: Option<MapViewport>,
}

impl NativeMapViewport {
  pub fn set(&mut self, viewport: Option<MapViewport>) {
    self.viewport = viewport;
  }

  #[must_use]
  pub fn get(&self) -> Option<MapViewport> {
    self.viewport
  }
}

pub struct NativeTilePlugin;

impl Plugin for NativeTilePlugin {
  fn build(&self, app: &mut App) {
    app.init_resource::<NativeMapViewport>().add_systems(
      Update,
      (collect_finished_tiles, update_native_tiles).chain(),
    );
  }
}

#[derive(Component)]
struct NativeTileSprite;

struct NativeTileEntry {
  image: Handle<Image>,
  entity: Option<Entity>,
}

enum NativeTileResult {
  Ready {
    generation: u64,
    tile: Tile,
    image: egui::ColorImage,
  },
  Failed {
    generation: u64,
    tile: Tile,
  },
}

#[derive(Resource)]
pub struct NativeTileLayer {
  receiver: Mutex<Receiver<NativeTileResult>>,
  sender: Sender<NativeTileResult>,
  all_tile_loader: Vec<Arc<CachedTileLoader>>,
  tile_loader_index: usize,
  tile_providers: Vec<TileProvider>,
  tile_source: TileSource,
  visible: bool,
  loaded_tiles: HashMap<Tile, NativeTileEntry>,
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
}

impl NativeTileLayer {
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

  pub fn draw_overlay(&self, ui: &mut egui::Ui, viewport: Option<MapViewport>) {
    if !self.visible || self.loaded_tiles.is_empty() {
      return;
    }
    let Some(tile_loader) = self.tile_loader() else {
      return;
    };
    if !tile_loader.requires_osm_attribution() {
      return;
    }
    let Some(viewport) = viewport else {
      return;
    };

    draw_osm_attribution(ui, viewport.rect);
  }

  pub fn ui(&mut self, ui: &mut egui::Ui) {
    ui.collapsing("Native Tile Layer", |ui| {
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

    let calculated_zoom = (viewport.transform.zoom
      * (viewport.rect.width().max(viewport.rect.height()) / TILE_SIZE))
      .log2()
      .max(0.0) as u8
      + 2;
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
    let vp_min = inv.apply(viewport.rect.min.into());
    let vp_max = inv.apply(viewport.rect.max.into());
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
    if tile.zoom > max_zoom {
      return;
    }

    self.in_flight_tiles.insert(tile);

    let sender = self.sender.clone();
    let tile_type = tile_loader.tile_type();
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
        let _ = sender.send(NativeTileResult::Failed { generation, tile });
        return;
      };

      let (render_rx, _) = mapvas::render_scheduler::RENDER_SCHEDULER
        .submit(priority, move || renderer.render(&tile, &tile_data));

      let render_result = tokio::time::timeout(std::time::Duration::from_secs(30), render_rx).await;
      let image = match render_result {
        Ok(Ok(Ok(image))) => image,
        _ => {
          let _ = sender.send(NativeTileResult::Failed { generation, tile });
          return;
        }
      };

      let _ = sender.send(NativeTileResult::Ready {
        generation,
        tile,
        image,
      });
    });
  }

  fn request_preload_tiles(&mut self, visible_tiles: &[Tile], runtime: tokio::runtime::Handle) {
    let Some(tile_loader) = self.tile_loader() else {
      return;
    };
    if !self.preload_enabled || !tile_loader.allows_preloading() {
      return;
    }

    for (tile, priority) in generate_preload_tiles(visible_tiles) {
      self.request_tile(tile, priority, runtime.clone());
    }
  }
}

fn collect_finished_tiles(mut layer: ResMut<NativeTileLayer>, mut images: ResMut<Assets<Image>>) {
  let mut results = Vec::new();
  if let Ok(receiver) = layer.receiver.lock() {
    while let Ok(result) = receiver.try_recv() {
      results.push(result);
    }
  }

  for result in results {
    match result {
      NativeTileResult::Ready {
        generation,
        tile,
        image,
      } => {
        if generation != layer.generation {
          continue;
        }
        layer.in_flight_tiles.remove(&tile);
        let image = images.add(color_image_to_bevy(image));
        layer.loaded_tiles.insert(
          tile,
          NativeTileEntry {
            image,
            entity: None,
          },
        );
      }
      NativeTileResult::Failed { generation, tile } => {
        if generation != layer.generation {
          continue;
        }
        layer.in_flight_tiles.remove(&tile);
      }
    }
  }
}

fn update_native_tiles(
  mut commands: Commands,
  mut layer: ResMut<NativeTileLayer>,
  viewport: Res<NativeMapViewport>,
  runtime: Res<NativeTileRuntime>,
  windows: Query<&Window, With<PrimaryWindow>>,
  mut sprites: Query<(&mut Transform, &mut Sprite, &mut Visibility), With<NativeTileSprite>>,
) {
  for entity in layer.stale_entities.drain(..) {
    commands.entity(entity).despawn();
  }

  if !layer.visible {
    for (_, _, mut visibility) in &mut sprites {
      *visibility = Visibility::Hidden;
    }
    return;
  }

  let Some(viewport) = viewport.viewport else {
    return;
  };
  let Ok(window) = windows.single() else {
    return;
  };

  let visible_tiles = layer.visible_tiles(viewport);
  let visible_set: HashSet<Tile> = visible_tiles.iter().copied().collect();
  let runtime_handle = runtime.0.clone();

  for tile in &visible_tiles {
    if !layer.loaded_tiles.contains_key(tile) {
      layer.request_tile(*tile, TilePriority::Current, runtime_handle.clone());
      continue;
    }

    let Some(tile_rect) = tile_screen_rect(viewport, *tile) else {
      continue;
    };
    let transform = transform_for_screen_rect(tile_rect, window);
    let custom_size = Vec2::new(tile_rect.width(), tile_rect.height());

    let entry = layer.loaded_tiles.get_mut(tile).expect("tile was checked");
    if let Some(entity) = entry.entity {
      if let Ok((mut sprite_transform, mut sprite, mut visibility)) = sprites.get_mut(entity) {
        *sprite_transform = transform;
        sprite.custom_size = Some(custom_size);
        *visibility = Visibility::Visible;
      } else {
        entry.entity = None;
      }
    }

    if entry.entity.is_none() {
      let mut sprite = Sprite::from_image(entry.image.clone());
      sprite.custom_size = Some(custom_size);
      entry.entity = Some(commands.spawn((sprite, transform, NativeTileSprite)).id());
    }
  }

  layer.request_preload_tiles(&visible_tiles, runtime_handle);

  for (tile, entry) in &mut layer.loaded_tiles {
    if visible_set.contains(tile) {
      continue;
    }
    if let Some(entity) = entry.entity
      && let Ok((_, _, mut visibility)) = sprites.get_mut(entity)
    {
      *visibility = Visibility::Hidden;
    }
  }
}

fn color_image_to_bevy(image: egui::ColorImage) -> Image {
  let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
  for pixel in image.pixels {
    rgba.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b(), pixel.a()]);
  }

  Image::new(
    Extent3d {
      width: image.size[0] as u32,
      height: image.size[1] as u32,
      depth_or_array_layers: 1,
    },
    TextureDimension::D2,
    rgba,
    TextureFormat::Rgba8UnormSrgb,
    RenderAssetUsages::RENDER_WORLD,
  )
}

fn tile_screen_rect(viewport: MapViewport, tile: Tile) -> Option<egui::Rect> {
  let (nw, se) = tile.position();
  let left_x = viewport_left_x(viewport.rect, &viewport.transform);
  let dx = tile_world_offset(nw.x, left_x);

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
    let tile_rect = egui::Rect::from_min_max(nw_screen.into(), se_screen.into());
    if tile_rect.intersects(viewport.rect) {
      return Some(tile_rect);
    }
  }

  None
}

fn transform_for_screen_rect(rect: egui::Rect, window: &Window) -> Transform {
  let center = rect.center();
  Transform::from_xyz(
    center.x - window.width() / 2.0,
    window.height() / 2.0 - center.y,
    -10.0,
  )
}

fn viewport_left_x(rect: egui::Rect, transform: &MapTransform) -> f32 {
  let inv = transform.invert();
  inv.apply(egui::pos2(rect.min.x, 0.0).into()).x
}

fn tile_world_offset(tile_x: f32, left_x: f32) -> f32 {
  ((left_x - tile_x) / CANVAS_SIZE - 1e-6).ceil() * CANVAS_SIZE
}

fn draw_osm_attribution(ui: &mut egui::Ui, clip_rect: egui::Rect) {
  let painter = ui.painter_at(clip_rect);
  let margin = 8.0;
  let size = egui::vec2(158.0, 20.0);
  let attribution_rect = egui::Rect::from_min_size(
    egui::pos2(
      clip_rect.max.x - size.x - margin,
      clip_rect.max.y - size.y - margin,
    ),
    size,
  );

  painter.rect_filled(
    attribution_rect,
    egui::CornerRadius::same(3),
    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 210),
  );
  painter.text(
    attribution_rect.center(),
    egui::Align2::CENTER_CENTER,
    "© OpenStreetMap contributors",
    egui::FontId::proportional(11.0),
    egui::Color32::from_rgb(35, 35, 35),
  );
}
