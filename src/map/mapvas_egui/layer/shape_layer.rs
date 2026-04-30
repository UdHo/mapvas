use super::{
  Layer, LayerProperties, Searchable, SubLayerInfo, geometry_highlighting::GeometryHighlighter,
  geometry_rasterizer, geometry_selection,
};
use rstar::{AABB, RTree, RTreeObject};

use crate::{
  config::{Config, HeadingStyle},
  map::{
    coordinates::{
      BoundingBox, Coordinate, PixelCoordinate, PixelPosition, TILE_SIZE, Tile, TileCoordinate,
      Transform, WGS84Coordinate, tiles_in_box,
    },
    geometry_collection::{Geometry, Metadata, Style},
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
  fmt::Write,
  sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
  },
};

const SCROLL_AREA_MAX_HEIGHT: f32 = 600.0;
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
struct HighlightCacheKey {
  geometry_path: (String, usize, Vec<usize>),
  viewport: [u32; 4],
  transform: [u32; 3],
  version: u64,
}

struct HighlightTextureCache {
  key: HighlightCacheKey,
  texture: egui::TextureHandle,
  screen_rect: egui::Rect,
}

fn truncate_label_by_width(ui: &egui::Ui, label: &str, available_width: f32) -> (String, bool) {
  // Ensure minimum available width
  if available_width < 20.0 {
    return ("...".to_string(), true);
  }

  let chars: Vec<char> = label.chars().collect();

  // Fast fallback for very long strings to prevent hanging
  if chars.len() > 200 {
    let truncated: String = chars[..50].iter().collect();
    return (format!("{truncated}..."), true);
  }

  let font_id = ui.style().text_styles.get(&egui::TextStyle::Body).unwrap();
  let ellipsis = "...";

  // Measure using egui's text measurement utilities
  let galley = ui
    .ctx()
    .fonts_mut(|f| f.layout_no_wrap(label.to_string(), font_id.clone(), egui::Color32::BLACK));
  let full_width = galley.size().x;

  // Add some safety margin to prevent edge cases
  let safe_available_width = available_width - 5.0;

  if full_width <= safe_available_width {
    return (label.to_string(), false);
  }

  // Find the longest substring that fits with ellipsis
  let ellipsis_galley = ui
    .ctx()
    .fonts_mut(|f| f.layout_no_wrap(ellipsis.to_string(), font_id.clone(), egui::Color32::BLACK));
  let ellipsis_width = ellipsis_galley.size().x;

  // If even ellipsis doesn't fit, return just dots
  if ellipsis_width > safe_available_width {
    return ("...".to_string(), true);
  }

  let mut best_len = 0;

  // Use binary search for efficiency with long strings
  let mut left = 0;
  let mut right = chars.len().min(100); // Cap to prevent excessive measurements

  while left <= right {
    let mid = usize::midpoint(left, right);
    if mid == 0 {
      break;
    }

    let substring: String = chars[..mid].iter().collect();
    let substring_galley = ui
      .ctx()
      .fonts_mut(|f| f.layout_no_wrap(substring, font_id.clone(), egui::Color32::BLACK));
    let test_width = substring_galley.size().x + ellipsis_width;

    if test_width <= safe_available_width {
      best_len = mid;
      left = mid + 1;
    } else {
      right = mid - 1;
    }
  }

  if best_len == 0 {
    (ellipsis.to_string(), true)
  } else {
    let truncated: String = chars[..best_len].iter().collect();
    (format!("{truncated}{ellipsis}"), true)
  }
}

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

  #[allow(clippy::too_many_lines)]
  fn show_shape_layers(&mut self, ui: &mut egui::Ui) {
    let layer_ids: Vec<String> = self.shape_map.keys().cloned().collect();

    for layer_id in layer_ids {
      let shapes_count = self.shape_map.get(&layer_id).map_or(0, Vec::len);

      // Check if any geometry in this layer is highlighted
      let has_highlighted_geometry = self.geometry_highlighter.has_highlighted_geometry();

      let header_id = egui::Id::new(format!("shape_layer_{layer_id}"));

      let font_id = ui.style().text_styles.get(&egui::TextStyle::Body).unwrap();
      let reserved_galley = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
          "📁  (9999) ".to_string(),
          font_id.clone(),
          egui::Color32::BLACK,
        )
      });
      let reserved_width = reserved_galley.size().x + 60.0;
      let available_width = (ui.available_width() - reserved_width).max(30.0);
      let (truncated_layer_id, was_truncated) =
        truncate_label_by_width(ui, &layer_id, available_width);
      let mut header =
        egui::CollapsingHeader::new(format!("📁 {truncated_layer_id} ({shapes_count})"))
          .id_salt(header_id)
          .default_open(has_highlighted_geometry);

      if was_truncated {
        header = header.show_background(true);
      }

      // Open sidebar on double-click (but not hover)
      if let Some((clicked_layer, _, _)) = &self.just_double_clicked
        && clicked_layer == &layer_id
      {
        header = header.open(Some(true));
      }

      let header_response = header.show(ui, |ui| {
        let shapes_count = self.shape_map.get(&layer_id).map_or(0, Vec::len);
        let row_height = ui.spacing().interact_size.y;
        let scroll_id = egui::Id::new(format!("layer_scroll_{layer_id}"));

        let mut scroll_area = egui::ScrollArea::vertical()
          .id_salt(scroll_id)
          .max_height(SCROLL_AREA_MAX_HEIGHT);

        // Jump directly to the double-clicked row. Row index = shape index (no filter).
        if let Some((clicked_layer, clicked_idx, _)) = &self.just_double_clicked
          && clicked_layer == &layer_id
        {
          #[allow(clippy::cast_precision_loss)]
          let offset = (*clicked_idx as f32 * (row_height + ui.spacing().item_spacing.y)
            - SCROLL_AREA_MAX_HEIGHT / 2.0)
            .max(0.0);
          scroll_area = scroll_area.vertical_scroll_offset(offset);
        }

        scroll_area.show_rows(ui, row_height, shapes_count, |ui, row_range| {
          for idx in row_range {
            if let Some(shape) = self.shape_map.get(&layer_id).and_then(|s| s.get(idx)) {
              let shape = shape.clone();
              self.show_shape_ui(ui, &layer_id, idx, &shape);
            }
          }
        });
      });

      let header_resp = header_response.header_response;
      if was_truncated && header_resp.clicked() {
        ui.memory_mut(|mem| {
          mem.data.insert_temp(
            egui::Id::new(format!("layer_popup_{layer_id}")),
            layer_id.clone(),
          );
        });
      }

      header_resp.context_menu(|ui| {
        let layer_visible = *self.layer_visibility.get(&layer_id).unwrap_or(&true);

        self.show_visibility_button(ui, layer_visible, "Layer", |this| {
          this
            .layer_visibility
            .insert(layer_id.clone(), !layer_visible);
          this.invalidate_cache();
        });

        ui.separator();

        if ui.button("🗑 Delete Layer").clicked() {
          self.shape_map.remove(&layer_id);
          self.layer_visibility.remove(&layer_id);
          self
            .geometry_visibility
            .retain(|(lid, _), _| lid != &layer_id);
          self.invalidate_cache();
          ui.close();
        }
      });

      let popup_id = egui::Id::new(format!("layer_popup_{layer_id}"));
      if let Some(full_text) = ui.memory(|mem| mem.data.get_temp::<String>(popup_id)) {
        let mut is_open = true;
        egui::Window::new("Full Layer Name")
          .id(popup_id)
          .open(&mut is_open)
          .collapsible(false)
          .resizable(true)
          .movable(true)
          .default_width(500.0)
          .min_width(400.0)
          .max_width(800.0)
          .max_height(400.0)
          .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical()
              .max_height(300.0)
              .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                  ui.add(egui::Label::new(&full_text).wrap());
                });
              });
          });

        if !is_open {
          ui.memory_mut(|mem| mem.data.remove::<String>(popup_id));
        }
      }
    }

    // Handle nested collection label popups once per frame (not per row).
    for (layer_id, shapes) in &self.shape_map {
      for (shape_idx, shape) in shapes.iter().enumerate() {
        if let Geometry::GeometryCollection(geometries, _) = shape {
          Self::check_nested_popups_recursive(ui, layer_id, shape_idx, geometries, &mut Vec::new());
        }
      }
    }

    // Show color picker windows once per frame (not per row).
    let mut color_picker_requests = Vec::new();
    for (layer_id, shapes) in &self.shape_map {
      for (shape_idx, shape) in shapes.iter().enumerate() {
        let popup_id = egui::Id::new(format!("color_picker_{layer_id}_{shape_idx}"));
        if ui
          .memory(|mem| mem.data.get_temp::<bool>(popup_id))
          .unwrap_or(false)
        {
          let window_title = match shape {
            Geometry::Polygon(_, _) => "Choose Colors",
            _ => "Choose Color",
          };
          color_picker_requests.push((layer_id.clone(), shape_idx, window_title, popup_id));
        }
      }
    }

    for (layer_id, shape_idx, window_title, popup_id) in color_picker_requests {
      let mut is_open = true;
      egui::Window::new(window_title)
        .id(popup_id)
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .movable(true)
        .default_width(250.0)
        .show(ui.ctx(), |ui| {
          if let Some(shapes) = self.shape_map.get_mut(&layer_id)
            && let Some(shape) = shapes.get_mut(shape_idx)
          {
            let metadata = match shape {
              Geometry::Point(_, metadata)
              | Geometry::LineString(_, metadata)
              | Geometry::Polygon(_, metadata)
              | Geometry::GeometryCollection(_, metadata)
              | Geometry::Heatmap(_, metadata) => metadata,
            };

            if metadata.style.is_none() {
              metadata.style = Some(crate::map::geometry_collection::Style::default());
            }

            if let Some(style) = &metadata.style {
              let mut stroke_color = style.color();
              let mut fill_color = style.fill_color();
              let is_polygon = matches!(shape, Geometry::Polygon(_, _));

              if is_polygon {
                ui.label("Stroke Color:");
                if ui.color_edit_button_srgba(&mut stroke_color).changed() {
                  self.update_shape_stroke_color(&layer_id, shape_idx, stroke_color);
                }

                let mut stroke_hsva = egui::ecolor::Hsva::from(stroke_color);
                egui::widgets::color_picker::color_picker_hsva_2d(
                  ui,
                  &mut stroke_hsva,
                  egui::widgets::color_picker::Alpha::Opaque,
                );
                let new_stroke_color = egui::Color32::from(stroke_hsva);
                if new_stroke_color != stroke_color {
                  self.update_shape_stroke_color(&layer_id, shape_idx, new_stroke_color);
                }

                ui.separator();
                ui.label("Fill Color:");
                if ui.color_edit_button_srgba(&mut fill_color).changed() {
                  self.update_shape_fill_color(&layer_id, shape_idx, fill_color);
                }

                let mut fill_hsva = egui::ecolor::Hsva::from(fill_color);
                egui::widgets::color_picker::color_picker_hsva_2d(
                  ui,
                  &mut fill_hsva,
                  egui::widgets::color_picker::Alpha::BlendOrAdditive,
                );
                let new_fill_color = egui::Color32::from(fill_hsva);
                if new_fill_color != fill_color {
                  self.update_shape_fill_color(&layer_id, shape_idx, new_fill_color);
                }
              } else {
                if ui.color_edit_button_srgba(&mut stroke_color).changed() {
                  self.update_shape_color(&layer_id, shape_idx, stroke_color);
                }

                ui.separator();
                let mut hsva = egui::ecolor::Hsva::from(stroke_color);
                egui::widgets::color_picker::color_picker_hsva_2d(
                  ui,
                  &mut hsva,
                  egui::widgets::color_picker::Alpha::Opaque,
                );
                let new_color = egui::Color32::from(hsva);
                if new_color != stroke_color {
                  self.update_shape_color(&layer_id, shape_idx, new_color);
                }
              }
            }
          }
        });

      if !is_open {
        ui.memory_mut(|mem| mem.data.remove::<bool>(popup_id));
      }
    }
  }

  fn show_shape_ui(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    shape: &Geometry<PixelCoordinate>,
  ) {
    let geometry_key = (layer_id.to_string(), shape_idx);
    let geometry_visible = *self.geometry_visibility.get(&geometry_key).unwrap_or(&true);
    let geometry_key_for_highlight = (layer_id.to_string(), shape_idx, Vec::new());
    let is_highlighted = self.geometry_highlighter.is_highlighted(
      &geometry_key_for_highlight.0,
      geometry_key_for_highlight.1,
      &geometry_key_for_highlight.2,
    );

    let bg_color = if is_highlighted {
      Some(egui::Color32::from_rgb(100, 100, 200))
    } else {
      None
    };

    let frame = if let Some(color) = bg_color {
      egui::Frame::default()
        .fill(color)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(4))
    } else {
      egui::Frame::default()
    };

    frame.show(ui, |ui| {
      // Handle collections differently - they get their own CollapsingHeader without eye icon
      if let Geometry::GeometryCollection(geometries, metadata) = shape {
        self.show_geometry_collection_inline(ui, layer_id, shape_idx, geometries, metadata);
      } else {
        // Non-collections get the traditional eye icon + content layout
        ui.horizontal(|ui| {
          let visibility_icon = if geometry_visible { "👁" } else { "🚫" };
          let eye_response = ui.add_sized([24.0, 20.0], egui::Button::new(visibility_icon));
          if eye_response.double_clicked() {
            if let Some(shapes) = self.shape_map.get(layer_id) {
              // Check if this element is already solo (only visible one)
              let is_solo = geometry_visible
                && (0..shapes.len()).all(|i| {
                  i == shape_idx
                    || !*self
                      .geometry_visibility
                      .get(&(layer_id.to_string(), i))
                      .unwrap_or(&true)
                });
              for i in 0..shapes.len() {
                self.geometry_visibility.insert(
                  (layer_id.to_string(), i),
                  if is_solo { true } else { i == shape_idx },
                );
              }
              self.invalidate_cache();
            }
          } else if eye_response.clicked() {
            self
              .geometry_visibility
              .insert(geometry_key.clone(), !geometry_visible);
            self.invalidate_cache();
          }

          let content_response = ui
            .horizontal(|ui| {
              self.show_shape_content(ui, layer_id, shape_idx, shape);
            })
            .response;

          // Handle double-click to show popup (TODO: implement popup)
          if content_response.double_clicked() {
            println!("TODO: Show detail popup for sidebar geometry");
          }

          content_response.context_menu(|ui| {
            self.show_visibility_button(ui, geometry_visible, "Geometry", |this| {
              this
                .geometry_visibility
                .insert(geometry_key.clone(), !geometry_visible);
            });

            ui.separator();

            self.show_delete_geometry_button(ui, layer_id, shape_idx, &geometry_key);
          });
        });
      }
    });
  }

  #[allow(clippy::too_many_lines)]
  fn show_shape_content(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    shape: &Geometry<PixelCoordinate>,
  ) {
    match shape {
      Geometry::Point(coord, metadata) => {
        let wgs84 = coord.as_wgs84();
        self.show_colored_icon(ui, layer_id, shape_idx, "📍", metadata, false);

        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 40.0).max(100.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, &label.short(), available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("point_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.full()));
          }
          let coord_text = format!("({:.3}, {:.3})", wgs84.lat, wgs84.lon);
          let available_width = (ui.available_width() - 20.0).max(30.0);
          let (truncated_coord, _) = truncate_label_by_width(ui, &coord_text, available_width);
          ui.small(truncated_coord);
        } else {
          let coord_text = format!("{:.3}, {:.3}", wgs84.lat, wgs84.lon);
          let available_width = (ui.available_width() - 20.0).max(30.0);
          let (truncated_coord, _) = truncate_label_by_width(ui, &coord_text, available_width);
          ui.label(truncated_coord);
        }
      }

      Geometry::LineString(coords, metadata) => {
        self.show_colored_icon(ui, layer_id, shape_idx, "📏", metadata, false);

        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 40.0).max(100.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, &label.short(), available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("line_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.full()));
          }
        } else {
          let response = ui.strong("Line");
          if response.clicked() {
            let popup_id = egui::Id::new(format!("line_popup_{layer_id}_{shape_idx}"));
            let line_info = format!(
              "📏 LineString\nPoints: {}\nStart: {:.4}, {:.4}\nEnd: {:.4}, {:.4}",
              coords.len(),
              coords.first().map_or(0.0, |c| c.as_wgs84().lat),
              coords.first().map_or(0.0, |c| c.as_wgs84().lon),
              coords.last().map_or(0.0, |c| c.as_wgs84().lat),
              coords.last().map_or(0.0, |c| c.as_wgs84().lon)
            );
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, line_info));
          }
        }

        ui.small(format!("({} pts)", coords.len()));

        if let (Some(first), Some(last)) = (coords.first(), coords.last()) {
          let first_wgs84 = first.as_wgs84();
          let last_wgs84 = last.as_wgs84();
          let coord_text = format!(
            "{:.2},{:.2}→{:.2},{:.2}",
            first_wgs84.lat, first_wgs84.lon, last_wgs84.lat, last_wgs84.lon
          );
          let available_width = (ui.available_width() - 20.0).max(30.0);
          let (truncated_coord, _) = truncate_label_by_width(ui, &coord_text, available_width);
          let response = ui.small(truncated_coord);
          if response.clicked() {
            let popup_id = egui::Id::new(format!("line_coords_popup_{layer_id}_{shape_idx}"));
            let all_coords = coords
              .iter()
              .enumerate()
              .map(|(i, coord)| {
                let wgs84 = coord.as_wgs84();
                format!("{:2}: {:.6}, {:.6}", i + 1, wgs84.lat, wgs84.lon)
              })
              .collect::<Vec<_>>()
              .join("\n");
            let coords_info = format!(
              "📏 LineString Coordinates\nTotal Points: {}\n\nAll Coordinates:\n{}",
              coords.len(),
              all_coords
            );
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, coords_info));
          }
        }
      }

      Geometry::Polygon(coords, metadata) => {
        self.show_colored_icon(ui, layer_id, shape_idx, "⬟", metadata, true);

        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 40.0).max(100.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, &label.short(), available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("polygon_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.full()));
          }
        } else {
          ui.label("Polygon");
        }

        ui.small(format!("({} pts)", coords.len()));

        if !coords.is_empty() {
          let wgs84_coords: Vec<WGS84Coordinate> =
            coords.iter().map(Coordinate::as_wgs84).collect();
          let min_lat = wgs84_coords
            .iter()
            .map(|c| c.lat)
            .min_by(f32::total_cmp)
            .unwrap_or(0.0);
          let max_lat = wgs84_coords
            .iter()
            .map(|c| c.lat)
            .max_by(f32::total_cmp)
            .unwrap_or(0.0);
          let min_lon = wgs84_coords
            .iter()
            .map(|c| c.lon)
            .min_by(f32::total_cmp)
            .unwrap_or(0.0);
          let max_lon = wgs84_coords
            .iter()
            .map(|c| c.lon)
            .max_by(f32::total_cmp)
            .unwrap_or(0.0);

          let bounds_text = format!("{min_lat:.1},{min_lon:.1}→{max_lat:.1},{max_lon:.1}");
          let available_width = (ui.available_width() - 20.0).max(30.0);
          let (truncated_bounds, _) = truncate_label_by_width(ui, &bounds_text, available_width);
          ui.small(truncated_bounds);
        }
      }

      Geometry::GeometryCollection(geometries, metadata) => {
        // Collections should use CollapsingHeader, not the eye icon UI
        // This handles the case where a top-level geometry is a collection
        self.show_geometry_collection_inline(ui, layer_id, shape_idx, geometries, metadata);
      }

      Geometry::Heatmap(coords, metadata) => {
        self.show_colored_icon(ui, layer_id, shape_idx, "🔥", metadata, false);

        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 40.0).max(100.0);
          let (truncated_label, _was_truncated) =
            truncate_label_by_width(ui, &label.short(), available_width);
          ui.strong(truncated_label);
        } else {
          ui.label("Heatmap");
        }

        let pts_text = format!("{} pts", coords.len());
        let available_width = (ui.available_width() - 20.0).max(30.0);
        let (truncated, _) = truncate_label_by_width(ui, &pts_text, available_width);
        ui.small(truncated);
      }
    }

    let geometry_popup_ids = [
      format!("point_popup_{layer_id}_{shape_idx}"),
      format!("line_popup_{layer_id}_{shape_idx}"),
      format!("line_coords_popup_{layer_id}_{shape_idx}"),
      format!("polygon_popup_{layer_id}_{shape_idx}"),
      format!("collection_popup_{layer_id}_{shape_idx}"),
      format!("collection_label_popup_{layer_id}_{shape_idx}"),
    ];

    for popup_id_str in geometry_popup_ids {
      let popup_id = egui::Id::new(&popup_id_str);
      if let Some(full_text) = ui.memory(|mem| mem.data.get_temp::<String>(popup_id)) {
        let mut is_open = true;
        egui::Window::new("Full Label")
          .id(popup_id)
          .open(&mut is_open)
          .collapsible(false)
          .resizable(true)
          .movable(true)
          .default_width(500.0)
          .min_width(400.0)
          .max_width(800.0)
          .max_height(400.0)
          .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical()
              .max_height(300.0)
              .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                  ui.add(egui::Label::new(&full_text).wrap());
                });
              });
          });

        if !is_open {
          ui.memory_mut(|mem| mem.data.remove::<String>(popup_id));
        }
      }
    }
  }

  fn show_geometry_collection_inline(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    geometries: &[Geometry<PixelCoordinate>],
    metadata: &Metadata,
  ) {
    let collection_key = (layer_id.to_string(), shape_idx, vec![]);
    let is_expanded = self
      .collection_expansion
      .get(&collection_key)
      .unwrap_or(&false);

    let collection_label = if let Some(label) = &metadata.label {
      format!("📁 {} ({} items)", label.short(), geometries.len())
    } else {
      format!("📁 Collection ({} items)", geometries.len())
    };

    let header_id = egui::Id::new(format!("collection_{layer_id}_{shape_idx}"));
    let header_response = egui::CollapsingHeader::new(collection_label)
      .id_salt(header_id)
      .default_open(*is_expanded)
      .show(ui, |ui| {
        for (nested_idx, nested_geometry) in geometries.iter().enumerate() {
          let nested_path = vec![nested_idx];
          self.show_nested_geometry_content(
            ui,
            layer_id,
            shape_idx,
            &nested_path,
            nested_geometry,
            geometries.len(),
          );
          if nested_idx < geometries.len() - 1 {
            ui.separator();
          }
        }
      });

    // Update expansion state based on the body response (if body was shown, header was open)
    let is_currently_open = header_response.body_response.is_some();
    self
      .collection_expansion
      .insert(collection_key, is_currently_open);

    // Handle double-click to show popup (TODO: implement popup)
    if header_response.header_response.double_clicked() {
      println!("TODO: Show detail popup for collection");
    }

    // Add context menu for collection
    header_response.header_response.context_menu(|ui| {
      let geometry_key = (layer_id.to_string(), shape_idx);
      let geometry_visible = *self.geometry_visibility.get(&geometry_key).unwrap_or(&true);

      self.show_visibility_button(ui, geometry_visible, "Collection", |this| {
        this
          .geometry_visibility
          .insert(geometry_key.clone(), !geometry_visible);
      });

      ui.separator();
      ui.separator();

      if let Some(label) = &metadata.label {
        let popup_id = format!("collection_label_popup_{layer_id}_{shape_idx}");
        Self::show_label_button(ui, label, &popup_id);
      } else {
        ui.label("(No label available)");
      }

      let popup_id = format!("collection_popup_{layer_id}_{shape_idx}");
      Self::show_collection_info_button(ui, geometries, &popup_id);

      ui.separator();

      self.show_delete_collection_button(ui, layer_id, shape_idx, &geometry_key);
    });
  }

  #[allow(clippy::too_many_lines)]
  fn show_nested_geometry_content(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
    geometry: &Geometry<PixelCoordinate>,
    sibling_count: usize,
  ) {
    let nested_key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
    let nested_visible = *self
      .nested_geometry_visibility
      .get(&nested_key)
      .unwrap_or(&true);

    if let Geometry::GeometryCollection(nested_geometries, nested_metadata) = geometry {
      let collection_key = nested_key.clone();
      let is_expanded = *self
        .collection_expansion
        .get(&collection_key)
        .unwrap_or(&false);

      let collection_label = if let Some(label) = &nested_metadata.label {
        format!("📁 {} ({} items)", label.short(), nested_geometries.len())
      } else {
        format!("📁 Collection ({} items)", nested_geometries.len())
      };

      let header_id = egui::Id::new(format!(
        "nested_collection_{layer_id}_{shape_idx}_{nested_path:?}"
      ));
      let header_response = egui::CollapsingHeader::new(collection_label)
        .id_salt(header_id)
        .default_open(is_expanded)
        .show(ui, |ui| {
          let total_items = nested_geometries.len();
          let sibling_count = nested_geometries.len();

          let scroll_id = egui::Id::new(format!(
            "nested_scroll_{layer_id}_{shape_idx}_{nested_path:?}"
          ));
          egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .max_height(SCROLL_AREA_MAX_HEIGHT)
            .show(ui, |ui| {
              for (sub_idx, sub_geometry) in nested_geometries.iter().enumerate() {
                let mut sub_path = nested_path.to_vec();
                sub_path.push(sub_idx);
                self.show_nested_geometry_content(
                  ui,
                  layer_id,
                  shape_idx,
                  &sub_path,
                  sub_geometry,
                  sibling_count,
                );
                if sub_idx < total_items - 1 {
                  ui.separator();
                }
              }
            });
        });

      // Update expansion state
      let is_currently_open = header_response.body_response.is_some();
      self
        .collection_expansion
        .insert(collection_key, is_currently_open);

      // Add context menu for nested collection
      header_response.header_response.context_menu(|ui| {
        self.show_visibility_button(ui, nested_visible, "Collection", |this| {
          this
            .nested_geometry_visibility
            .insert(nested_key, !nested_visible);
        });

        ui.separator();

        // Show full label option for nested collections
        if let Some(label) = &nested_metadata.label {
          let popup_id_str = format!(
            "nested_label_{layer_id}_{shape_idx}_{}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join("_")
          );
          Self::show_label_button(ui, label, &popup_id_str);
        } else {
          ui.label("(No label available)");
        }
      });
    } else {
      // Individual geometries get eye icon + content with indentation
      let mut toggle_visibility = false;

      let horizontal_response = ui.horizontal(|ui| {
        // Add minimal indentation based on nesting level (only for individual geometries)
        let indent_level = nested_path.len();
        #[allow(clippy::cast_precision_loss)]
        ui.add_space(4.0 * (indent_level as f32));

        // Visibility toggle button for individual geometries
        let visibility_icon = if nested_visible { "👁" } else { "🚫" };
        let eye_response = ui.add_sized([24.0, 20.0], egui::Button::new(visibility_icon));
        if eye_response.double_clicked() {
          let parent_path = &nested_path[..nested_path.len() - 1];
          let current_idx = nested_path[nested_path.len() - 1];
          // Check if this element is already solo (only visible one among siblings)
          let is_solo = nested_visible
            && (0..sibling_count).all(|i| {
              i == current_idx
                || !*self
                  .nested_geometry_visibility
                  .get(&{
                    let mut p = parent_path.to_vec();
                    p.push(i);
                    (layer_id.to_string(), shape_idx, p)
                  })
                  .unwrap_or(&true)
            });
          for i in 0..sibling_count {
            let mut sibling_path = parent_path.to_vec();
            sibling_path.push(i);
            self.nested_geometry_visibility.insert(
              (layer_id.to_string(), shape_idx, sibling_path),
              if is_solo { true } else { i == current_idx },
            );
          }
          self.invalidate_cache();
        } else if eye_response.clicked() {
          toggle_visibility = true;
        }

        // Show individual geometry content
        match geometry {
          Geometry::Point(coord, nested_metadata) => {
            let wgs84 = coord.as_wgs84();
            ui.label("📍");
            if let Some(label) = &nested_metadata.label {
              let available_width = (ui.available_width() - 40.0).max(100.0);
              let (truncated_label, _was_truncated) =
                truncate_label_by_width(ui, &label.short(), available_width);
              ui.strong(truncated_label);
            } else {
              ui.label("Point");
            }
            ui.small(format!("({:.3}, {:.3})", wgs84.lat, wgs84.lon));
          }
          Geometry::LineString(coords, nested_metadata) => {
            ui.label("📏");
            if let Some(label) = &nested_metadata.label {
              let available_width = (ui.available_width() - 40.0).max(100.0);
              let (truncated_label, _was_truncated) =
                truncate_label_by_width(ui, &label.short(), available_width);
              ui.strong(truncated_label);
            } else {
              ui.label("Line");
            }
            ui.small(format!("({} pts)", coords.len()));
          }
          Geometry::Polygon(coords, nested_metadata) => {
            ui.label("⬟");
            if let Some(label) = &nested_metadata.label {
              let available_width = (ui.available_width() - 40.0).max(100.0);
              let (truncated_label, _was_truncated) =
                truncate_label_by_width(ui, &label.short(), available_width);
              ui.strong(truncated_label);
            } else {
              ui.label("Polygon");
            }
            ui.small(format!("({} pts)", coords.len()));
          }
          Geometry::GeometryCollection(..) => {
            // This should not happen in individual geometry context
          }
          Geometry::Heatmap(coords, nested_metadata) => {
            ui.label("🔥");
            if let Some(label) = &nested_metadata.label {
              let available_width = (ui.available_width() - 40.0).max(100.0);
              let (truncated_label, _was_truncated) =
                truncate_label_by_width(ui, &label.short(), available_width);
              ui.strong(truncated_label);
            } else {
              ui.label("Heatmap");
            }
            ui.small(format!("({} pts)", coords.len()));
          }
        }
      });

      // Check if this individual nested geometry is highlighted for sidebar background
      let geometry_key_for_highlight = (layer_id.to_string(), shape_idx, nested_path.to_vec());
      let is_highlighted = self.geometry_highlighter.is_highlighted(
        &geometry_key_for_highlight.0,
        geometry_key_for_highlight.1,
        &geometry_key_for_highlight.2,
      );

      // Add background color to the horizontal response if highlighted
      if is_highlighted {
        let rect = horizontal_response.response.rect;
        ui.painter()
          .rect_filled(rect, 2.0, egui::Color32::from_rgb(100, 100, 200));

        // Scroll to this element if it was just double-clicked on the map
        if self
          .just_double_clicked
          .as_ref()
          .is_some_and(|(l, idx, path)| l == layer_id && *idx == shape_idx && path == nested_path)
        {
          horizontal_response
            .response
            .scroll_to_me(Some(egui::Align::Center));
        }
      }

      // Handle visibility toggle after the horizontal closure
      if toggle_visibility {
        self
          .nested_geometry_visibility
          .insert(nested_key.clone(), !nested_visible);
        self.invalidate_cache();
      }

      // Handle double-click to show popup (TODO: implement popup)
      if horizontal_response.response.double_clicked() {
        println!("TODO: Show detail popup for individual nested geometry");
      }

      // Add context menu to individual geometries
      horizontal_response.response.context_menu(|ui| {
        self.show_visibility_button(ui, nested_visible, "Geometry", |this| {
          this
            .nested_geometry_visibility
            .insert(nested_key, !nested_visible);
          this.invalidate_cache();
        });
      });
    }
  }

  fn show_colored_icon(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    icon: &str,
    metadata: &Metadata,
    is_polygon: bool,
  ) {
    let stroke_color = if let Some(style) = &metadata.style {
      style.color()
    } else {
      egui::Color32::BLUE
    };

    let colored_text = egui::RichText::new(icon).color(stroke_color);

    let hover_text = if is_polygon {
      "Click to change stroke & fill colors"
    } else {
      "Click to change color"
    };
    let icon_response = ui.button(colored_text).on_hover_text(hover_text);

    let popup_id = egui::Id::new(format!("color_picker_{layer_id}_{shape_idx}"));

    if icon_response.clicked() {
      if metadata.style.is_none()
        && let Some(shapes) = self.shape_map.get_mut(layer_id)
        && let Some(shape) = shapes.get_mut(shape_idx)
      {
        let shape_metadata = match shape {
          Geometry::Point(_, metadata)
          | Geometry::LineString(_, metadata)
          | Geometry::Polygon(_, metadata)
          | Geometry::GeometryCollection(_, metadata)
          | Geometry::Heatmap(_, metadata) => metadata,
        };
        shape_metadata.style = Some(crate::map::geometry_collection::Style::default());
      }
      ui.memory_mut(|mem| mem.data.insert_temp(popup_id, true));
    }
  }

  fn update_shape_color(&mut self, layer_id: &str, shape_idx: usize, new_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id)
      && let Some(shape) = shapes.get_mut(shape_idx)
    {
      let metadata = match shape {
        Geometry::Point(_, metadata)
        | Geometry::LineString(_, metadata)
        | Geometry::Polygon(_, metadata)
        | Geometry::GeometryCollection(_, metadata)
        | Geometry::Heatmap(_, metadata) => metadata,
      };

      let new_style = if let Some(existing_style) = &metadata.style {
        Style::default()
          .with_color(new_color)
          .with_fill_color(existing_style.fill_color())
          .with_visible(true)
      } else {
        Style::default().with_color(new_color)
      };
      metadata.style = Some(new_style);
    }
  }

  fn update_shape_stroke_color(&mut self, layer_id: &str, shape_idx: usize, new_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id)
      && let Some(shape) = shapes.get_mut(shape_idx)
    {
      let metadata = match shape {
        Geometry::Point(_, metadata)
        | Geometry::LineString(_, metadata)
        | Geometry::Polygon(_, metadata)
        | Geometry::GeometryCollection(_, metadata)
        | Geometry::Heatmap(_, metadata) => metadata,
      };

      let new_style = if let Some(existing_style) = &metadata.style {
        Style::default()
          .with_color(new_color)
          .with_fill_color(existing_style.fill_color())
          .with_visible(true)
      } else {
        Style::default().with_color(new_color)
      };
      metadata.style = Some(new_style);
    }
  }

  fn update_shape_fill_color(&mut self, layer_id: &str, shape_idx: usize, new_fill_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id)
      && let Some(shape) = shapes.get_mut(shape_idx)
    {
      let metadata = match shape {
        Geometry::Point(_, metadata)
        | Geometry::LineString(_, metadata)
        | Geometry::Polygon(_, metadata)
        | Geometry::GeometryCollection(_, metadata)
        | Geometry::Heatmap(_, metadata) => metadata,
      };

      let new_style = if let Some(existing_style) = &metadata.style {
        Style::default()
          .with_color(existing_style.color())
          .with_fill_color(new_fill_color)
          .with_visible(true)
      } else {
        Style::default()
          .with_color(Color32::BLUE)
          .with_fill_color(new_fill_color)
      };
      metadata.style = Some(new_style);
    }
  }

  /// Check for nested collection popups at any depth
  fn check_nested_popups_recursive(
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    geometries: &[Geometry<PixelCoordinate>],
    current_path: &mut Vec<usize>,
  ) {
    for (nested_idx, nested_geometry) in geometries.iter().enumerate() {
      current_path.push(nested_idx);

      if let Geometry::GeometryCollection(sub_geometries, metadata) = nested_geometry {
        // Check if this collection has a label and could be a popup target
        if metadata.label.is_some() {
          let popup_id_str = format!(
            "nested_label_{layer_id}_{shape_idx}_{}",
            current_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join("_")
          );
          let popup_id = egui::Id::new(&popup_id_str);

          if let Some(full_text) = ui.memory(|mem| mem.data.get_temp::<String>(popup_id)) {
            let mut is_open = true;
            egui::Window::new("Full Label")
              .id(popup_id)
              .open(&mut is_open)
              .collapsible(false)
              .resizable(true)
              .movable(true)
              .default_width(500.0)
              .min_width(400.0)
              .max_width(800.0)
              .max_height(400.0)
              .show(ui.ctx(), |ui| {
                egui::ScrollArea::vertical()
                  .max_height(300.0)
                  .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                      ui.add(egui::Label::new(&full_text).wrap());
                    });
                  });
              });

            if !is_open {
              ui.memory_mut(|mem| mem.data.remove::<String>(popup_id));
            }
          }
        }

        // Recursively check deeper nesting levels
        Self::check_nested_popups_recursive(ui, layer_id, shape_idx, sub_geometries, current_path);
      }

      current_path.pop();
    }
  }
  // Context menu helpers
  fn show_visibility_button(
    &mut self,
    ui: &mut egui::Ui,
    is_visible: bool,
    item_type: &str,
    toggle_action: impl FnOnce(&mut Self),
  ) {
    let visibility_text = if is_visible { "Hide" } else { "Show" };
    if ui
      .button(format!("{visibility_text} {item_type}"))
      .clicked()
    {
      toggle_action(self);
      ui.close();
    }
  }

  fn show_label_button(
    ui: &mut egui::Ui,
    label: &crate::map::geometry_collection::Label,
    popup_id: &str,
  ) {
    if ui.button("📄 Show Full Label").clicked() {
      let id = egui::Id::new(popup_id);
      ui.memory_mut(|mem| mem.data.insert_temp(id, label.full()));
      ui.close();
    }
  }

  fn show_delete_geometry_button(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    geometry_key: &(String, usize),
  ) {
    if ui.button("🗑 Delete Geometry").clicked() {
      if let Some(shapes) = self.shape_map.get_mut(layer_id)
        && shape_idx < shapes.len()
      {
        shapes.remove(shape_idx);
        self.geometry_visibility.remove(geometry_key);

        // Update indices for remaining geometries
        let keys_to_update: Vec<_> = self
          .geometry_visibility
          .keys()
          .filter(|(lid, idx)| lid == layer_id && *idx > shape_idx)
          .cloned()
          .collect();

        for (lid, idx) in keys_to_update {
          if let Some(visible) = self.geometry_visibility.remove(&(lid.clone(), idx)) {
            self.geometry_visibility.insert((lid, idx - 1), visible);
          }
        }
        self.invalidate_cache();
      }
      ui.close();
    }
  }

  fn show_collection_info_button(
    ui: &mut egui::Ui,
    geometries: &[Geometry<PixelCoordinate>],
    popup_id: &str,
  ) {
    if ui.button("📋 Collection Info").clicked() {
      let id = egui::Id::new(popup_id);
      let collection_info = format!(
        "📁 Geometry Collection\nItems: {}\nNested geometries: {}",
        geometries.len(),
        geometries
          .iter()
          .map(|g| match g {
            Geometry::Point(_, _) => "Point".to_string(),
            Geometry::LineString(_, _) => "LineString".to_string(),
            Geometry::Polygon(_, _) => "Polygon".to_string(),
            Geometry::GeometryCollection(nested, _) => format!("Collection ({})", nested.len()),
            Geometry::Heatmap(coords, _) => format!("Heatmap ({})", coords.len()),
          })
          .collect::<Vec<_>>()
          .join(", ")
      );
      ui.memory_mut(|mem| mem.data.insert_temp(id, collection_info));
      ui.close();
    }
  }

  /// Highlight a geometry by its path (converts to ID-based highlighting)
  fn highlight_geometry(&mut self, layer_id: &str, shape_idx: usize, nested_path: &[usize]) {
    self
      .geometry_highlighter
      .highlight_geometry(layer_id, shape_idx, nested_path);
  }

  /// Draw highlighting for a single specific geometry using the `geometry_highlighting` module
  fn draw_highlighted_geometry(
    geometry: &Geometry<PixelCoordinate>,
    painter: &egui::Painter,
    transform: &Transform,
    _highlight_all: bool, // Unused - we never highlight entire collections
  ) {
    use super::geometry_highlighting::draw_highlighted_geometry;
    draw_highlighted_geometry(geometry, painter, transform, false);
  }

  /// Render the hover-highlight for the currently selected geometry.
  /// Polygon fills are rasterized via tiny-skia and cached as a texture;
  /// strokes/points/lines are added as egui shapes.
  fn draw_highlight_overlay(&mut self, ui: &mut egui::Ui, transform: &Transform, rect: Rect) {
    let Some((layer_id, shape_idx, nested_path)) =
      self.geometry_highlighter.get_highlighted_geometry()
    else {
      self.highlight_texture = None;
      return;
    };
    if !nested_path.is_empty() {
      // The render loop only handles top-level highlights; preserve that.
      self.highlight_texture = None;
      return;
    }
    if !*self.layer_visibility.get(&layer_id).unwrap_or(&true) {
      self.highlight_texture = None;
      return;
    }
    if !*self
      .geometry_visibility
      .get(&(layer_id.clone(), shape_idx))
      .unwrap_or(&true)
    {
      self.highlight_texture = None;
      return;
    }
    let Some(shape) = self
      .shape_map
      .get(&layer_id)
      .and_then(|s| s.get(shape_idx))
      .cloned()
    else {
      self.highlight_texture = None;
      return;
    };

    let key = HighlightCacheKey {
      geometry_path: (layer_id, shape_idx, nested_path),
      viewport: [
        rect.min.x.to_bits(),
        rect.min.y.to_bits(),
        rect.max.x.to_bits(),
        rect.max.y.to_bits(),
      ],
      transform: [
        transform.zoom.to_bits(),
        transform.trans.x.to_bits(),
        transform.trans.y.to_bits(),
      ],
      version: self.version,
    };

    let needs_rebuild = self.highlight_texture.as_ref().is_none_or(|c| c.key != key);
    if needs_rebuild {
      use super::geometry_highlighting::rasterize_highlighted_polygons;
      if let Some((image, screen_rect)) = rasterize_highlighted_polygons(&shape, transform, rect) {
        let handle =
          ui.ctx()
            .load_texture("highlight_polygon", image, egui::TextureOptions::LINEAR);
        self.highlight_texture = Some(HighlightTextureCache {
          key,
          texture: handle,
          screen_rect,
        });
      } else {
        self.highlight_texture = None;
      }
    }

    let painter = ui.painter_at(rect);
    if let Some(cache) = &self.highlight_texture {
      painter.image(
        cache.texture.id(),
        cache.screen_rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
      );
    }
    Self::draw_highlighted_geometry(&shape, &painter, transform, false);
  }

  fn show_delete_collection_button(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    geometry_key: &(String, usize),
  ) {
    if ui.button("🗑 Delete Collection").clicked() {
      if let Some(shapes) = self.shape_map.get_mut(layer_id)
        && shape_idx < shapes.len()
      {
        shapes.remove(shape_idx);
        self.geometry_visibility.remove(geometry_key);

        // Clean up any nested visibility state for this collection
        self
          .nested_geometry_visibility
          .retain(|(lid, idx, _), _| !(lid == layer_id && *idx == shape_idx));
        self
          .collection_expansion
          .retain(|(lid, idx, _), _| !(lid == layer_id && *idx == shape_idx));

        // Update indices for remaining geometries
        let keys_to_update: Vec<_> = self
          .geometry_visibility
          .keys()
          .filter(|(lid, idx)| lid == layer_id && *idx > shape_idx)
          .cloned()
          .collect();

        for (lid, idx) in keys_to_update {
          if let Some(visible) = self.geometry_visibility.remove(&(lid.clone(), idx)) {
            self.geometry_visibility.insert((lid, idx - 1), visible);
          }
        }
        self.invalidate_cache();
      }
      ui.close();
    }
  }

  /// Search for geometries that match the given query string (supports regex)
  pub fn search_geometries(&mut self, query: &str) {
    self.search_results.clear();

    // Try to compile as regex first, fallback to literal string search
    let search_pattern = match regex::Regex::new(query) {
      Ok(regex) => SearchPattern::Regex(regex),
      Err(_) => {
        // If regex compilation fails, treat as literal string (case-insensitive)
        SearchPattern::Literal(query.to_lowercase())
      }
    };

    // Collect results first to avoid borrowing issues
    let mut results = Vec::new();

    for (layer_id, shapes) in &self.shape_map {
      for (shape_idx, shape) in shapes.iter().enumerate() {
        Self::search_in_geometry_static(
          layer_id,
          shape_idx,
          &Vec::new(),
          shape,
          &search_pattern,
          &mut results,
        );
      }
    }

    self.search_results = results.clone();

    // Highlight all found geometries
    if results.is_empty() {
      // Clear highlighting if no results found
      self.geometry_highlighter.clear_highlighting();
    } else {
      // Clear any previous highlighting
      self.geometry_highlighter.clear_highlighting();

      // Highlight the first search result
      if let Some((layer_id, shape_idx, nested_path)) = results.first() {
        self.highlight_geometry(layer_id, *shape_idx, nested_path);

        // Show popup for the first search result
        self.show_search_result_popup();
      }
    }
  }

  /// Get current search results
  #[must_use]
  pub fn get_search_results(&self) -> &Vec<(String, usize, Vec<usize>)> {
    &self.search_results
  }

  /// Show popup for currently highlighted search result
  pub fn show_search_result_popup(&mut self) {
    if let Some((layer_id, shape_idx, nested_path)) =
      self.geometry_highlighter.get_highlighted_geometry()
      && let Some(detail_info) =
        self.generate_geometry_detail_info(&layer_id, shape_idx, &nested_path)
    {
      // Find the geometry to get its representative coordinate for popup positioning
      if let Some(coord) =
        self.get_geometry_representative_coordinate(&layer_id, shape_idx, &nested_path)
      {
        // Convert to screen position using current transform
        let screen_pos = if self.current_transform.is_invalid() {
          egui::pos2(0.0, 0.0) // Fallback position
        } else {
          let pixel_pos = self.current_transform.apply(coord);
          egui::pos2(pixel_pos.x, pixel_pos.y)
        };

        let creation_time = std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap_or_default()
          .as_secs_f64();

        self.pending_detail_popup = Some((screen_pos, coord, detail_info, creation_time));
      }
    }
  }

  /// Navigate to next search result
  pub fn next_search_result(&mut self) -> bool {
    if self.search_results.is_empty() {
      return false;
    }

    let current_highlighted = self.geometry_highlighter.get_highlighted_geometry();
    let current_idx = if let Some(current) = current_highlighted {
      self
        .search_results
        .iter()
        .position(|result| result == &current)
    } else {
      None
    };

    let next_idx = match current_idx {
      Some(idx) => (idx + 1) % self.search_results.len(),
      None => 0,
    };

    if let Some((layer_id, shape_idx, nested_path)) = self.search_results.get(next_idx).cloned() {
      self.highlight_geometry(&layer_id, shape_idx, &nested_path);
      self.show_search_result_popup();
      true
    } else {
      false
    }
  }

  /// Navigate to previous search result
  pub fn previous_search_result(&mut self) -> bool {
    if self.search_results.is_empty() {
      return false;
    }

    let current_highlighted = self.geometry_highlighter.get_highlighted_geometry();
    let current_idx = if let Some(current) = current_highlighted {
      self
        .search_results
        .iter()
        .position(|result| result == &current)
    } else {
      None
    };

    let prev_idx = match current_idx {
      Some(idx) => {
        if idx == 0 {
          self.search_results.len() - 1
        } else {
          idx - 1
        }
      }
      None => self.search_results.len() - 1,
    };

    if let Some((layer_id, shape_idx, nested_path)) = self.search_results.get(prev_idx).cloned() {
      self.highlight_geometry(&layer_id, shape_idx, &nested_path);
      self.show_search_result_popup();
      true
    } else {
      false
    }
  }

  /// Get representative coordinate for a geometry (used for popup positioning)
  fn get_geometry_representative_coordinate(
    &self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
  ) -> Option<PixelCoordinate> {
    let shapes = self.shape_map.get(layer_id)?;
    let mut current_geometry = shapes.get(shape_idx)?;

    // Navigate to the specific nested geometry if there's a path
    for &idx in nested_path {
      if let Geometry::GeometryCollection(geometries, _) = current_geometry {
        current_geometry = geometries.get(idx)?;
      } else {
        return None; // Invalid path
      }
    }

    // Return representative coordinate based on geometry type
    Self::get_geometry_first_coordinate(current_geometry)
  }

  /// Get the first coordinate from any geometry type
  fn get_geometry_first_coordinate(
    geometry: &Geometry<PixelCoordinate>,
  ) -> Option<PixelCoordinate> {
    match geometry {
      Geometry::Point(coord, _) => Some(*coord),
      Geometry::LineString(coords, _) | Geometry::Polygon(coords, _) => coords.first().copied(),
      Geometry::Heatmap(coords, _) => coords.first().copied(),
      Geometry::GeometryCollection(geometries, _) => {
        // For collections, try to get coordinate from first child geometry
        geometries
          .first()
          .and_then(Self::get_geometry_first_coordinate)
      }
    }
  }

  /// Apply filter to hide non-matching geometries
  pub fn filter_geometries(&mut self, query: &str) {
    // Try to compile as regex first, fallback to literal string search
    let filter_pattern = match regex::Regex::new(query) {
      Ok(regex) => SearchPattern::Regex(regex),
      Err(_) => {
        // If regex compilation fails, treat as literal string (case-insensitive)
        SearchPattern::Literal(query.to_lowercase())
      }
    };

    self.filter_pattern = Some(filter_pattern);
    self.invalidate_cache();
  }

  /// Clear filter and show all geometries
  pub fn clear_filter(&mut self) {
    self.filter_pattern = None;
    self.invalidate_cache();
  }

  /// Check if a geometry matches the current filter
  fn geometry_matches_filter(&self, geometry: &Geometry<PixelCoordinate>) -> bool {
    if let Some(ref pattern) = self.filter_pattern {
      Self::geometry_matches_pattern_static(geometry, pattern)
    } else {
      true // No filter active, show all geometries
    }
  }

  /// Check if a geometry matches a search pattern (static version)
  fn geometry_matches_pattern_static(
    geometry: &Geometry<PixelCoordinate>,
    pattern: &SearchPattern,
  ) -> bool {
    let metadata = match geometry {
      Geometry::Point(_, metadata)
      | Geometry::LineString(_, metadata)
      | Geometry::Polygon(_, metadata)
      | Geometry::GeometryCollection(_, metadata)
      | Geometry::Heatmap(_, metadata) => metadata,
    };

    // Check if metadata contains the search pattern
    if let Some(label) = &metadata.label {
      if Self::matches_pattern(&label.name, pattern)
        || Self::matches_pattern(&label.short(), pattern)
        || Self::matches_pattern(&label.full(), pattern)
      {
        return true;
      }

      if let Some(description) = &label.description
        && Self::matches_pattern(description, pattern)
      {
        return true;
      }
    }

    // For collections, check nested geometries recursively
    if let Geometry::GeometryCollection(geometries, _) = geometry {
      for nested_geometry in geometries {
        if Self::geometry_matches_pattern_static(nested_geometry, pattern) {
          return true;
        }
      }
    }

    false
  }

  /// Check if text matches the search pattern
  fn matches_pattern(text: &str, pattern: &SearchPattern) -> bool {
    match pattern {
      SearchPattern::Regex(regex) => regex.is_match(text),
      SearchPattern::Literal(literal) => text.to_lowercase().contains(literal),
    }
  }

  /// Recursively search within a geometry for the query string (static version to avoid borrow checker issues)
  fn search_in_geometry_static(
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
    geometry: &Geometry<PixelCoordinate>,
    pattern: &SearchPattern,
    results: &mut Vec<(String, usize, Vec<usize>)>,
  ) {
    let metadata = match geometry {
      Geometry::Point(_, metadata)
      | Geometry::LineString(_, metadata)
      | Geometry::Polygon(_, metadata)
      | Geometry::GeometryCollection(_, metadata)
      | Geometry::Heatmap(_, metadata) => metadata,
    };

    // Check if metadata contains the search pattern
    let mut matches = false;

    if let Some(label) = &metadata.label {
      if Self::matches_pattern(&label.name, pattern)
        || Self::matches_pattern(&label.short(), pattern)
        || Self::matches_pattern(&label.full(), pattern)
      {
        matches = true;
      }

      if let Some(description) = &label.description
        && Self::matches_pattern(description, pattern)
      {
        matches = true;
      }
    }

    if matches {
      results.push((layer_id.to_string(), shape_idx, nested_path.to_vec()));
    }

    // Recursively search in nested geometries
    if let Geometry::GeometryCollection(nested_geometries, _) = geometry {
      for (nested_idx, nested_geometry) in nested_geometries.iter().enumerate() {
        let mut nested_path = nested_path.to_vec();
        nested_path.push(nested_idx);
        Self::search_in_geometry_static(
          layer_id,
          shape_idx,
          &nested_path,
          nested_geometry,
          pattern,
          results,
        );
      }
    }
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

impl ShapeLayer {
  /// Navigate to a specific geometry within nested collections
  fn get_geometry_at_path<'a>(
    geometry: &'a Geometry<PixelCoordinate>,
    nested_path: &[usize],
  ) -> Option<&'a Geometry<PixelCoordinate>> {
    let mut current_geometry = geometry;

    for &path_index in nested_path {
      match current_geometry {
        Geometry::GeometryCollection(nested_geometries, _)
          if path_index < nested_geometries.len() =>
        {
          current_geometry = &nested_geometries[path_index];
        }
        _ => return None,
      }
    }

    Some(current_geometry)
  }

  /// Get temporal range from all geometries in this layer
  #[must_use]
  pub fn get_temporal_range(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let mut earliest: Option<DateTime<Utc>> = None;
    let mut latest: Option<DateTime<Utc>> = None;

    for shapes in self.shape_map.values() {
      for shape in shapes {
        Self::extract_temporal_from_geometry(shape, &mut earliest, &mut latest);
      }
    }

    (earliest, latest)
  }

  /// Recursively extract temporal data from a geometry and its children
  fn extract_temporal_from_geometry(
    geometry: &Geometry<PixelCoordinate>,
    earliest: &mut Option<DateTime<Utc>>,
    latest: &mut Option<DateTime<Utc>>,
  ) {
    let metadata = match geometry {
      Geometry::Point(_, meta)
      | Geometry::LineString(_, meta)
      | Geometry::Polygon(_, meta)
      | Geometry::Heatmap(_, meta) => meta,
      Geometry::GeometryCollection(children, meta) => {
        // Recursively process child geometries first
        for child in children {
          Self::extract_temporal_from_geometry(child, earliest, latest);
        }
        meta
      }
    };

    // Extract temporal data from this geometry's metadata
    if let Some(time_data) = &metadata.time_data {
      if let Some(timestamp) = time_data.timestamp {
        *earliest = Some(earliest.map_or(timestamp, |e| e.min(timestamp)));
        *latest = Some(latest.map_or(timestamp, |l| l.max(timestamp)));
      }

      if let Some(time_span) = &time_data.time_span {
        if let Some(begin) = time_span.begin {
          *earliest = Some(earliest.map_or(begin, |e| e.min(begin)));
        }
        if let Some(end) = time_span.end {
          *latest = Some(latest.map_or(end, |l| l.max(end)));
        }
      }
    }
  }

  /// Check if a top-level geometry should be visible at the given time
  fn is_geometry_visible_at_time(
    &self,
    geometry: &Geometry<PixelCoordinate>,
    current_time: DateTime<Utc>,
  ) -> bool {
    match geometry {
      Geometry::Point(_, meta)
      | Geometry::LineString(_, meta)
      | Geometry::Polygon(_, meta)
      | Geometry::Heatmap(_, meta) => {
        // For individual geometries, check their metadata
        if let Some(time_window) = self.temporal_time_window {
          meta.is_visible_in_time_window(current_time, time_window)
        } else {
          meta.is_visible_at_time(current_time)
        }
      }
      Geometry::GeometryCollection(children, meta) => {
        // For GeometryCollections, first check if the collection itself has temporal data
        if meta.time_data.is_some() {
          if let Some(time_window) = self.temporal_time_window {
            meta.is_visible_in_time_window(current_time, time_window)
          } else {
            meta.is_visible_at_time(current_time)
          }
        } else {
          // If collection has no temporal data, check if ANY child is visible
          // We still show the collection if at least one child is visible
          children
            .iter()
            .any(|child| self.is_geometry_visible_at_time(child, current_time))
        }
      }
    }
  }

  /// Check if an individual geometry (not a collection) should be visible at the given time
  fn is_individual_geometry_visible_at_time(
    &self,
    geometry: &Geometry<PixelCoordinate>,
    current_time: DateTime<Utc>,
  ) -> bool {
    let meta = match geometry {
      Geometry::Point(_, meta)
      | Geometry::LineString(_, meta)
      | Geometry::Polygon(_, meta)
      | Geometry::GeometryCollection(_, meta)
      | Geometry::Heatmap(_, meta) => meta, // Collections shouldn't reach here, but handle gracefully
    };

    if let Some(time_window) = self.temporal_time_window {
      meta.is_visible_in_time_window(current_time, time_window)
    } else {
      meta.is_visible_at_time(current_time)
    }
  }

  /// Generate detailed information about a geometry for popup display
  #[allow(clippy::too_many_lines)]
  fn generate_geometry_detail_info(
    &self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
  ) -> Option<String> {
    let shapes = self.shape_map.get(layer_id)?;
    let current_shape = shapes.get(shape_idx)?;
    let mut current_geometry = current_shape;

    // Navigate to the specific nested geometry if there's a path
    for &idx in nested_path {
      if let Geometry::GeometryCollection(geometries, _) = current_geometry {
        current_geometry = geometries.get(idx)?;
      } else {
        return None; // Invalid path
      }
    }

    // Generate basic information for geometry type
    let detail_info = match current_geometry {
      Geometry::Point(coord, metadata) => {
        let wgs84 = coord.as_wgs84();
        let mut info = format!("📍 Point\nCoordinates: {:.6}, {:.6}", wgs84.lat, wgs84.lon);

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        if let Some(time_data) = &metadata.time_data
          && let Some(timestamp) = time_data.timestamp
        {
          write!(
            info,
            "\nTimestamp: {}",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
          )
          .unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }

      Geometry::LineString(coords, metadata) => {
        let mut info = format!("📏 LineString\nPoints: {}", coords.len());

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        if let Some(time_data) = &metadata.time_data
          && let Some(timestamp) = time_data.timestamp
        {
          write!(
            info,
            "\nTimestamp: {}",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
          )
          .unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }

      Geometry::Polygon(coords, metadata) => {
        let mut info = format!("⬟ Polygon\nVertices: {}", coords.len());

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        if let Some(time_data) = &metadata.time_data
          && let Some(timestamp) = time_data.timestamp
        {
          write!(
            info,
            "\nTimestamp: {}",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
          )
          .unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }

      Geometry::Heatmap(coords, metadata) => {
        let mut info = format!("🔥 Heatmap\nPoints: {}", coords.len());

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }

      Geometry::GeometryCollection(geometries, metadata) => {
        let mut info = format!("📁 Collection\nItems: {}", geometries.len());

        if let Some(label) = &metadata.label {
          write!(info, "\nLabel: {}", label.full()).unwrap();
        }

        if let Some(time_data) = &metadata.time_data
          && let Some(timestamp) = time_data.timestamp
        {
          write!(
            info,
            "\nTimestamp: {}",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
          )
          .unwrap();
        }

        write!(info, "\nLayer: {layer_id}").unwrap();
        if !nested_path.is_empty() {
          write!(
            info,
            "\nNested Path: {}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join(" → ")
          )
          .unwrap();
        }

        info
      }
    };

    Some(detail_info)
  }

  /// Recursively find the closest individual geometry to a point
  #[allow(clippy::too_many_arguments)]
  fn find_closest_in_geometry(
    &self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
    geometry: &Geometry<PixelCoordinate>,
    click_pos: Pos2,
    transform: &Transform,
    closest_distance: &mut f64,
    closest_geometry: &mut Option<(String, usize, Vec<usize>)>,
  ) {
    geometry_selection::find_closest_in_geometry(
      layer_id,
      shape_idx,
      nested_path,
      geometry,
      click_pos,
      transform,
      closest_distance,
      closest_geometry,
      |layer_id, shape_idx, nested_path| {
        // Nested visibility check
        let nested_key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
        *self
          .nested_geometry_visibility
          .get(&nested_key)
          .unwrap_or(&true)
      },
      |nested_geometry| {
        // Temporal visibility check
        if let Some(current_time) = self.temporal_current_time {
          self.is_individual_geometry_visible_at_time(nested_geometry, current_time)
        } else {
          true
        }
      },
    );
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
