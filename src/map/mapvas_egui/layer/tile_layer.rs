use std::{
  collections::{HashMap, HashSet},
  sync::{Arc, Mutex},
};

use egui::{Color32, ColorImage, Pos2, Rect, Ui};
use log::error;

use crate::{
  config::TileType,
  map::{
    coordinates::{
      TILE_SIZE, Tile, TileCoordinate, TilePriority, Transform, generate_preload_tiles,
      tiles_in_box,
    },
    tile_loader::{CachedTileLoader, TileLoader, TileSource},
    tile_renderer::{RasterTileRenderer, TileRenderer, VectorTileRenderer, style_version},
  },
  profile_scope,
  task_tracker::{TaskCategory, TaskGuard},
};

use super::{Layer, LayerProperties};

/// Splits a `ColorImage` into a grid of tiles for super-resolution rendering.
///
/// # Arguments
/// * `image` - The source image to split (must be sized as `TILE_SIZE` * `grid_size`)
/// * `grid_size` - Number of tiles per side (e.g., 2 for 4 tiles, 4 for 16 tiles)
///
/// # Returns
/// Vector of `ColorImages`, ordered row by row (top-left to bottom-right)
fn split_image_into_tiles(image: &ColorImage, grid_size: usize) -> Vec<ColorImage> {
  let size = image.size[0];
  let tile_size = size / grid_size;
  let num_tiles = grid_size * grid_size;

  let mut tiles = Vec::with_capacity(num_tiles);

  // Create tiles row by row
  for tile_y in 0..grid_size {
    for tile_x in 0..grid_size {
      let mut tile_rgba = Vec::with_capacity(tile_size * tile_size * 4);

      // Extract pixels for this tile
      for y in 0..tile_size {
        for x in 0..tile_size {
          let src_x = tile_x * tile_size + x;
          let src_y = tile_y * tile_size + y;
          let src_idx = src_y * size + src_x;
          let color = image.pixels[src_idx];
          tile_rgba.extend_from_slice(&[color.r(), color.g(), color.b(), color.a()]);
        }
      }

      tiles.push(ColorImage::from_rgba_unmultiplied([tile_size, tile_size], &tile_rgba));
    }
  }

  tiles
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoordinateDisplayMode {
  Off,
  Overlay,
  GridOnly,
}

/// A layer that loads and displays the map tiles.
pub struct TileLayer {
  receiver: std::sync::mpsc::Receiver<(Tile, ColorImage)>,
  sender: std::sync::mpsc::Sender<(Tile, ColorImage)>,
  tile_loader_index: usize,
  tile_loader_old_index: usize,
  all_tile_loader: Vec<Arc<CachedTileLoader>>,
  loaded_tiles: HashMap<Tile, egui::TextureHandle>,
  in_flight_tiles: Arc<Mutex<HashSet<Tile>>>,
  ctx: egui::Context,
  layer_properties: LayerProperties,
  tile_source: TileSource,
  coordinate_display_mode: CoordinateDisplayMode,
  raster_renderer: Arc<dyn TileRenderer>,
  vector_renderer: Arc<dyn TileRenderer>,
  render_semaphore: Arc<tokio::sync::Semaphore>,
  // Statistics
  current_ideal_zoom: u8,
  current_request_zoom: u8,
  current_max_zoom: u8,
  // Style version tracking for cache invalidation
  last_style_version: u64,
  // Last visible tiles for immediate re-request on style change
  last_visible_tiles: Vec<Tile>,
  // Abort handles for in-flight render tasks (to cancel on style change)
  render_abort_handles: Arc<Mutex<Vec<tokio::task::AbortHandle>>>,
}

const NAME: &str = "Tile Layer";

impl TileLayer {
  pub fn from_config(clone: egui::Context, config: &crate::config::Config) -> TileLayer {
    let (sender, receiver) = std::sync::mpsc::channel();
    let all_tile_loader = CachedTileLoader::from_config(config)
      .map(Arc::new)
      .collect();
    let layer = TileLayer {
      receiver,
      sender,
      tile_loader_index: 0,
      tile_loader_old_index: 0,
      all_tile_loader,
      loaded_tiles: HashMap::new(),
      in_flight_tiles: Arc::new(Mutex::new(HashSet::new())),
      ctx: clone,
      layer_properties: LayerProperties::default(),
      tile_source: TileSource::All,
      coordinate_display_mode: CoordinateDisplayMode::Off,
      raster_renderer: Arc::new(RasterTileRenderer::new()),
      vector_renderer: Arc::new(VectorTileRenderer::new()),
      render_semaphore: Arc::new(tokio::sync::Semaphore::new(24)),
      current_ideal_zoom: 0,
      current_request_zoom: 0,
      current_max_zoom: 0,
      last_style_version: style_version(),
      last_visible_tiles: Vec::new(),
      render_abort_handles: Arc::new(Mutex::new(Vec::new())),
    };

    // Spawn diagnostic task to monitor in-flight tiles
    let in_flight_diagnostic = layer.in_flight_tiles.clone();
    let semaphore_diagnostic = layer.render_semaphore.clone();
    tokio::spawn(async move {
      loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let in_flight = in_flight_diagnostic.lock().unwrap();
        let in_flight_count = in_flight.len();
        let available_permits = semaphore_diagnostic.available_permits();
        if in_flight_count > 0 {
          log::warn!(
            "DIAGNOSTIC: {} tiles in-flight, {} semaphore permits available. In-flight tiles: {:?}",
            in_flight_count,
            available_permits,
            in_flight.iter().take(5).collect::<Vec<_>>()
          );
        }
      }
    });

    layer
  }

  fn draw_tile(&self, ui: &mut Ui, rect: Rect, tile: &Tile, transform: &Transform) -> bool {
    if let Some(image_data) = self.loaded_tiles.get(tile) {
      let (nw, se) = tile.position();
      let (nw, se) = (transform.apply(nw), transform.apply(se));
      let tile_rect = Rect::from_min_max(nw.into(), se.into());

      let tint_color = if self.coordinate_display_mode == CoordinateDisplayMode::Overlay {
        let is_even_tile = (tile.x + tile.y).is_multiple_of(2);
        if is_even_tile {
          Color32::from_rgba_unmultiplied(255, 240, 240, 255)
        } else {
          Color32::from_rgba_unmultiplied(240, 240, 255, 255)
        }
      } else {
        Color32::WHITE
      };

      ui.painter_at(rect).image(
        image_data.id(),
        tile_rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint_color,
      );

      if self.coordinate_display_mode == CoordinateDisplayMode::Overlay {
        draw_coordinate_text_overlay(ui, rect, tile, &tile_rect);
      }

      return true;
    }
    false
  }

  fn draw_coordinate_grid(
    &self,
    ui: &mut Ui,
    clip_rect: Rect,
    transform: &Transform,
    min_pos: TileCoordinate,
    max_pos: TileCoordinate,
  ) {
    use crate::map::coordinates::tiles_in_box;

    let painter = ui.painter_at(clip_rect);

    for tile in tiles_in_box(min_pos, max_pos) {
      let (nw, se) = tile.position();
      let (nw, se) = (transform.apply(nw), transform.apply(se));
      let tile_rect = Rect::from_min_max(nw.into(), se.into());

      if !tile_rect.intersects(clip_rect) {
        continue;
      }

      if self.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
        let is_even_tile = (tile.x + tile.y).is_multiple_of(2);
        let bg_color = if is_even_tile {
          Color32::from_rgba_unmultiplied(255, 240, 240, 120)
        } else {
          Color32::from_rgba_unmultiplied(240, 240, 255, 120)
        };
        painter.rect_filled(tile_rect, egui::CornerRadius::ZERO, bg_color);
      }

      self.draw_tile_info(ui, clip_rect, &tile, &tile_rect);
    }
  }

  fn draw_tile_info(&self, ui: &mut Ui, clip_rect: Rect, tile: &Tile, tile_rect: &Rect) {
    let painter = ui.painter_at(clip_rect);

    let bg_width = 100.0;
    let bg_height = 60.0;
    let bg_rect = Rect::from_center_size(tile_rect.center(), egui::vec2(bg_width, bg_height));

    if tile_rect.width() > bg_width && tile_rect.height() > bg_height {
      painter.rect_filled(
        bg_rect,
        egui::CornerRadius::same(5),
        Color32::from_rgba_unmultiplied(0, 0, 0, 180),
      );

      let font_id = egui::FontId::monospace(11.0);
      let text_color = Color32::WHITE;

      let lines = [
        format!("Z:{}", tile.zoom),
        format!("X:{}", tile.x),
        format!("Y:{}", tile.y),
      ];

      for (i, line) in lines.iter().enumerate() {
        #[expect(clippy::cast_precision_loss)]
        let text_pos = bg_rect.min + egui::vec2(8.0, 8.0 + i as f32 * 14.0);
        painter.text(
          text_pos,
          egui::Align2::LEFT_TOP,
          line,
          font_id.clone(),
          text_color,
        );
      }
    }

    let border_color = if self.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
      Color32::from_rgb(80, 80, 80)
    } else {
      Color32::from_rgb(100, 100, 100)
    };

    painter.rect_stroke(
      *tile_rect,
      egui::CornerRadius::ZERO,
      egui::Stroke::new(1.0, border_color),
      egui::epaint::StrokeKind::Outside,
    );
  }

  fn tile_loader(&self) -> Arc<CachedTileLoader> {
    self.all_tile_loader[self.tile_loader_index].clone()
  }

  fn renderer_for_tile_type(&self, tile_type: TileType) -> Arc<dyn TileRenderer> {
    match tile_type {
      TileType::Raster => self.raster_renderer.clone(),
      TileType::Vector => self.vector_renderer.clone(),
    }
  }

  fn get_tile(&self, tile: Tile) {
    self.get_tile_with_priority(tile, TilePriority::Current);
  }

  #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
  fn get_tile_super_resolution(&self, tile: Tile, priority: TilePriority) {
    let max_zoom = self.tile_loader().max_zoom();
    let zoom_diff = tile.zoom - max_zoom;

    // Calculate the parent tile at max_zoom (go up zoom_diff levels)
    let mut parent_tile = tile;
    for _ in 0..zoom_diff {
      parent_tile = parent_tile.parent().expect("tile should have parent");
    }

    // Calculate grid size and scale factor
    let grid_size = 1 << zoom_diff; // 2^zoom_diff (2, 4, 8, or 16)
    let scale = grid_size as u32;

    // Calculate all child tiles we'll generate
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

    // Check if any of the child tiles are already loaded or in-flight
    {
      let in_flight = self.in_flight_tiles.lock().unwrap();
      if child_tiles.iter().any(|t| self.loaded_tiles.contains_key(t) || in_flight.contains(t)) {
        log::debug!("Super-resolution tiles for {parent_tile:?} already loading/loaded");
        return;
      }
    }

    // Mark all child tiles as in-flight
    {
      let mut in_flight = self.in_flight_tiles.lock().unwrap();
      for child_tile in &child_tiles {
        in_flight.insert(*child_tile);
      }
      log::info!(
        "Super-resolution: marked {} child tiles ({}x{} grid) as in-flight for parent {parent_tile:?}",
        child_tiles.len(),
        grid_size,
        grid_size
      );
    }

    let sender = self.sender.clone();
    let tile_loader = self.tile_loader().clone();
    let ctx = self.ctx.clone();
    let tile_source = self.tile_source;
    let renderer = self.vector_renderer.clone();
    let in_flight_tiles = self.in_flight_tiles.clone();
    let render_semaphore = self.render_semaphore.clone();

    log::info!(
      "Super-resolution: loading parent tile {parent_tile:?} at zoom {}, will generate {grid_size}x{grid_size} tiles at zoom {}",
      parent_tile.zoom,
      tile.zoom
    );

    tokio::spawn(async move {
      let task_name = format!(
        "tile-superres-{}-{}-{}",
        parent_tile.zoom, parent_tile.x, parent_tile.y
      );
      let _guard = TaskGuard::new(task_name, TaskCategory::TileSuperRes);

      // Download parent tile
      let tile_data = tile_loader
        .tile_data_with_priority(&parent_tile, tile_source, priority)
        .await;

      match &tile_data {
        Ok(data) => log::info!("Super-resolution: parent tile {parent_tile:?} data received: {} bytes", data.len()),
        Err(e) => {
          log::error!("Super-resolution: failed to fetch parent tile {parent_tile:?}: {e}");
          let mut in_flight = in_flight_tiles.lock().unwrap();
          for child_tile in &child_tiles {
            in_flight.remove(child_tile);
          }
          return;
        }
      }

      if let Ok(tile_data) = tile_data {
        // Acquire render permit
        let _permit = render_semaphore.acquire().await.unwrap();
        log::info!("Super-resolution: acquired render permit for {parent_tile:?}");

        // Render parent tile at scale (e.g., 2x, 4x, 8x, or 16x)
        let render_start = std::time::Instant::now();
        let blocking_task = tokio::task::spawn_blocking(move || {
          log::info!("Super-resolution: rendering parent {parent_tile:?} at {scale}x scale");
          let result = renderer.render_scaled(&parent_tile, &tile_data, scale);
          log::info!("Super-resolution: finished rendering parent {parent_tile:?}");
          result
        });

        let render_result = tokio::time::timeout(
          std::time::Duration::from_secs(60),
          blocking_task
        ).await;

        let render_duration = render_start.elapsed();

        let render_result = match render_result {
          Ok(result) => result,
          Err(_timeout) => {
            error!("Super-resolution: render timed out for {parent_tile:?}");
            let mut in_flight = in_flight_tiles.lock().unwrap();
            for child_tile in &child_tiles {
              in_flight.remove(child_tile);
            }
            return;
          }
        };

        log::info!("Super-resolution: render completed in {render_duration:?}");

        let scaled_image = match render_result {
          Ok(Ok(image)) => image,
          Ok(Err(e)) => {
            error!("Super-resolution: failed to render {parent_tile:?}: {e}");
            let mut in_flight = in_flight_tiles.lock().unwrap();
            for child_tile in &child_tiles {
              in_flight.remove(child_tile);
            }
            return;
          }
          Err(e) => {
            error!("Super-resolution: render task panicked for {parent_tile:?}: {e}");
            let mut in_flight = in_flight_tiles.lock().unwrap();
            for child_tile in &child_tiles {
              in_flight.remove(child_tile);
            }
            return;
          }
        };

        // Split into grid of tiles
        log::info!("Super-resolution: splitting {parent_tile:?} into {} child tiles ({}x{} grid)", child_tiles.len(), grid_size, grid_size);
        let split_tiles = split_image_into_tiles(&scaled_image, grid_size);

        // Send all child tiles
        for (i, child_tile) in child_tiles.iter().enumerate() {
          if let Err(e) = sender.send((*child_tile, split_tiles[i].clone())) {
            error!("Super-resolution: failed to send child tile {child_tile:?}: {e}");
          } else {
            log::debug!("Super-resolution: sent child tile {child_tile:?}");
          }
        }

        // Remove all from in-flight
        {
          let mut in_flight = in_flight_tiles.lock().unwrap();
          for child_tile in &child_tiles {
            in_flight.remove(child_tile);
          }
          log::info!("Super-resolution: completed {} child tiles for {parent_tile:?}", child_tiles.len());
        }

        ctx.request_repaint();
      }
    });
  }

  #[allow(clippy::too_many_lines)]
  fn get_tile_with_priority(&self, tile: Tile, priority: TilePriority) {
    // Check if tile exceeds max zoom level for this provider
    let max_zoom = self.tile_loader().max_zoom();
    let tile_type = self.tile_loader().tile_type();

    // For vector tiles beyond max zoom, use super-resolution rendering
    if tile.zoom > max_zoom {
      if tile_type == TileType::Vector && tile.zoom <= 19 {
        log::info!("Tile {tile:?} exceeds max zoom {max_zoom}, using super-resolution");
        self.get_tile_super_resolution(tile, priority);
        return;
      }
      log::debug!("Tile {tile:?} exceeds max zoom {max_zoom}, skipping request");
      return;
    }

    // Check if tile is already loaded or in-flight
    if self.loaded_tiles.contains_key(&tile) {
      log::trace!("Tile {tile:?} already loaded, skipping");
      return;
    }

    let mut in_flight = self.in_flight_tiles.lock().unwrap();
    if in_flight.contains(&tile) {
      log::warn!("Tile {tile:?} already in-flight (total in-flight: {}), skipping", in_flight.len());
      return; // Already loading
    }
    in_flight.insert(tile);
    let in_flight_count = in_flight.len();
    drop(in_flight); // Release lock before spawning

    log::info!("Tile {tile:?} inserted into in-flight set (now {in_flight_count} tiles in-flight)");

    let sender = self.sender.clone();
    let tile_loader = self.tile_loader().clone();
    let ctx = self.ctx.clone();
    let tile_source = self.tile_source;
    let tile_type = tile_loader.tile_type();
    let renderer = self.renderer_for_tile_type(tile_type);
    let in_flight_tiles = self.in_flight_tiles.clone();
    let render_semaphore = self.render_semaphore.clone();
    let abort_handles = self.render_abort_handles.clone();

    log::debug!(
      "Loading tile {tile:?} with {} renderer (tile_type: {tile_type:?}), available render permits: {}",
      renderer.name(),
      render_semaphore.available_permits()
    );

    let task_handle = tokio::spawn(async move {
      let task_name = format!("tile-load-{}-{}-{}", tile.zoom, tile.x, tile.y);
      let _guard = TaskGuard::new(task_name, TaskCategory::TileLoad);

      // Download phase (I/O - good for tokio)
      let tile_data = tile_loader
        .tile_data_with_priority(&tile, tile_source, priority)
        .await;
      match &tile_data {
        Ok(data) => log::info!("Tile {tile:?} data received: {} bytes", data.len()),
        Err(e) => {
          log::error!("Failed to fetch tile {tile:?}: {e}, removing from in-flight");
          let removed = in_flight_tiles.lock().unwrap().remove(&tile);
          log::info!("Tile {tile:?} removed from in-flight: {removed}");
          return;
        }
      }
      if let Ok(tile_data) = tile_data {
        // Acquire permit before rendering (limits concurrent renders to 24)
        let in_flight_count = in_flight_tiles.lock().unwrap().len();
        log::debug!(
          "Tile {tile:?} waiting for render permit, available: {}, in-flight: {}",
          render_semaphore.available_permits(),
          in_flight_count
        );
        let _permit = render_semaphore.acquire().await.unwrap();
        log::info!(
          "Tile {tile:?} acquired render permit, available: {}, in-flight: {}",
          render_semaphore.available_permits(),
          in_flight_count
        );

        // Render phase (CPU-bound - move to blocking thread)
        log::debug!("Tile {tile:?} spawning blocking task");
        let render_start = std::time::Instant::now();
        let blocking_task = tokio::task::spawn_blocking(move || {
          log::info!("INSIDE blocking task: Starting render for tile {tile:?}");
          let result = renderer.render(&tile, &tile_data);
          log::info!("INSIDE blocking task: Finished render for tile {tile:?}");
          result
        });

        // Add timeout to detect hanging renders
        let render_result = tokio::time::timeout(
          std::time::Duration::from_secs(30),
          blocking_task
        ).await;

        let render_duration = render_start.elapsed();

        // Handle timeout
        let render_result = match render_result {
          Ok(result) => result,
          Err(_timeout) => {
            error!("Tile {tile:?} render TIMED OUT after 30s, releasing permit and removing from in-flight");
            let removed = in_flight_tiles.lock().unwrap().remove(&tile);
            log::info!("Tile {tile:?} removed from in-flight after timeout: {removed}");
            return;
          }
        };

        // Permit automatically released when _permit drops
        log::info!("Tile {tile:?} render completed in {render_duration:?}, releasing permit");

        let egui_image = match render_result {
          Ok(Ok(image)) => image,
          Ok(Err(e)) => {
            error!("Failed to render tile {tile:?}: {e}, releasing permit and removing from in-flight");
            let removed = in_flight_tiles.lock().unwrap().remove(&tile);
            log::info!("Tile {tile:?} removed from in-flight after render error: {removed}");
            return;
          }
          Err(e) => {
            error!("Render task panicked for tile {tile:?}: {e}, releasing permit and removing from in-flight");
            let removed = in_flight_tiles.lock().unwrap().remove(&tile);
            log::info!("Tile {tile:?} removed from in-flight after panic: {removed}");
            return;
          }
        };

        if let Err(e) = sender.send((tile, egui_image)) {
          error!("Failed to send tile {tile:?}: {e}, removing from in-flight");
          let removed = in_flight_tiles.lock().unwrap().remove(&tile);
          log::info!("Tile {tile:?} removed from in-flight after send error: {removed}");
          return;
        }

        // Successfully completed - remove from in-flight
        log::info!("Tile {tile:?} successfully sent, removing from in-flight");
        let removed = in_flight_tiles.lock().unwrap().remove(&tile);
        log::info!("Tile {tile:?} removed from in-flight after success: {removed}, remaining in-flight: {}",
          in_flight_tiles.lock().unwrap().len());

        // Shorter delay for higher priority tiles
        let delay = match priority {
          TilePriority::Current => 100,
          TilePriority::Adjacent => 200,
          TilePriority::ZoomLevel => 300,
        };
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;

        ctx.request_repaint();
      }
    });

    // Store abort handle so we can cancel this task on style change
    abort_handles.lock().unwrap().push(task_handle.abort_handle());
  }

  fn preload_tiles(&self, visible_tiles: &[Tile]) {
    // Generate preload candidates
    let preload_candidates = generate_preload_tiles(visible_tiles);

    // Limit preloading to avoid overwhelming the system
    let max_preload = 20;
    for (tile, priority) in preload_candidates.into_iter().take(max_preload) {
      // Only preload if not already loaded or loading
      if !self.loaded_tiles.contains_key(&tile) {
        self.get_tile_with_priority(tile, priority);
      }
    }
  }
  fn collect_new_tile_data(&mut self, ui: &Ui) {
    for (tile, egui_image) in self.receiver.try_iter() {
      let handle = ui.ctx().load_texture(
        format!("{}-{}-{}", tile.zoom, tile.x, tile.y),
        egui_image,
        egui::TextureOptions::default(),
      );
      self.loaded_tiles.insert(tile, handle);
      self.in_flight_tiles.lock().unwrap().remove(&tile);
    }
  }
}

