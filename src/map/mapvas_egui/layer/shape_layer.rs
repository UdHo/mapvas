use super::{
  Layer, LayerProperties, Searchable, SubLayerInfo, geometry_highlighting::GeometryHighlighter,
  geometry_rasterizer,
};
use rstar::{AABB, RTree, RTreeObject};

use crate::{
  config::{Config, HeadingStyle},
  map::{
    coordinates::{
      BoundingBox, PixelCoordinate, PixelPosition, TILE_SIZE, Tile, TileCoordinate, Transform,
      tiles_in_box,
    },
    geometry_collection::Geometry,
    map_event::{Layer as EventLayer, MapEvent},
  },
  profile_scope,
  task_tracker::{TaskCategory, TaskGuard},
};
use chrono::{DateTime, Duration, Utc};
use egui::{Color32, ColorImage, Pos2, Rect, Ui};
use regex::Regex;
use std::{
  collections::{HashMap, HashSet},
  sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
  },
};

pub(super) const SCROLL_AREA_MAX_HEIGHT: f32 = 600.0;
const HIGLIGHT_PIXEL_DISTANCE: f64 = 10.0;
/// Render resolution (pixels) for every geometry tile, regardless of zoom level.
const GEO_TILE_PIXEL_SIZE: u32 = 512;

/// Entry stored in the R-tree spatial index.
struct GeometryEntry {
  layer_id: String,
  shape_idx: usize,
  envelope: AABB<[f32; 2]>,
}

impl RTreeObject for GeometryEntry {
  type Envelope = AABB<[f32; 2]>;
  fn envelope(&self) -> Self::Envelope {
    self.envelope
  }
}

/// Search pattern that can be either a regex or literal string
enum SearchPattern {
  Regex(Regex),
  Literal(String),
}

/// A layer that draws shapes on the map.
pub struct ShapeLayer {
  shape_map: HashMap<String, Vec<Geometry<PixelCoordinate>>>,
  layer_visibility: HashMap<String, bool>,
  geometry_visibility: HashMap<(String, usize), bool>,
  collection_expansion: HashMap<(String, usize, Vec<usize>), bool>,
  nested_geometry_visibility: HashMap<(String, usize, Vec<usize>), bool>,
  recv: Arc<Receiver<MapEvent>>,
  send: Sender<MapEvent>,
  layer_properties: LayerProperties,
  geometry_highlighter: GeometryHighlighter,
  config: Config,
  // Temporal filtering state
  temporal_current_time: Option<DateTime<Utc>>,
  temporal_time_window: Option<Duration>,
  // Pending popup to display
  pending_detail_popup: Option<(egui::Pos2, PixelCoordinate, String, f64)>, // (click_pos, click_world_coord, content, creation_time)
  // Current transform for coordinate-to-pixel conversion
  current_transform: Transform,
  // Search results
  search_results: Vec<(String, usize, Vec<usize>)>, // (layer_id, shape_idx, nested_path)
  // Filter pattern (None = no filter active)
  filter_pattern: Option<SearchPattern>,
  // Track if a double-click action just occurred (separate from hover highlighting)
  pub(crate) just_double_clicked: Option<(String, usize, Vec<usize>)>, // (layer_id, shape_idx, nested_path)
  // Tile-based geometry cache (world-space tiles, invariant to pan; zoom-level-aware).
  tile_cache: HashMap<Tile, egui::TextureHandle>,
  /// R-tree spatial index for fast bounding-box queries (tile rendering and hover highlighting).
  spatial_index: RTree<GeometryEntry>,
  cache_version: u64,
  version: u64,
  // Async geometry tile rendering — mirrors the TileLayer channel pattern.
  geo_tile_sender: std::sync::mpsc::Sender<(Tile, ColorImage)>,
  geo_tile_receiver: std::sync::mpsc::Receiver<(Tile, ColorImage)>,
  in_flight_geo_tiles: Arc<Mutex<HashSet<Tile>>>,
  /// Scheduler task handles for queued (not yet executing) geo tile renders.
  /// Used to bump prefetch tiles to Current priority when they become visible.
  geo_tile_handles: HashMap<Tile, crate::render_scheduler::TaskHandle>,
  ctx: egui::Context,
  /// When true, render geo tiles synchronously (used in headless/test mode).
  headless: bool,
  /// Receives sub-layer visibility toggle commands from the HTTP server.
  vis_receiver: Receiver<(String, bool)>,
  /// Sender half exposed to `Remote` for sub-layer visibility toggles.
  vis_sender: Sender<(String, bool)>,
  /// Receives individual shape visibility toggle commands from the HTTP server.
  shape_vis_receiver: Receiver<(String, usize, bool)>,
  /// Sender half exposed to `Remote` for shape visibility toggles.
  shape_vis_sender: Sender<(String, usize, bool)>,
  /// Shared shape info cache that the HTTP endpoint reads directly.
  shape_info:
    Arc<std::sync::RwLock<std::collections::HashMap<String, Vec<crate::remote::ShapeInfo>>>>,
  /// Cached pixmap+texture for the polygon fills of the currently highlighted
  /// geometry. Avoids re-rasterizing every frame on a static hover.
  highlight_texture: Option<HighlightTextureCache>,
}

#[derive(PartialEq, Eq)]
pub(super) struct HighlightCacheKey {
  geometry_path: (String, usize, Vec<usize>),
  viewport: [u32; 4],
  transform: [u32; 3],
  version: u64,
}