impl Layer for TileLayer {
  #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, rect: Rect) {
    profile_scope!("TileLayer::draw");
    self.collect_new_tile_data(ui);
    if self.tile_loader_index != self.tile_loader_old_index {
      log::info!(
        "Provider switched from {} to {}, clearing {} tiles",
        self.all_tile_loader[self.tile_loader_old_index].name(),
        self.all_tile_loader[self.tile_loader_index].name(),
        self.loaded_tiles.len()
      );
      self.loaded_tiles.clear();
      self.in_flight_tiles.lock().unwrap().clear();
      self.tile_loader_old_index = self.tile_loader_index;
    }

    // Check if style config has changed (for vector tiles)
    let current_style_version = style_version();
    if current_style_version != self.last_style_version {
      // Only clear vector tiles - raster tiles are not affected by style changes
      if self.tile_loader().tile_type() == TileType::Vector {
        log::info!(
          "Style changed (v{} -> v{}), clearing {} vector tiles, re-requesting {} tiles",
          self.last_style_version,
          current_style_version,
          self.loaded_tiles.len(),
          self.last_visible_tiles.len()
        );
        self.loaded_tiles.clear();
        self.in_flight_tiles.lock().unwrap().clear();

        // Abort all running render tasks to free up resources immediately
        {
          let mut handles = self.render_abort_handles.lock().unwrap();
          let abort_count = handles.len();
          for handle in handles.drain(..) {
            handle.abort();
          }
          log::info!("Aborted {} running render tasks", abort_count);
        }

        // Immediately re-request the last visible tiles
        let tiles_to_request = self.last_visible_tiles.clone();
        log::info!(
          "Style change: immediately re-requesting {} tiles: {:?}",
          tiles_to_request.len(),
          tiles_to_request.iter().take(5).collect::<Vec<_>>()
        );
        for tile in tiles_to_request {
          self.get_tile(tile);
        }

        // Request repaint to show results when ready
        self.ctx.request_repaint();
      }
      self.last_style_version = current_style_version;
    }