pub(super) struct HighlightTextureCache {
  key: HighlightCacheKey,
  texture: egui::TextureHandle,
  screen_rect: egui::Rect,
}

mod search;
mod sidebar;
mod temporal;

/// CPU-heavy rasterization of a geometry tile into a `ColorImage`.
/// This is designed to be called from `tokio::task::spawn_blocking`.
#[allow(clippy::needless_pass_by_value)]
fn rasterize_geo_tile_to_image(
  geometries: Vec<Geometry<PixelCoordinate>>,
  tile: Tile,
  heading_style: HeadingStyle,
) -> Option<ColorImage> {
  let (nw, se) = tile.position();
  let tile_xmin = nw.x;
  let tile_ymin = nw.y;
  let ws = se.x - nw.x;
  #[allow(clippy::cast_precision_loss)]
  let zoom_factor = GEO_TILE_PIXEL_SIZE as f32 / ws;

  let tile_transform = Transform::default()
    .zoomed(zoom_factor)
    .translated(PixelPosition {
      x: -tile_xmin * zoom_factor,
      y: -tile_ymin * zoom_factor,
    });

  #[allow(clippy::cast_precision_loss)]
  let tile_rect = egui::Rect::from_min_max(
    egui::pos2(0.0, 0.0),
    egui::pos2(GEO_TILE_PIXEL_SIZE as f32, GEO_TILE_PIXEL_SIZE as f32),
  );

  let pixmap = geometry_rasterizer::rasterize_geometries(
    geometries.iter(),
    &tile_transform,
    tile_rect,
    heading_style,
  )?;

  // tiny_skia stores premultiplied RGBA; un-premultiply before handing to egui.
  let mut straight = Vec::with_capacity(pixmap.data().len());
  for p in pixmap.data().chunks_exact(4) {
    let a = p[3];
    if a == 0 {
      straight.extend_from_slice(&[0, 0, 0, 0]);
    } else {
      #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_lossless
      )]
      let inv = 255.0_f32 / f32::from(a);
      #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_lossless
      )]
      straight.push((f32::from(p[0]) * inv).min(255.0) as u8);
      #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_lossless
      )]
      straight.push((f32::from(p[1]) * inv).min(255.0) as u8);
      #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_lossless
      )]
      straight.push((f32::from(p[2]) * inv).min(255.0) as u8);
      straight.push(a);
    }
  }
  let px = GEO_TILE_PIXEL_SIZE as usize;
  Some(ColorImage::from_rgba_unmultiplied([px, px], &straight))
}

impl ShapeLayer {
  #[must_use]
  pub fn new(
    config: Config,
    ctx: egui::Context,
    shape_info: Arc<
      std::sync::RwLock<std::collections::HashMap<String, Vec<crate::remote::ShapeInfo>>>,
    >,
  ) -> Self {
    let (send, recv) = std::sync::mpsc::channel();
    let (geo_tile_sender, geo_tile_receiver) = std::sync::mpsc::channel();
    let (vis_sender, vis_receiver) = std::sync::mpsc::channel();
    let (shape_vis_sender, shape_vis_receiver) = std::sync::mpsc::channel();

    Self {
      shape_map: HashMap::new(),
      layer_visibility: HashMap::new(),
      geometry_visibility: HashMap::new(),
      collection_expansion: HashMap::new(),
      nested_geometry_visibility: HashMap::new(),
      recv: recv.into(),
      send,
      layer_properties: LayerProperties::default(),
      geometry_highlighter: GeometryHighlighter::new(),
      config,
      temporal_current_time: None,
      temporal_time_window: None,
      pending_detail_popup: None,
      current_transform: Transform::invalid(),
      search_results: Vec::new(),
      filter_pattern: None,
      just_double_clicked: None,
      tile_cache: HashMap::new(),
      spatial_index: RTree::new(),
      cache_version: 0,
      version: 0,
      geo_tile_sender,
      geo_tile_receiver,
      in_flight_geo_tiles: Arc::new(Mutex::new(HashSet::new())),
      geo_tile_handles: HashMap::new(),
      ctx,
      headless: false,
      vis_receiver,
      vis_sender,
      shape_vis_receiver,
      shape_vis_sender,
      shape_info,
      highlight_texture: None,
    }
  }

  /// Return the sender half so `Remote` can toggle sub-layer visibility.
  #[must_use]
  pub fn get_vis_sender(&self) -> Sender<(String, bool)> {
    self.vis_sender.clone()
  }

  /// Return the sender half so `Remote` can toggle individual shape visibility.
  #[must_use]
  pub fn get_shape_vis_sender(&self) -> Sender<(String, usize, bool)> {
    self.shape_vis_sender.clone()
  }

  /// Update highlighting based on mouse hover position
  fn update_hover_highlighting(&mut self, mouse_pos: egui::Pos2, transform: &Transform) {
    profile_scope!("ShapeLayer::update_hover_highlighting");

    // Use the R-tree to find only geometries near the mouse cursor.
    let world_pos = transform.invert().apply(mouse_pos.into());
    #[allow(clippy::cast_possible_truncation)]
    let world_radius = HIGLIGHT_PIXEL_DISTANCE as f32 / transform.zoom;
    let query = AABB::from_corners(
      [world_pos.x - world_radius, world_pos.y - world_radius],
      [world_pos.x + world_radius, world_pos.y + world_radius],
    );
    let candidates: Vec<(String, usize)> = self
      .spatial_index
      .locate_in_envelope_intersecting(&query)
      .map(|e| (e.layer_id.clone(), e.shape_idx))
      .collect();

    let mut closest_distance = f64::INFINITY;
    let mut closest_geometry: Option<(String, usize, Vec<usize>)> = None;

    for (layer_id, shape_idx) in candidates {
      if !*self.layer_visibility.get(&layer_id).unwrap_or(&true) {
        continue;
      }
      if !*self
        .geometry_visibility
        .get(&(layer_id.clone(), shape_idx))
        .unwrap_or(&true)
      {
        continue;
      }
      if let Some(shape) = self.shape_map.get(&layer_id).and_then(|s| s.get(shape_idx)) {
        self.find_closest_in_geometry(
          &layer_id,
          shape_idx,
          &[],
          shape,
          mouse_pos,
          transform,
          &mut closest_distance,
          &mut closest_geometry,
        );
      }
    }

    if let Some((layer_id, shape_idx, nested_path)) = closest_geometry {
      if closest_distance < HIGLIGHT_PIXEL_DISTANCE {
        self.highlight_geometry(&layer_id, shape_idx, &nested_path);
      } else {
        self.geometry_highlighter.clear_highlighting();
      }
    } else {
      self.geometry_highlighter.clear_highlighting();
    }
  }

  fn handle_new_shapes(&mut self) {
    let mut received = false;
    for event in self.recv.try_iter() {
      if let MapEvent::Layer(EventLayer { id, geometries }) = event {
        received = true;
        let l = self.shape_map.entry(id.clone()).or_default();
        let start_idx = l.len();
        l.extend(geometries);
        self.layer_visibility.entry(id.clone()).or_insert(true);

        for i in start_idx..l.len() {
          self
            .geometry_visibility
            .entry((id.clone(), i))
            .or_insert(true);
        }
      }
    }
    for (id, visible) in self.vis_receiver.try_iter() {
      self.layer_visibility.insert(id, visible);
      received = true;
    }
    for (layer_id, shape_idx, visible) in self.shape_vis_receiver.try_iter() {
      self
        .geometry_visibility
        .insert((layer_id, shape_idx), visible);
      received = true;
    }
    if received {
      self.version += 1;
      self.update_shape_info_cache();
    }
  }

  /// Update the shared shape info cache so the HTTP endpoint can read it directly.
  fn update_shape_info_cache(&self) {
    let Ok(mut cache) = self.shape_info.try_write() else {
      return;
    };
    cache.clear();
    for id in self.shape_map.keys() {
      cache.insert(id.clone(), self.collect_shape_info(id));
    }
  }

  fn invalidate_cache(&mut self) {
    self.version += 1;
  }

  /// Receive any geometry tiles that finished rendering on background threads.
  fn collect_new_geo_tile_data(&mut self, ui: &egui::Ui) {
    for (tile, image) in self.geo_tile_receiver.try_iter() {
      let handle = ui.ctx().load_texture(
        format!("geo_tile_{}_{}_{}", tile.zoom, tile.x, tile.y),
        image,
        egui::TextureOptions::default(),
      );
      self.tile_cache.insert(tile, handle);
      self.in_flight_geo_tiles.lock().unwrap().remove(&tile);
      self.geo_tile_handles.remove(&tile);
    }
  }

  /// Rebuild the R-tree spatial index from all current geometries.
  /// Called whenever `version` advances past `cache_version`.
  fn rebuild_spatial_index(&mut self) {
    let entries: Vec<GeometryEntry> = self
      .shape_map
      .iter()
      .flat_map(|(layer_id, shapes)| {
        shapes.iter().enumerate().filter_map(|(shape_idx, shape)| {
          let bbox = shape.bounding_box();
          if !bbox.is_valid() {
            return None;
          }
          Some(GeometryEntry {
            layer_id: layer_id.clone(),
            shape_idx,
            envelope: AABB::from_corners(
              [bbox.min_x(), bbox.min_y()],
              [bbox.max_x(), bbox.max_y()],
            ),
          })
        })
      })
      .collect();
    self.spatial_index = RTree::bulk_load(entries);
  }

  /// Return the `Tile`s visible in the current viewport, using the same zoom formula as `TileLayer`.
  #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
  fn compute_visible_geo_tiles(transform: &Transform, rect: Rect) -> Vec<Tile> {
    let max_dim = rect.width().max(rect.height());
    let zoom = ((transform.zoom * max_dim / TILE_SIZE).log2() as u8).saturating_add(2);
    let zoom = zoom.min(19); // cap at OSM max zoom to avoid explosion of tiny tiles
    let inv = transform.invert();
    let min_pos = TileCoordinate::from_pixel_position(inv.apply(rect.min.into()), zoom);
    let max_pos = TileCoordinate::from_pixel_position(inv.apply(rect.max.into()), zoom);
    tiles_in_box(min_pos, max_pos).collect()
  }

  /// Collect geometries that overlap the given `Tile`, applying all visibility / filter rules.
  fn collect_tile_geometries(&self, tile: Tile) -> Vec<Geometry<PixelCoordinate>> {
    let (nw, se) = tile.position();
    let query = AABB::from_corners([nw.x, nw.y], [se.x, se.y]);
    self
      .spatial_index
      .locate_in_envelope_intersecting(&query)
      .filter_map(|entry| {
        if !*self.layer_visibility.get(&entry.layer_id).unwrap_or(&true) {
          return None;
        }
        if !*self
          .geometry_visibility
          .get(&(entry.layer_id.clone(), entry.shape_idx))
          .unwrap_or(&true)
        {
          return None;
        }
        let shape = self.shape_map.get(&entry.layer_id)?.get(entry.shape_idx)?;
        if let Some(t) = self.temporal_current_time
          && !self.is_geometry_visible_at_time(shape, t)
        {
          return None;
        }
        if !self.geometry_matches_filter(shape) {
          return None;
        }
        self.filter_nested_visibility(&entry.layer_id, entry.shape_idx, &[], shape)
      })
      .collect()
  }