    if !self.visible() {
      return;
    }

    let (width, height) = (rect.width(), rect.height());
    let calculated_zoom = (transform.zoom * (width.max(height) / TILE_SIZE)).log2() as u8 + 2;
    let max_zoom = self.tile_loader().max_zoom();
    let tile_type = self.tile_loader().tile_type();

    // For vector tiles, allow requesting up to zoom 19 (super-resolution will handle it)
    // For raster tiles, cap at max_zoom
    let request_zoom = if tile_type == TileType::Vector && calculated_zoom > max_zoom {
      calculated_zoom.min(19)
    } else {
      calculated_zoom.min(max_zoom)
    };

    // Update statistics
    self.current_ideal_zoom = calculated_zoom;
    self.current_request_zoom = request_zoom;
    self.current_max_zoom = max_zoom;

    if calculated_zoom > max_zoom {
      log::info!(
        "Zoom capped: ideal={}, max={}, request_zoom={} (super-resolution: {})",
        calculated_zoom,
        max_zoom,
        request_zoom,
        tile_type == TileType::Vector
      );
    } else {
      log::debug!("Zoom: ideal={calculated_zoom}, max={max_zoom}, request_zoom={request_zoom}");
    }

    let inv = transform.invert();
    let min_pos = TileCoordinate::from_pixel_position(inv.apply(rect.min.into()), request_zoom);
    let max_pos = TileCoordinate::from_pixel_position(inv.apply(rect.max.into()), request_zoom);