  /// Paint a cached tile onto the map using the current screen transform.
  fn paint_geo_tile(
    painter: &egui::Painter,
    handle: &egui::TextureHandle,
    tile: Tile,
    transform: &Transform,
  ) {
    let (nw, se) = tile.position();
    let screen_nw: egui::Pos2 = transform.apply(nw).into();
    let screen_se: egui::Pos2 = transform.apply(se).into();
    let tile_rect = egui::Rect::from_min_max(screen_nw, screen_se);
    painter.image(
      handle.id(),
      tile_rect,
      egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
      Color32::WHITE,
    );
  }

  /// Recursively filter a geometry tree based on nested visibility settings.
  /// Returns None if the geometry itself is hidden.
  fn filter_nested_visibility(
    &self,
    layer_id: &str,
    shape_idx: usize,
    path: &[usize],
    geometry: &Geometry<PixelCoordinate>,
  ) -> Option<Geometry<PixelCoordinate>> {
    let key = (layer_id.to_string(), shape_idx, path.to_vec());
    if !*self.nested_geometry_visibility.get(&key).unwrap_or(&true) {
      return None;
    }

    match geometry {
      Geometry::GeometryCollection(geometries, metadata) => {
        let filtered: Vec<_> = geometries
          .iter()
          .enumerate()
          .filter_map(|(i, g)| {
            let mut child_path = path.to_vec();
            child_path.push(i);
            self.filter_nested_visibility(layer_id, shape_idx, &child_path, g)
          })
          .collect();
        if filtered.is_empty() {
          None
        } else {
          Some(Geometry::GeometryCollection(filtered, metadata.clone()))
        }
      }
      other => {
        // Check temporal visibility for nested geometries
        if let Some(current_time) = self.temporal_current_time
          && !self.is_individual_geometry_visible_at_time(other, current_time)
        {
          return None;
        }
        Some(other.clone())
      }
    }
  }

  #[must_use]
  pub fn get_sender(&self) -> Sender<MapEvent> {
    self.send.clone()
  }

  /// Collect shape info for a given layer ID (used by HTTP query handler).
  fn collect_shape_info(&self, id: &str) -> Vec<crate::remote::ShapeInfo> {
    let Some(shapes) = self.shape_map.get(id) else {
      return vec![];
    };
    shapes
      .iter()
      .enumerate()
      .map(|(idx, shape)| {
        let (label, shape_type) = match shape {
          Geometry::Point(_, meta) => (meta.label.as_ref().map(|l| l.name.clone()), "Point"),
          Geometry::LineString(_, meta) => {
            (meta.label.as_ref().map(|l| l.name.clone()), "LineString")
          }
          Geometry::Polygon(_, meta) => (meta.label.as_ref().map(|l| l.name.clone()), "Polygon"),
          Geometry::GeometryCollection(_, meta) => {
            (meta.label.as_ref().map(|l| l.name.clone()), "Collection")
          }
          Geometry::Heatmap(_, meta) => (meta.label.as_ref().map(|l| l.name.clone()), "Heatmap"),
        };
        crate::remote::ShapeInfo {
          index: idx,
          label,
          shape_type,
          visible: *self
            .geometry_visibility
            .get(&(id.to_owned(), idx))
            .unwrap_or(&true),
        }
      })
      .collect()
  }
}

const NAME: &str = "Shape Layer";

impl Layer for ShapeLayer {
  /// Get temporal range from all geometries in this layer
  fn get_temporal_range(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  ) {
    self.get_temporal_range()
  }
  fn process_pending_events(&mut self) {
    self.handle_new_shapes();
  }

  fn discard_pending_events(&mut self) {
    for _event in self.recv.try_iter() {}
  }

  fn set_headless(&mut self) {
    self.headless = true;
  }