    let visible_tiles: Vec<Tile> = tiles_in_box(min_pos, max_pos).collect();

    // Store visible tiles for immediate re-request on style change
    self.last_visible_tiles = visible_tiles.clone();

    // Load current visible tiles with highest priority
    for tile in &visible_tiles {
      if !self.loaded_tiles.contains_key(tile) {
        self.get_tile(*tile);
      }
    }

    // Start preloading adjacent and zoom level tiles
    self.preload_tiles(&visible_tiles);

    // Draw parent tiles if detailed tiles are not available yet. Coarser tiles are drawn first to
    // have detailed textures visible on top.
    let mut tiles_to_draw = tiles_in_box(min_pos, max_pos)
      .filter_map(|mut tile| {
        while !self.loaded_tiles.contains_key(&tile) {
          tile = tile.parent()?;
        }
        Some(tile)
      })
      .collect::<Vec<_>>();
    tiles_to_draw.sort_unstable_by_key(|tile| tile.zoom);
    tiles_to_draw.dedup();
    tiles_to_draw.reverse();

    if self.coordinate_display_mode != CoordinateDisplayMode::GridOnly {
      for tile in tiles_to_draw {
        if !self.draw_tile(ui, rect, &tile, transform) {
          self.get_tile(tile);
        }
      }
    }