  #[allow(clippy::too_many_lines)]
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, rect: Rect) {
    profile_scope!("ShapeLayer::draw");

    // Store current transform for popup positioning
    self.current_transform = *transform;

    self.handle_new_shapes();

    if !self.visible() {
      return;
    }

    // Track mouse position and find closest geometry for hover highlighting
    if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
      self.update_hover_highlighting(mouse_pos, transform);
    }

    // Receive any tiles that finished rendering on background threads.
    self.collect_new_geo_tile_data(ui);

    // Rebuild spatial index and clear stale tile textures when data/visibility changed.
    if self.cache_version != self.version {
      self.tile_cache.clear();
      self.in_flight_geo_tiles.lock().unwrap().clear();
      self.geo_tile_handles.clear();
      self.rebuild_spatial_index();
      self.cache_version = self.version;
    }

    // Paint visible tiles (spawn background render if not yet cached).
    let total_geometries: usize = self.shape_map.values().map(Vec::len).sum();
    let has_heatmap = self.shape_map.values().flatten().any(|g| {
      matches!(g, Geometry::Heatmap(_, _))
        || matches!(g, Geometry::GeometryCollection(children, _)
          if children.iter().any(|c| matches!(c, Geometry::Heatmap(_, _))))
    });
    let use_sync_render = self.headless || (!has_heatmap && total_geometries <= 10_000);
    let painter = ui.painter_at(rect);
    for tile in Self::compute_visible_geo_tiles(transform, rect) {
      // Quick reject: check if the R-tree has any geometry overlapping this tile.
      let (nw, se) = tile.position();
      let tile_envelope = AABB::from_corners([nw.x, nw.y], [se.x, se.y]);
      if self
        .spatial_index
        .locate_in_envelope_intersecting(&tile_envelope)
        .next()
        .is_none()
      {
        continue;
      }
      if !self.tile_cache.contains_key(&tile) {
        let already_in_flight = self.in_flight_geo_tiles.lock().unwrap().contains(&tile);
        if already_in_flight {
          // Tile was preloaded at lower priority — promote it now that it is visible.
          if let Some(handle) = self.geo_tile_handles.get(&tile) {
            handle.bump(crate::map::coordinates::TilePriority::Current);
          }
        } else {
          let geometries = self.collect_tile_geometries(tile);
          if !geometries.is_empty() {
            if use_sync_render {
              // Render synchronously: small datasets (≤10k) or headless mode.
              if let Some(image) =
                rasterize_geo_tile_to_image(geometries, tile, self.config.heading_style)
              {
                let handle = ui.ctx().load_texture(
                  format!("geo_tile_{}_{}_{}", tile.zoom, tile.x, tile.y),
                  image,
                  egui::TextureOptions::LINEAR,
                );
                self.tile_cache.insert(tile, handle);
              }
            } else {
              self.in_flight_geo_tiles.lock().unwrap().insert(tile);
              let sender = self.geo_tile_sender.clone();
              let ctx = self.ctx.clone();
              let in_flight = self.in_flight_geo_tiles.clone();
              let heading_style = self.config.heading_style;
              let (rx, task_handle) = crate::render_scheduler::RENDER_SCHEDULER
                .submit(crate::map::coordinates::TilePriority::Current, move || {
                  rasterize_geo_tile_to_image(geometries, tile, heading_style)
                });
              self.geo_tile_handles.insert(tile, task_handle);
              tokio::spawn(async move {
                let task_name = format!("geo-render-{}-{}-{}", tile.zoom, tile.x, tile.y);
                let _guard = TaskGuard::new(task_name, TaskCategory::GeoRender);
                match rx.await {
                  Ok(Some(image)) => {
                    let _ = sender.send((tile, image));
                  }
                  _ => {
                    in_flight.lock().unwrap().remove(&tile);
                  }
                }
                ctx.request_repaint();
              });
            }
          }
        }
      }
      if let Some(handle) = self.tile_cache.get(&tile) {
        Self::paint_geo_tile(&painter, handle, tile, transform);
      }
    }

    // Pre-render tiles one zoom level deeper so they are ready when the user zooms in.
    // Only in async mode — sync mode renders instantly on demand anyway.
    if !use_sync_render {
      for tile in Self::compute_visible_geo_tiles(transform, rect)
        .into_iter()
        .flat_map(|t| t.children())
      {
        if self.tile_cache.contains_key(&tile) {
          continue;
        }
        let already_in_flight = self.in_flight_geo_tiles.lock().unwrap().contains(&tile);
        if already_in_flight {
          continue;
        }
        let (nw, se) = tile.position();
        let tile_envelope = AABB::from_corners([nw.x, nw.y], [se.x, se.y]);
        if self
          .spatial_index
          .locate_in_envelope_intersecting(&tile_envelope)
          .next()
          .is_none()
        {
          continue;
        }
        let geometries = self.collect_tile_geometries(tile);
        if geometries.is_empty() {
          continue;
        }
        self.in_flight_geo_tiles.lock().unwrap().insert(tile);
        let sender = self.geo_tile_sender.clone();
        let ctx = self.ctx.clone();
        let in_flight = self.in_flight_geo_tiles.clone();
        let heading_style = self.config.heading_style;
        let (rx, task_handle) = crate::render_scheduler::RENDER_SCHEDULER.submit(
          crate::map::coordinates::TilePriority::ZoomLevel,
          move || rasterize_geo_tile_to_image(geometries, tile, heading_style),
        );
        self.geo_tile_handles.insert(tile, task_handle);
        tokio::spawn(async move {
          let task_name = format!("geo-render-{}-{}-{}", tile.zoom, tile.x, tile.y);
          let _guard = TaskGuard::new(task_name, TaskCategory::GeoRender);
          match rx.await {
            Ok(Some(image)) => {
              let _ = sender.send((tile, image));
            }
            _ => {
              in_flight.lock().unwrap().remove(&tile);
            }
          }
          ctx.request_repaint();
        });
      }
    }

    // Draw highlight overlay for the currently-hovered geometry.
    // Polygon fills go through tiny-skia (cached as a texture) to avoid egui's
    // fan-triangulation; points, lines, and polygon strokes go through egui.
    self.draw_highlight_overlay(ui, transform, rect);

    // Handle pending detail popup from double-click as lightweight positioned window
    // This needs to be in draw() so it shows regardless of sidebar state
    if let Some((click_pos, click_world_coord, detail_info, creation_time)) =
      &self.pending_detail_popup
    {
      // Extract values to avoid borrow checker issues
      let click_pos = *click_pos;
      let click_world_coord = *click_world_coord;
      let detail_info = detail_info.clone();
      let creation_time = *creation_time;

      // Calculate how long the popup has been visible
      let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
      let time_since_creation = current_time - creation_time;

      // For the first 500ms, use click position for better UX
      // After that, track the click world coordinate so popup follows map movement
      let screen_pos = if time_since_creation < 0.5 {
        click_pos
      } else if self.current_transform.is_invalid() {
        // Fallback to original click position if transform is invalid
        click_pos
      } else {
        // Convert click world coordinate to current screen position
        let pixel_pos = self.current_transform.apply(click_world_coord);
        egui::pos2(pixel_pos.x, pixel_pos.y)
      };

      let mut show_popup = true;

      egui::Window::new("Geometry Info")
        .id(egui::Id::new("geometry_detail_context_menu"))
        .open(&mut show_popup)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .title_bar(false)
        .frame(egui::Frame::popup(ui.style()))
        .fixed_pos(screen_pos)
        .show(ui.ctx(), |ui| {
          ui.set_min_width(280.0);
          ui.set_max_width(400.0);

          // Split detail info into lines and format nicely
          for line in detail_info.lines() {
            if line.starts_with("📍")
              || line.starts_with("📏")
              || line.starts_with("⬟")
              || line.starts_with("📦")
            {
              ui.strong(line);
            } else if line.starts_with("Layer:") || line.starts_with("Coordinates:") {
              ui.label(line);
            } else if line.starts_with("Label:") || line.starts_with("Timestamp:") {
              ui.small(line);
            } else {
              ui.label(line);
            }
          }
        });

      if show_popup {
        // Also close on any click or escape key, but ignore clicks for a short period after creation
        ui.ctx().input(|i| {
          // Ignore clicks for 200ms after popup creation to prevent immediate closure
          let ignore_clicks = time_since_creation < 0.2;

          if (!ignore_clicks && i.pointer.any_click()) || i.key_pressed(egui::Key::Escape) {
            self.pending_detail_popup = None;
          }
        });
      } else {
        self.pending_detail_popup = None; // Clear if window was closed
      }
    }
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    let bb = self
      .shape_map
      .iter()
      .filter(|(layer_id, _)| *self.layer_visibility.get(*layer_id).unwrap_or(&true))
      .flat_map(|(layer_id, shapes)| {
        shapes.iter().enumerate().filter_map(|(shape_idx, shape)| {
          let geometry_key = (layer_id.clone(), shape_idx);
          if *self.geometry_visibility.get(&geometry_key).unwrap_or(&true) {
            Some(shape.bounding_box())
          } else {
            None
          }
        })
      })
      .fold(BoundingBox::default(), |acc, b| acc.extend(&b));

    bb.is_valid().then_some(bb)
  }

  fn clear(&mut self) {
    self.shape_map.clear();
    self.layer_visibility.clear();
    self.geometry_visibility.clear();
    self.collection_expansion.clear();
    self.nested_geometry_visibility.clear();
    self.tile_cache.clear();
    self.spatial_index = RTree::new();
    self.invalidate_cache();
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

  fn ui(&mut self, ui: &mut Ui) {
    let has_highlighted_geometry = self.geometry_highlighter.has_highlighted_geometry();
    let layer_id = egui::Id::new("shape_layer_header");

    let mut layer_header = egui::CollapsingHeader::new(self.name().to_owned())
      .id_salt(layer_id)
      .default_open(has_highlighted_geometry);

    // Open sidebar on double-click (but not hover)
    if self.just_double_clicked.is_some() {
      layer_header = layer_header.open(Some(true));
    }

    layer_header.show(ui, |ui| {
      ui.checkbox(self.visible_mut(), "visible");
      self.ui_content(ui);
    });
  }

  fn ui_content(&mut self, ui: &mut Ui) {
    profile_scope!("ShapeLayer::ui_content");
    let has_highlighted_geometry = self.geometry_highlighter.has_highlighted_geometry();
    let shapes_header_id = egui::Id::new("shapes_header");

    let mut shapes_header = egui::CollapsingHeader::new("Shapes")
      .id_salt(shapes_header_id)
      .default_open(has_highlighted_geometry);

    // Open sidebar on double-click (but not hover)
    if self.just_double_clicked.is_some() {
      shapes_header = shapes_header.open(Some(true));
    }

    shapes_header.show(ui, |ui| {
      self.show_shape_layers(ui);
    });

    let _ = self.geometry_highlighter.was_just_highlighted();
    // Clear double-click flag after UI update
    self.just_double_clicked = None;
  }

  fn has_highlighted_geometry(&self) -> bool {
    self.geometry_highlighter.has_highlighted_geometry()
  }

  fn has_double_click_action(&self) -> bool {
    self.just_double_clicked.is_some()
  }

  fn as_searchable(&self) -> Option<&dyn Searchable> {
    Some(self)
  }

  fn as_searchable_mut(&mut self) -> Option<&mut dyn Searchable> {
    Some(self)
  }

  fn closest_geometry_with_selection(&mut self, pos: Pos2, transform: &Transform) -> Option<f64> {
    let world_pos = transform.invert().apply(pos.into());
    #[allow(clippy::cast_possible_truncation)]
    let world_radius = HIGLIGHT_PIXEL_DISTANCE as f32 / transform.zoom;
    let query = AABB::from_corners(
      [world_pos.x - world_radius, world_pos.y - world_radius],
      [world_pos.x + world_radius, world_pos.y + world_radius],
    );
    let candidates: Vec<(String, usize)> = self
      .spatial_index
      .locate_in_envelope_intersecting(&query)
      .map(|e| (e.layer_id.clone(), e.shape_idx))
      .collect();

    let mut closest_distance = f64::INFINITY;
    let mut closest_geometry: Option<(String, usize, Vec<usize>)> = None;

    for (layer_id, shape_idx) in candidates {
      if !*self.layer_visibility.get(&layer_id).unwrap_or(&true) {
        continue;
      }
      if !*self
        .geometry_visibility
        .get(&(layer_id.clone(), shape_idx))
        .unwrap_or(&true)
      {
        continue;
      }
      let Some(shape) = self.shape_map.get(&layer_id).and_then(|s| s.get(shape_idx)) else {
        continue;
      };
      if let Some(current_time) = self.temporal_current_time
        && !self.is_geometry_visible_at_time(shape, current_time)
      {
        continue;
      }
      self.find_closest_in_geometry(
        &layer_id,
        shape_idx,
        &[],
        shape,
        pos,
        transform,
        &mut closest_distance,
        &mut closest_geometry,
      );
    }

    // If we found a closest geometry within tolerance, show popup (hover highlighting handled separately)
    if let Some((layer_id, shape_idx, nested_path)) = closest_geometry {
      // Check if the geometry is within reasonable click distance (use same as hover highlighting)
      if closest_distance < HIGLIGHT_PIXEL_DISTANCE {
        if let Some(detail_info) =
          self.generate_geometry_detail_info(&layer_id, shape_idx, &nested_path)
        {
          // Convert click position to world coordinate for tracking
          let click_world_coord = transform.invert().apply(pos.into());

          // Store current time to ignore immediate clicks
          let creation_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
          // Store click position and its world coordinate
          self.pending_detail_popup = Some((pos, click_world_coord, detail_info, creation_time));
        }

        // Handle collection expansion for double-clicks on GeometryCollections
        if let Some(shapes) = self.shape_map.get(&layer_id)
          && let Some(clicked_shape) = shapes.get(shape_idx)
        {
          // Check if we clicked on a collection (either top-level or nested)
          let clicked_geometry = Self::get_geometry_at_path(clicked_shape, &nested_path);
          if let Some(Geometry::GeometryCollection(_, _)) = clicked_geometry {
            // Toggle expansion state for this collection
            let collection_key = (layer_id.clone(), shape_idx, nested_path.clone());
            let current_expanded = *self
              .collection_expansion
              .get(&collection_key)
              .unwrap_or(&false);
            self
              .collection_expansion
              .insert(collection_key, !current_expanded);
          }
        }

        // Set double-click flag for sidebar expansion (any geometry type)
        self.just_double_clicked = Some((layer_id.clone(), shape_idx, nested_path.clone()));
        return Some(closest_distance);
      }
    }

    None
  }

  fn update_config(&mut self, config: &crate::config::Config) {
    if self.config.heading_style != config.heading_style {
      self.invalidate_cache();
    }
    self.config = config.clone();
  }

  fn set_temporal_filter(
    &mut self,
    current_time: Option<DateTime<Utc>>,
    time_window: Option<Duration>,
  ) {
    if self.temporal_current_time != current_time || self.temporal_time_window != time_window {
      self.invalidate_cache();
    }
    self.temporal_current_time = current_time;
    self.temporal_time_window = time_window;
  }

  fn sub_layers(&self) -> Vec<SubLayerInfo> {
    self
      .shape_map
      .iter()
      .map(|(id, geometries)| SubLayerInfo {
        id: id.clone(),
        visible: *self.layer_visibility.get(id).unwrap_or(&true),
        shape_count: geometries.len(),
      })
      .collect()
  }

  fn set_sub_layer_visible(&mut self, id: &str, visible: bool) {
    self.layer_visibility.insert(id.to_owned(), visible);
    self.version += 1;
  }

  fn shape_bounding_box(&self, layer_id: &str, shape_idx: usize) -> Option<BoundingBox> {
    let bb = self.shape_map.get(layer_id)?.get(shape_idx)?.bounding_box();
    bb.is_valid().then_some(bb)
  }

  fn sub_layer_shapes(&self, id: &str) -> Vec<crate::remote::ShapeInfo> {
    self.collect_shape_info(id)
  }

  fn sub_layer_bounding_box(&self, id: &str) -> Option<BoundingBox> {
    let bb = self
      .shape_map
      .get(id)?
      .iter()
      .enumerate()
      .filter(|(shape_idx, _)| {
        *self
          .geometry_visibility
          .get(&(id.to_owned(), *shape_idx))
          .unwrap_or(&true)
      })
      .map(|(_, shape)| shape.bounding_box())
      .fold(BoundingBox::default(), |acc, b| acc.extend(&b));
    bb.is_valid().then_some(bb)
  }

  fn handle_double_click(&mut self, _pos: Pos2, _transform: &Transform) -> bool {
    // This method is not used - double-click handling happens in closest_geometry_with_selection
    false
  }
}