    if self.coordinate_display_mode != CoordinateDisplayMode::Off {
      self.draw_coordinate_grid(ui, rect, transform, min_pos, max_pos);
    }
  }

  fn name(&self) -> &str {
    NAME
  }

  fn visible(&self) -> bool {
    self.layer_properties.visible
  }

  fn visible_mut(&mut self) -> &mut bool {
    &mut self.layer_properties.visible
  }

  fn ui_content(&mut self, ui: &mut Ui) {
    egui::ComboBox::from_label("tile provider")
      .selected_text(
        self.all_tile_loader[self.tile_loader_index]
          .name()
          .to_string(),
      )
      .show_ui(ui, |ui| {
        for (i, tile_loader) in self.all_tile_loader.iter().enumerate() {
          ui.selectable_value(
            &mut self.tile_loader_index,
            i,
            tile_loader.name().to_string(),
          );
        }
      });
    egui::ComboBox::from_label("tile source")
      .selected_text(self.tile_source.to_string())
      .show_ui(ui, |ui| {
        for s in [TileSource::All, TileSource::Cache, TileSource::Download] {
          ui.selectable_value(&mut self.tile_source, s, s.to_string());
        }
      });

    ui.separator();
    ui.label("Statistics:");

    // Zoom levels
    ui.horizontal(|ui| {
      ui.label("Ideal zoom:");
      ui.label(format!("{}", self.current_ideal_zoom));
    });
    ui.horizontal(|ui| {
      ui.label("Request zoom:");
      ui.label(format!("{}", self.current_request_zoom));
    });
    ui.horizontal(|ui| {
      ui.label("Max zoom:");
      ui.label(format!("{}", self.current_max_zoom));
    });

    ui.separator();

    // Tile counts
    let tiles_downloading = self.tile_loader().tiles_downloading();
    let tiles_queued = self.tile_loader().tiles_queued();
    let tiles_in_flight = self.in_flight_tiles.lock().unwrap().len();
    let tiles_loaded = self.loaded_tiles.len();

    // Estimate rendering: in_flight minus downloading = rendering/queued for rendering
    let tiles_rendering = tiles_in_flight.saturating_sub(tiles_downloading);

    ui.horizontal(|ui| {
      ui.label("Tiles loaded:");
      ui.label(format!("{tiles_loaded}"));
    });
    ui.horizontal(|ui| {
      ui.label("Tiles downloading:");
      ui.label(format!("{tiles_downloading}"));
    });
    ui.horizontal(|ui| {
      ui.label("Tiles queued:");
      ui.label(format!("{tiles_queued}"));
    });
    ui.horizontal(|ui| {
      ui.label("Tiles in flight:");
      ui.label(format!("{tiles_in_flight}"));
    });
    ui.horizontal(|ui| {
      ui.label("Tiles rendering:");
      ui.label(format!("{tiles_rendering}"));
    });

    ui.separator();
    ui.label("Tile Coordinate Display:");

    ui.horizontal(|ui| {
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
    });
  }

  fn closest_geometry_with_selection(&mut self, _pos: Pos2, _transform: &Transform) -> Option<f64> {
    None
  }

  fn handle_double_click(&mut self, _pos: Pos2, _transform: &Transform) -> bool {
    false
  }
}

fn draw_coordinate_text_overlay(ui: &mut Ui, clip_rect: Rect, tile: &Tile, tile_rect: &Rect) {
  let painter = ui.painter_at(clip_rect);

  let bg_width = 100.0;
  let bg_height = 60.0;
  let bg_rect = Rect::from_center_size(tile_rect.center(), egui::vec2(bg_width, bg_height));

  painter.rect_filled(
    bg_rect,
    egui::CornerRadius::same(5),
    Color32::from_rgba_unmultiplied(0, 0, 0, 180),
  );

  let font_id = egui::FontId::monospace(11.0);
  let text_color = Color32::WHITE;

  let lines = [
    format!("Z:{}", tile.zoom),
    format!("X:{}", tile.x),
    format!("Y:{}", tile.y),
  ];

  for (i, line) in lines.iter().enumerate() {
    #[expect(clippy::cast_precision_loss)]
    let text_pos = bg_rect.min + egui::vec2(8.0, 8.0 + i as f32 * 14.0);
    painter.text(
      text_pos,
      egui::Align2::LEFT_TOP,
      line,
      font_id.clone(),
      text_color,
    );
  }

  painter.rect_stroke(
    *tile_rect,
    egui::CornerRadius::ZERO,
    egui::Stroke::new(2.0, Color32::from_rgb(100, 100, 100)),
    egui::epaint::StrokeKind::Outside,
  );
}