impl Searchable for ShapeLayer {
  fn search_geometries(&mut self, query: &str) {
    ShapeLayer::search_geometries(self, query);
  }
  fn next_search_result(&mut self) -> bool {
    ShapeLayer::next_search_result(self)
  }
  fn previous_search_result(&mut self) -> bool {
    ShapeLayer::previous_search_result(self)
  }
  fn search_results_count(&self) -> usize {
    self.get_search_results().len()
  }
  fn show_search_result_popup(&mut self) {
    ShapeLayer::show_search_result_popup(self);
  }
  fn filter_geometries(&mut self, query: &str) {
    ShapeLayer::filter_geometries(self, query);
  }
  fn clear_filter(&mut self) {
    ShapeLayer::clear_filter(self);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::map::{
    coordinates::{PixelCoordinate, Transform},
    geometry_collection::{Geometry, Metadata},
  };
  use egui::Pos2;
  use std::sync::mpsc;

  #[test]
  fn test_find_closest_nested_geometry() {
    let shape_layer = ShapeLayer::new_with_test_receiver();

    // Create test geometries - a collection with nested individual geometries
    let point1 = Geometry::Point(PixelCoordinate { x: 100.0, y: 100.0 }, Metadata::default());
    let point2 = Geometry::Point(PixelCoordinate { x: 200.0, y: 200.0 }, Metadata::default());
    let line1 = Geometry::LineString(
      vec![
        PixelCoordinate { x: 150.0, y: 150.0 },
        PixelCoordinate { x: 160.0, y: 160.0 },
      ],
      Metadata::default(),
    );

    let nested_collection =
      Geometry::GeometryCollection(vec![point1, point2, line1], Metadata::default());

    // Create an identity transform (no scaling/translation)
    let transform = Transform::default();

    // Test case 1: Click closest to point1 (100, 100)
    let click_pos = Pos2::new(105.0, 105.0); // Very close to point1
    let mut closest_distance = f64::INFINITY;
    let mut closest_geometry: Option<(String, usize, Vec<usize>)> = None;

    shape_layer.find_closest_in_geometry(
      "test_layer",
      0,
      &Vec::new(),
      &nested_collection,
      click_pos,
      &transform,
      &mut closest_distance,
      &mut closest_geometry,
    );

    // Should find the first nested geometry (point1) at path [0]
    assert!(closest_geometry.is_some());
    let (layer_id, shape_idx, nested_path) = closest_geometry.unwrap();
    assert_eq!(layer_id, "test_layer");
    assert_eq!(shape_idx, 0);
    assert_eq!(nested_path, vec![0]); // First nested geometry
    assert!(closest_distance < 10.0); // Should be very close

    // Test case 2: Click closest to point2 (200, 200)
    let click_pos = Pos2::new(195.0, 195.0); // Very close to point2
    let mut closest_distance = f64::INFINITY;
    let mut closest_geometry: Option<(String, usize, Vec<usize>)> = None;

    shape_layer.find_closest_in_geometry(
      "test_layer",
      0,
      &Vec::new(),
      &nested_collection,
      click_pos,
      &transform,
      &mut closest_distance,
      &mut closest_geometry,
    );

    // Should find the second nested geometry (point2) at path [1]
    assert!(closest_geometry.is_some());
    let (layer_id, shape_idx, nested_path) = closest_geometry.unwrap();
    assert_eq!(layer_id, "test_layer");
    assert_eq!(shape_idx, 0);
    assert_eq!(nested_path, vec![1]); // Second nested geometry
    assert!(closest_distance < 10.0); // Should be very close

    // Test case 3: Click closest to line1 (around 155, 155)
    let click_pos = Pos2::new(155.0, 155.0); // On the line
    let mut closest_distance = f64::INFINITY;
    let mut closest_geometry: Option<(String, usize, Vec<usize>)> = None;

    shape_layer.find_closest_in_geometry(
      "test_layer",
      0,
      &Vec::new(),
      &nested_collection,
      click_pos,
      &transform,
      &mut closest_distance,
      &mut closest_geometry,
    );

    // Should find the third nested geometry (line1) at path [2]
    assert!(closest_geometry.is_some());
    let (layer_id, shape_idx, nested_path) = closest_geometry.unwrap();
    assert_eq!(layer_id, "test_layer");
    assert_eq!(shape_idx, 0);
    assert_eq!(nested_path, vec![2]); // Third nested geometry (line)
    assert!(closest_distance < 10.0); // Should be close to the line
  }

  impl ShapeLayer {
    // Helper method for testing
    #[allow(clippy::arc_with_non_send_sync)]
    fn new_with_test_receiver() -> Self {
      let (send, recv) = mpsc::channel();
      Self {
        shape_map: HashMap::new(),
        layer_visibility: HashMap::new(),
        geometry_visibility: HashMap::new(),
        collection_expansion: HashMap::new(),
        nested_geometry_visibility: HashMap::new(),
        recv: Arc::new(recv),
        send,
        layer_properties: crate::map::mapvas_egui::layer::LayerProperties { visible: true },
        geometry_highlighter: GeometryHighlighter::new(),
        config: crate::config::Config::new(),
        temporal_current_time: None,
        temporal_time_window: None,
        pending_detail_popup: None,
        current_transform: Transform::invalid(),
        search_results: Vec::new(),
        filter_pattern: None,
        just_double_clicked: None,
        tile_cache: HashMap::new(),
        spatial_index: RTree::new(),
        cache_version: 0,
        version: 0,
        geo_tile_sender: {
          let (s, _) = mpsc::channel();
          s
        },
        geo_tile_receiver: {
          let (_, r) = mpsc::channel();
          r
        },
        in_flight_geo_tiles: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        geo_tile_handles: HashMap::new(),
        ctx: egui::Context::default(),
        headless: false,
        vis_receiver: {
          let (_, r) = mpsc::channel();
          r
        },
        vis_sender: {
          let (s, _) = mpsc::channel();
          s
        },
        shape_vis_receiver: {
          let (_, r) = mpsc::channel();
          r
        },
        shape_vis_sender: {
          let (s, _) = mpsc::channel();
          s
        },
        shape_info: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        highlight_texture: None,
      }
    }
  }
}
