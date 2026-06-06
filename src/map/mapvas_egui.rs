use std::{
  cell::RefCell,
  path::PathBuf,
  rc::Rc,
  sync::{
    Arc, RwLock,
    mpsc::{Receiver, Sender},
  },
};

use crate::{
  config::Config,
  map::coordinates::{
    Coordinate, PixelCoordinate, Transform, WGS84Coordinate, transform_zoom_for_tile_zoom,
  },
  map::viewport::{fit_to_screen, set_coordinate_to_pixel, zoom_with_center},
  parser::{GrepParser, JsonParser, Parser},
  profile_scope,
  remote::{MapState, Remote, RepaintSignal},
};
use arboard::Clipboard;
use egui::{InputState, PointerButton, Rect, Response, Sense, Ui};
use helpers::{pixel_to_pos, point_to_coordinate, pos_to_pixel, show_box};
use layer::{EguiLayer, EguiMapFrame, TimelineLayer};
use layer_store::{MapLayerHolderImpl, MapLayerStore};
use log::{debug, warn};

use super::{
  coordinates::{BoundingBox, PixelPosition, PixelRect},
  map_event::MapEvent,
};

mod helpers;
mod layer_store;
pub mod timeline_widget;

pub mod layer;
pub use super::viewport::{GeometrySnapshot, MapViewport};
pub use layer::ParameterUpdate;
pub use layer_store::MapLayerHolder;

pub struct Map {
  transform: Transform,
  layers: MapLayerStore,
  timeline: Rc<RefCell<TimelineLayer>>,
  recv: Rc<Receiver<MapEvent>>,
  layer_sender: Sender<MapEvent>,
  command_sender: Sender<ParameterUpdate>,
  repaint: RepaintSignal,
  map_state: Arc<RwLock<MapState>>,
  right_click: Option<PixelCoordinate>,
  right_click_screen_pos: Option<egui::Pos2>,
  right_click_open_requested: bool,
  keys: Vec<String>,
  config: Config,
  last_viewport: Option<MapViewport>,
  bevy_pointer_blocking_rects: Vec<PixelRect>,
  screenshot_target: ScreenshotTarget,
  shutdown_requested: bool,
}

enum ScreenshotTarget {
  Egui(Sender<MapEvent>),
  Bevy(Vec<PathBuf>),
}

#[derive(Clone, Copy)]
enum MapConstructionMode {
  Egui { include_tiles: bool },
  Bevy,
}

impl Map {
  #[must_use]
  pub fn new_egui(
    ctx: egui::Context,
    cfg: crate::config::Config,
  ) -> (Self, Remote, Rc<dyn MapLayerHolder>) {
    let repaint = RepaintSignal::egui(ctx.clone());
    Self::new_impl(
      Some(ctx),
      cfg,
      MapConstructionMode::Egui {
        include_tiles: true,
      },
      repaint,
    )
  }

  #[must_use]
  pub fn new_egui_without_tiles(
    ctx: egui::Context,
    cfg: crate::config::Config,
  ) -> (Self, Remote, Rc<dyn MapLayerHolder>) {
    let repaint = RepaintSignal::egui(ctx.clone());
    Self::new_impl(
      Some(ctx),
      cfg,
      MapConstructionMode::Egui {
        include_tiles: false,
      },
      repaint,
    )
  }

  #[must_use]
  pub fn new_bevy(
    cfg: crate::config::Config,
    repaint: RepaintSignal,
  ) -> (Self, Remote, Rc<dyn MapLayerHolder>) {
    Self::new_impl(None, cfg, MapConstructionMode::Bevy, repaint)
  }

  fn new_impl(
    ctx: Option<egui::Context>,
    cfg: crate::config::Config,
    mode: MapConstructionMode,
    repaint: RepaintSignal,
  ) -> (Self, Remote, Rc<dyn MapLayerHolder>) {
    let tile_layer = match mode {
      MapConstructionMode::Egui {
        include_tiles: true,
      } => {
        let ctx = ctx
          .as_ref()
          .expect("egui tile construction requires an egui context");
        Some(layer::TileLayer::from_config(ctx.clone(), &cfg))
      }
      MapConstructionMode::Egui {
        include_tiles: false,
      }
      | MapConstructionMode::Bevy => None,
    };
    let shape_info = std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
    let shape_layer = layer::ShapeLayer::new(cfg.clone(), repaint.clone(), shape_info.clone());
    let shape_layer_sender = shape_layer.get_sender();
    let shape_layer_vis_sender = shape_layer.get_vis_sender();
    let shape_layer_shape_vis_sender = shape_layer.get_shape_vis_sender();
    let map_state = std::sync::Arc::new(std::sync::RwLock::new(crate::remote::MapState::default()));

    let (command, command_sender) = layer::CommandLayer::new();

    let (screenshot_layer, screenshot_target) = match mode {
      MapConstructionMode::Bevy => (None, ScreenshotTarget::Bevy(Vec::new())),
      MapConstructionMode::Egui { .. } => {
        let ctx = ctx
          .as_ref()
          .expect("egui screenshot construction requires an egui context");
        let screenshot_layer = layer::ScreenshotLayer::new(ctx.clone());
        let screenshot_layer_sender = screenshot_layer.get_sender();
        (
          Some(screenshot_layer),
          ScreenshotTarget::Egui(screenshot_layer_sender),
        )
      }
    };

    let timeline = Rc::new(RefCell::new(layer::TimelineLayer::new()));

    let (send, recv) = std::sync::mpsc::channel();

    let keys = command.register_keys().fold(Vec::new(), |mut acc, key| {
      if !acc.contains(&key.to_string()) {
        acc.push(key.to_string());
      }
      acc
    });

    let mut layer_vec: Vec<Box<dyn EguiLayer>> = Vec::new();
    if let Some(tl) = tile_layer {
      layer_vec.push(Box::new(tl));
    }
    layer_vec.push(Box::new(shape_layer));
    layer_vec.push(Box::new(command));
    if let Some(screenshot_layer) = screenshot_layer {
      layer_vec.push(Box::new(screenshot_layer));
    }
    let layers = MapLayerStore::new(layer_vec);

    let map_data_holder = Rc::new(MapLayerHolderImpl::new(layers.clone(), timeline.clone()));

    let remote = Remote {
      layer: shape_layer_sender.clone(),
      map_events: send.clone(),
      command: command_sender.clone(),
      layer_vis: shape_layer_vis_sender,
      shape_vis: shape_layer_shape_vis_sender,
      state: map_state.clone(),
      shape_info,
      repaint: repaint.clone(),
    };
    (
      Self {
        transform: Transform::invalid(),
        layers,
        timeline,
        recv: recv.into(),
        layer_sender: shape_layer_sender,
        command_sender,
        repaint,
        map_state,
        right_click: None,
        right_click_screen_pos: None,
        right_click_open_requested: false,
        keys,
        config: cfg,
        last_viewport: None,
        bevy_pointer_blocking_rects: Vec::new(),
        screenshot_target,
        shutdown_requested: false,
      },
      remote,
      map_data_holder,
    )
  }

  pub fn handle_double_click(&mut self, pos: PixelPosition) {
    log::debug!("Map: Double-click detected at {pos:?}");

    let transform = self.transform;
    self.layers.for_each_neutral_mut(|layer| {
      if let Some(distance) = layer.closest_geometry_with_selection(pos, &transform) {
        log::debug!(
          "Map: Layer '{}' can handle at distance: {:.2}",
          layer.name(),
          distance
        );
      }
    });
  }

  pub fn handle_command_drag(
    &self,
    pos: PixelPosition,
    delta: PixelPosition,
    transform: Transform,
  ) {
    let _ = self
      .command_sender
      .send(ParameterUpdate::DragUpdate(pos, delta, transform));
  }

  pub fn open_command_context_menu(&mut self, pos: PixelPosition) {
    self.right_click = Some(point_to_coordinate(pos, &self.transform));
    self.right_click_screen_pos = Some(pixel_to_pos(pos));
    self.right_click_open_requested = true;
  }

  fn clear_command_context_menu(&mut self) {
    self.right_click = None;
    self.right_click_screen_pos = None;
  }

  fn command_context_menu_content(&self, ui: &mut egui::Ui, right_click: PixelCoordinate) -> bool {
    let mut close = false;
    ui.set_min_width(140.);
    let wgs84 = right_click.as_wgs84();
    ui.label(format!("{:.6},{:.6}", wgs84.lat, wgs84.lon));
    for key in &self.keys {
      if ui.button(key).clicked() {
        let _ = self
          .command_sender
          .send(ParameterUpdate::Update(key.clone(), Some(right_click)))
          .inspect_err(|e| {
            log::error!("Failed to send {key}: {e:?}");
          });
        close = true;
      }
    }
    close
  }

  fn show_bevy_command_context_menu(&mut self, ctx: &egui::Context) {
    let Some(right_click) = self.right_click else {
      return;
    };
    let Some(screen_pos) = self.right_click_screen_pos else {
      return;
    };

    let mut close = false;
    let inner = egui::Area::new(egui::Id::new("bevy_map_command_context_menu"))
      .order(egui::Order::Foreground)
      .fixed_pos(screen_pos)
      .show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
          close = self.command_context_menu_content(ui, right_click);
        });
      });

    let close_from_input = ctx.input(|input| {
      input.key_pressed(egui::Key::Escape)
        || (!self.right_click_open_requested
          && input.pointer.any_click()
          && input
            .pointer
            .interact_pos()
            .is_some_and(|pos| !inner.response.rect.contains(pos)))
    });

    if close || close_from_input {
      self.clear_command_context_menu();
    }
  }

  pub fn handle_bevy_hover(&mut self, pos: Option<PixelPosition>) {
    let transform = self.transform;
    self.layers.for_each_neutral_mut(|layer| {
      layer.handle_hover(pos, &transform);
    });
  }

  fn handle_keys(
    &mut self,
    ctx: &egui::Context,
    events: impl Iterator<Item = egui::Event>,
    rect: PixelRect,
  ) {
    for event in events {
      if let egui::Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
      } = event
      {
        match key {
          egui::Key::Delete => self.clear(),

          egui::Key::ArrowDown => {
            let _ = self.transform.translate(PixelPosition { x: 0., y: -10. });
          }
          egui::Key::ArrowLeft => {
            let _ = self.transform.translate(PixelPosition { x: 10., y: 0. });
          }
          egui::Key::ArrowRight => {
            let _ = self.transform.translate(PixelPosition { x: -10., y: 0. });
          }
          egui::Key::ArrowUp => {
            let _ = self.transform.translate(PixelPosition { x: 0., y: 10. });
          }

          egui::Key::Minus => {
            zoom_with_center(&mut self.transform, 0.9, rect.center());
          }
          egui::Key::Plus | egui::Key::Equals => {
            zoom_with_center(&mut self.transform, 1. / 0.9, rect.center());
          }

          egui::Key::F => {
            self.show_bounding_box(rect);
          }

          egui::Key::V => self.paste(),
          egui::Key::C => self.copy(),
          egui::Key::S => self.request_screenshot_if_no_text_focus(ctx),
          _ => {
            debug!("Unhandled key pressed: {key:?} {modifiers:?}");
          }
        }
      }
    }
  }

  fn show_bounding_box(&mut self, rect: PixelRect) {
    let mut bb = self
      .layers
      .fold_neutral(BoundingBox::get_invalid(), |acc, layer| {
        layer.bounding_box().map_or(acc, |bb| acc.extend(&bb))
      });
    if bb.is_valid() {
      if !bb.is_box() {
        bb.frame(0.02);
      }

      show_box(&mut self.transform, &bb, rect);
    }
  }

  pub fn focus_all_layers(&mut self) {
    if let Some(viewport) = self.last_viewport {
      self.show_bounding_box(viewport.rect);
    }
  }

  fn show_layer_bounding_box(&mut self, id: &str, rect: PixelRect) {
    let mut bb = self
      .layers
      .fold_neutral(BoundingBox::get_invalid(), |acc, layer| {
        layer
          .sub_layer_bounding_box(id)
          .map_or(acc, |bb| acc.extend(&bb))
      });
    if bb.is_valid() {
      if !bb.is_box() {
        bb.frame(0.02);
      }
      show_box(&mut self.transform, &bb, rect);
    }
  }

  fn show_shape_bounding_box(&mut self, layer_id: &str, shape_idx: usize, rect: PixelRect) {
    let Some(mut bb) = self
      .layers
      .find_neutral(|layer| layer.shape_bounding_box(layer_id, shape_idx))
    else {
      return;
    };
    if !bb.is_box() {
      bb.frame(0.02);
    }
    show_box(&mut self.transform, &bb, rect);
  }

  fn focus_on_coordinate(
    &mut self,
    coordinate: WGS84Coordinate,
    zoom_level: Option<u8>,
    rect: PixelRect,
  ) {
    use crate::map::coordinates::PixelCoordinate;

    // Convert WGS84 coordinate to pixel coordinate
    let pixel_coord = PixelCoordinate::from(coordinate);

    let tile_zoom = zoom_level.unwrap_or(15);
    self.transform.zoom = transform_zoom_for_tile_zoom(tile_zoom).clamp(1.0, 524_288.0);

    // Center the map on the coordinate
    set_coordinate_to_pixel(pixel_coord, rect.center(), &mut self.transform);

    log::info!(
      "Focused on coordinate: {:.4}, {:.4} with transform zoom: {:.2}, tile_zoom: {:?}",
      coordinate.lat,
      coordinate.lon,
      self.transform.zoom,
      Some(tile_zoom)
    );
  }

  pub fn paste(&self) {
    let sender = self.layer_sender.clone();
    rayon::spawn(move || {
      let Ok(mut clipboard) = Clipboard::new() else {
        warn!("Failed to access clipboard");
        return;
      };
      if let Ok(text) = clipboard.get_text() {
        let mut json = JsonParser::new();
        let _ = json.parse_line(&text);
        JsonParser::new().parse_line(&text);
        if let Some(map_event) = json.finalize() {
          let _ = sender.send(map_event);
          return;
        }

        if let Some(map_event) = GrepParser::new(false).parse_line(&text) {
          let _ = sender.send(map_event);
        }
      }
    });
  }

  fn handle_mouse_wheel(&mut self, ui: &Ui, response: &Response) {
    if response.hovered() {
      // check mouse wheel movvement
      let delta = ui
        .input(|i| {
          i.events
            .iter()
            .find_map(move |e| match e {
              egui::Event::MouseWheel {
                unit: _,
                delta,
                modifiers: _,
              } => Some(delta),
              _ => None,
            })
            .copied()
        })
        .map(|d| (d.y / 1. + 1.).clamp(0.8, 1.4).sqrt());
      if let Some(delta) = delta {
        let cursor = pos_to_pixel(response.hover_pos().unwrap_or_default());
        zoom_with_center(&mut self.transform, delta, cursor);
      }
    }
  }

  fn request_screenshot_path(&mut self, path: PathBuf) {
    match &mut self.screenshot_target {
      ScreenshotTarget::Egui(sender) => {
        let _ = sender.send(MapEvent::Screenshot(path));
      }
      ScreenshotTarget::Bevy(paths) => {
        paths.push(path);
      }
    }
  }

  fn handle_map_events(&mut self, rect: PixelRect) {
    profile_scope!("Map::handle_map_events");
    let events = self.recv.try_iter().collect::<Vec<_>>();

    // Process Clear events first (they should happen before new data)
    for event in &events {
      if matches!(event, MapEvent::Clear) {
        self.clear();
      }
    }

    // Check if we have focus or screenshot events - process pending layer data before these
    let has_focus_event = events.iter().any(|e| {
      matches!(
        e,
        MapEvent::Focus | MapEvent::FocusLayer { .. } | MapEvent::FocusShape { .. }
      )
    });
    let has_screenshot_event = events.iter().any(|e| matches!(e, MapEvent::Screenshot(_)));

    if has_focus_event || has_screenshot_event {
      self.process_pending_layer_data();
    }

    // Process remaining events (Focus and Screenshot)
    for event in &events {
      match event {
        MapEvent::Focus => self.show_bounding_box(rect),
        MapEvent::FocusLayer { id } => self.show_layer_bounding_box(id, rect),
        MapEvent::FocusShape {
          layer_id,
          shape_idx,
        } => {
          self.show_shape_bounding_box(layer_id, *shape_idx, rect);
        }
        MapEvent::FocusOn {
          coordinate,
          zoom_level,
        } => {
          log::info!(
            "Processing FocusOn event for coordinate: {:.4}, {:.4}, zoom: {:?}",
            coordinate.lat,
            coordinate.lon,
            zoom_level
          );
          self.focus_on_coordinate(*coordinate, *zoom_level, rect);
        }
        MapEvent::Screenshot(path) => {
          // Screenshots are routed here, after pending focus events, so captures
          // use the intended viewport.
          self.request_screenshot_path(path.clone());
        }
        MapEvent::Clear => {
          // Already processed above
        }
        MapEvent::Shutdown => {
          self.shutdown_requested = true;
        }
        _ => (),
      }
    }
    if !events.is_empty() {
      self.repaint.request_repaint();
    }
  }

  fn process_pending_layer_data(&mut self) {
    // Force all layers to process any pending data immediately
    self.layers.for_each_neutral_mut(|layer| {
      // For shape layers, this will process any pending MapEvent::Layer events
      // For other layers, this is typically a no-op
      layer.process_pending_events();
    });
  }

  pub fn clear(&mut self) {
    // First, discard any pending layer events to prevent them from being processed later
    self
      .layers
      .for_each_neutral_mut(|layer| layer.discard_pending_events());

    // Then clear the actual layer data
    self.layers.for_each_neutral_mut(|layer| layer.clear());
  }

  pub fn handle_dropped_file(&self, file_path: PathBuf) {
    let sender = self.layer_sender.clone();
    let repaint = self.repaint.clone();
    tokio::spawn(async move {
      let mut auto_parser = crate::parser::AutoFileParser::new(&file_path);
      for map_event in auto_parser.parse() {
        let _ = sender.send(map_event);
        repaint.request_repaint();
      }
    });
  }

  fn handle_dropped_files(&self, ctx: &egui::Context) {
    for file in ctx.input(|i| i.raw.dropped_files.clone()) {
      if let Some(file_path) = file.path {
        self.handle_dropped_file(file_path);
      }
    }
  }

  #[expect(clippy::unused_self)]
  pub fn copy(&self) {
    // TODO
  }

  fn request_screenshot_if_no_text_focus(&mut self, ctx: &egui::Context) {
    let has_text_focus = ctx.memory(|mem| mem.focused().is_some());
    if !has_text_focus {
      self.request_screenshot();
    }
  }

  pub fn request_screenshot(&mut self) {
    let path = helpers::current_time_screenshot_name();
    self.request_screenshot_path(path);
  }

  pub fn take_screenshot_requests(&mut self) -> Vec<PathBuf> {
    match &mut self.screenshot_target {
      ScreenshotTarget::Egui(_) => Vec::new(),
      ScreenshotTarget::Bevy(paths) => std::mem::take(paths),
    }
  }

  pub fn take_shutdown_requested(&mut self) -> bool {
    std::mem::take(&mut self.shutdown_requested)
  }

  fn handle_egui_map_input(&mut self, ui: &Ui, response: &Response, rect: PixelRect) {
    self.handle_dropped_files(ui.ctx());
    self.handle_mouse_wheel(ui, response);

    let events = ui.input(|i: &InputState| {
      i.events
        .iter()
        .filter(|e| matches!(e, egui::Event::Key { .. }))
        .cloned()
        .collect::<Vec<_>>()
    });
    self.handle_keys(ui.ctx(), events.into_iter(), rect);

    if response.double_clicked()
      && let Some(pos) = response.hover_pos()
    {
      self.handle_double_click(pos_to_pixel(pos));
    }

    if !self.right_click_open_requested && !response.context_menu_opened() {
      self.clear_command_context_menu();
    }
    if response.secondary_clicked()
      && let Some(pos) = response.hover_pos()
    {
      self.open_command_context_menu(pos_to_pixel(pos));
    }

    let mut close_menu = false;
    if let Some(right_click) = self.right_click {
      response.context_menu(|ui| {
        if self.command_context_menu_content(ui, right_click) {
          ui.close();
          close_menu = true;
        }
      });
    }
    if close_menu {
      self.clear_command_context_menu();
    }
    self.right_click_open_requested = false;

    if response.dragged()
      && response.dragged_by(PointerButton::Secondary)
      && let Some(hover_pos) = response.hover_pos()
    {
      let delta = PixelPosition {
        x: response.drag_delta().x,
        y: response.drag_delta().y,
      };

      self.handle_command_drag(pos_to_pixel(hover_pos), delta, self.transform);
    }

    if response.dragged() && response.dragged_by(PointerButton::Primary) {
      self.transform.translate(PixelPosition {
        x: response.drag_delta().x,
        y: response.drag_delta().y,
      });
    }
  }

  fn prepare_bevy_layer_frame(&mut self, ui: &mut Ui, rect: Rect) {
    profile_scope!("Map::prepare_bevy_layer_frame");
    if !ui.is_rect_visible(rect) {
      return;
    }

    let transform = self.transform;
    let timeline = self.timeline.clone();
    self
      .layers
      .try_with_egui_layers_mut("bevy layer frame", |layers| {
        for layer in layers.iter_mut() {
          layer.process_pending_events();
        }
        profile_scope!("Layer::draw_egui", "Timeline");
        timeline.borrow_mut().draw_egui(ui, rect);
        Self::draw_egui_layer_overlays(layers, ui, &transform);
      });
  }

  fn draw_egui_layer_frame(&mut self, ui: &mut Ui, frame: EguiMapFrame) {
    profile_scope!("Map::draw_egui_layers");
    let rect = frame.rect;
    if !ui.is_rect_visible(rect) {
      return;
    }

    let transform = self.transform;
    let timeline = self.timeline.clone();
    self
      .layers
      .try_with_egui_layers_mut("egui layer frame", |layers| {
        Self::draw_egui_layers(layers, &timeline, ui, &transform, frame);
      });
  }

  fn draw_egui_layer_overlays(
    layers: &mut [Box<dyn EguiLayer>],
    ui: &mut Ui,
    transform: &Transform,
  ) {
    for layer in layers {
      profile_scope!("Layer::draw_egui_overlay", layer.name());
      layer.draw_egui_overlay(ui, transform);
    }
  }

  fn draw_egui_layers(
    layers: &mut [Box<dyn EguiLayer>],
    timeline: &RefCell<TimelineLayer>,
    ui: &mut Ui,
    transform: &Transform,
    frame: EguiMapFrame,
  ) {
    for layer in layers.iter_mut() {
      layer.process_pending_events();
      profile_scope!("Layer::draw_egui", layer.name());
      layer.draw_egui(ui, transform, frame);
    }

    profile_scope!("Layer::draw_egui", "Timeline");
    timeline.borrow_mut().draw_egui(ui, frame.rect);
    for layer in layers.iter() {
      layer.draw_egui_attribution(ui, frame.rect);
    }
    Self::draw_egui_layer_overlays(layers, ui, transform);
  }

  fn allocate_map_response(ui: &mut Ui, sense: Sense) -> (Rect, Response) {
    let size = ui.available_size();
    ui.allocate_exact_size(size, sense)
  }

  fn initialize_transform_if_needed(&mut self, rect: PixelRect) {
    if self.transform.is_invalid() {
      fit_to_screen(&mut self.transform, rect);
      set_coordinate_to_pixel(
        PixelCoordinate { x: 500., y: 500. },
        rect.center(),
        &mut self.transform,
      );

      assert!(
        !self.transform.is_invalid(),
        "Transform: {:?}",
        self.transform
      );
    }
  }

  fn update_frame_viewport(&mut self, rect: PixelRect) {
    fit_to_screen(&mut self.transform, rect);
    self.last_viewport = Some(MapViewport {
      transform: self.transform,
      rect,
    });
  }

  fn update_bevy_pointer_blocking_rects(&mut self, rect: Rect) {
    self.bevy_pointer_blocking_rects.clear();
    if let Some(timeline_rect) = self.timeline.borrow().input_rect(rect) {
      self
        .bevy_pointer_blocking_rects
        .push(PixelRect::from_min_max(
          pos_to_pixel(timeline_rect.min),
          pos_to_pixel(timeline_rect.max),
        ));
    }
  }

  pub fn ui_egui(&mut self, ui: &mut Ui) -> Response {
    profile_scope!("Map::ui_egui");
    let (rect, response) = Self::allocate_map_response(ui, Sense::click_and_drag());
    let frame = EguiMapFrame::from_rect(rect);
    let pixel_rect = frame.pixel_rect;

    self.initialize_transform_if_needed(pixel_rect);
    self.handle_egui_map_input(ui, &response, pixel_rect);
    self.update_frame_viewport(pixel_rect);
    self.draw_egui_layer_frame(ui, frame);
    self.handle_map_events(pixel_rect);

    response
  }

  pub fn ui_bevy(&mut self, ui: &mut Ui) -> Response {
    profile_scope!("Map::ui_bevy");
    let (rect, response) = Self::allocate_map_response(ui, Sense::hover());
    let pixel_rect = PixelRect::from_min_max(pos_to_pixel(rect.min), pos_to_pixel(rect.max));

    self.initialize_transform_if_needed(pixel_rect);
    self.show_bevy_command_context_menu(ui.ctx());
    self.right_click_open_requested = false;
    self.update_frame_viewport(pixel_rect);
    self.update_bevy_pointer_blocking_rects(rect);
    self.prepare_bevy_layer_frame(ui, rect);
    self.handle_map_events(pixel_rect);

    response
  }

  pub fn prepare_headless_bevy_frame(&mut self, rect: PixelRect) {
    profile_scope!("Map::prepare_headless_bevy_frame");
    self.initialize_transform_if_needed(rect);
    self.update_frame_viewport(rect);
    self.process_pending_layer_data();
    self.handle_map_events(rect);
    self.update_state_snapshot();
  }
}

impl Map {
  pub fn set_headless(&mut self) {
    self
      .layers
      .for_each_neutral_mut(|layer| layer.set_headless());
  }

  #[must_use]
  pub fn has_pending_work(&self) -> bool {
    self.layers.any_neutral(|layer| layer.has_pending_work())
  }

  /// Check if a double-click just occurred on any layer
  #[must_use]
  pub fn has_double_click_action(&self) -> bool {
    self
      .layers
      .any_neutral(|layer| layer.has_double_click_action())
  }

  /// Refresh the shared `MapState` snapshot that the HTTP `/state` endpoint serves.
  pub fn update_state_snapshot(&self) {
    let sub_layers = self
      .layers
      .fold_neutral(Vec::new(), |mut sub_layers, layer| {
        sub_layers.extend(
          layer
            .sub_layers()
            .into_iter()
            .map(|s| crate::remote::LayerInfo {
              id: s.id,
              visible: s.visible,
              shape_count: s.shape_count,
            }),
        );
        sub_layers
      });
    let Ok(mut state) = self.map_state.try_write() else {
      return;
    };
    state.layers = sub_layers;
  }

  #[must_use]
  pub fn viewport(&self) -> Option<MapViewport> {
    self.last_viewport
  }

  #[must_use]
  pub fn bevy_pointer_blocking_rects(&self) -> &[PixelRect] {
    &self.bevy_pointer_blocking_rects
  }

  pub fn set_transform(&mut self, transform: Transform) {
    self.transform = transform;
  }

  #[must_use]
  pub fn geometry_snapshot_since(&self, geometry_version: u64) -> GeometrySnapshot {
    self.geometry_snapshot_since_version(Some(geometry_version))
  }

  fn geometry_snapshot_since_version(
    &self,
    known_geometry_version: Option<u64>,
  ) -> GeometrySnapshot {
    let geometry_version = self.layers.fold_neutral(0_u64, |version, layer| {
      version
        .wrapping_mul(31)
        .wrapping_add(layer.geometry_base_snapshot_version())
    });
    let include_geometries = known_geometry_version != Some(geometry_version);

    self
      .layers
      .fold_neutral(GeometrySnapshot::default(), |mut acc, layer| {
        let mut snapshot = if include_geometries {
          layer.geometry_snapshot()
        } else {
          layer.geometry_snapshot_without_geometries()
        };
        acc.version = acc.version.wrapping_mul(31).wrapping_add(snapshot.version);
        acc.geometry_version = acc
          .geometry_version
          .wrapping_mul(31)
          .wrapping_add(snapshot.geometry_version);
        acc.geometries.append(&mut snapshot.geometries);
        acc
          .highlighted_geometries
          .append(&mut snapshot.highlighted_geometries);
        acc
      })
  }

  #[must_use]
  pub fn geometry_snapshot_version(&self) -> u64 {
    self.layers.fold_neutral(0, |version, layer| {
      version
        .wrapping_mul(31)
        .wrapping_add(layer.geometry_snapshot_version())
    })
  }

  /// Update the config for the map and all its layers
  pub fn update_config(&mut self, new_config: &crate::config::Config) {
    self.config = new_config.clone();

    // Update all layers that need config updates
    self
      .layers
      .for_each_neutral_mut(|layer| layer.update_config(new_config));
  }

  /// Set temporal filtering for all layers
  pub fn set_temporal_filter(
    &mut self,
    current_time: Option<chrono::DateTime<chrono::Utc>>,
    time_window: Option<chrono::Duration>,
  ) {
    self
      .layers
      .for_each_neutral_mut(|layer| layer.set_temporal_filter(current_time, time_window));
  }

  /// Update the timeline layer with time range and current interval
  pub fn update_timeline(
    &mut self,
    time_range: (
      Option<chrono::DateTime<chrono::Utc>>,
      Option<chrono::DateTime<chrono::Utc>>,
    ),
    current_interval: (
      Option<chrono::DateTime<chrono::Utc>>,
      Option<chrono::DateTime<chrono::Utc>>,
    ),
    is_playing: bool,
    playback_speed: f32,
  ) {
    self.timeline.borrow_mut().update_timeline(
      time_range,
      current_interval,
      is_playing,
      playback_speed,
    );
  }

  /// Get the current timeline interval from the timeline layer
  #[must_use]
  pub fn get_timeline_interval(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  ) {
    self.timeline.borrow().get_interval()
  }

  /// Get the timeline playback state from the timeline layer
  #[must_use]
  pub fn get_timeline_playback_state(&self) -> (bool, f32) {
    self.timeline.borrow().playback_state()
  }

  /// Set the timeline layer visibility
  pub fn set_timeline_visible(&mut self, visible: bool) {
    self.timeline.borrow_mut().set_visible(visible);
  }

  /// Check if the timeline layer is visible
  #[must_use]
  pub fn is_timeline_visible(&self) -> bool {
    self.timeline.borrow().visible()
  }

  /// Toggle timeline interval lock state
  pub fn toggle_timeline_interval_lock(&mut self) {
    self.timeline.borrow_mut().toggle_interval_lock();
  }

  /// Get current timeline interval lock state
  #[must_use]
  pub fn get_timeline_interval_lock(&self) -> crate::map::IntervalLock {
    self.timeline.borrow().interval_lock()
  }

  /// Search geometries across all layers
  pub fn search_geometries(&mut self, query: &str) {
    self.layers.for_each_neutral_mut(|layer| {
      if let Some(s) = layer.as_searchable_mut() {
        s.search_geometries(query);
      }
    });
  }

  /// Navigate to next search result
  pub fn next_search_result(&mut self) -> bool {
    self
      .layers
      .find_neutral_mut(|layer| {
        let s = layer.as_searchable_mut()?;
        s.next_search_result().then_some(())
      })
      .is_some()
  }

  /// Navigate to previous search result
  pub fn previous_search_result(&mut self) -> bool {
    self
      .layers
      .find_neutral_mut(|layer| {
        let s = layer.as_searchable_mut()?;
        s.previous_search_result().then_some(())
      })
      .is_some()
  }

  /// Show popup for current search result
  pub fn show_search_result_popup(&mut self) {
    self.layers.for_each_neutral_mut(|layer| {
      if let Some(s) = layer.as_searchable_mut() {
        s.show_search_result_popup();
      }
    });
  }

  /// Filter geometries across all layers
  pub fn filter_geometries(&mut self, query: &str) {
    self.layers.for_each_neutral_mut(|layer| {
      if let Some(s) = layer.as_searchable_mut() {
        s.filter_geometries(query);
      }
    });
  }

  /// Clear filter and show all geometries
  pub fn clear_filter(&mut self) {
    self.layers.for_each_neutral_mut(|layer| {
      if let Some(s) = layer.as_searchable_mut() {
        s.clear_filter();
      }
    });
  }

  /// Get the count of search results
  #[must_use]
  pub fn get_search_results_count(&self) -> usize {
    self
      .layers
      .find_neutral(|layer| {
        let count = layer.as_searchable()?.search_results_count();
        (count > 0).then_some(count)
      })
      .unwrap_or(0)
  }

  /// Show a specific layer
  pub fn show_layer(&mut self, layer_name: &str) -> bool {
    self
      .layers
      .find_neutral_mut(|layer| {
        (layer.name() == layer_name).then(|| {
          layer.set_visible(true);
        })
      })
      .is_some()
  }

  /// Hide a specific layer
  pub fn hide_layer(&mut self, layer_name: &str) -> bool {
    self
      .layers
      .find_neutral_mut(|layer| {
        (layer.name() == layer_name).then(|| {
          layer.set_visible(false);
        })
      })
      .is_some()
  }

  /// Toggle visibility of a specific layer
  pub fn toggle_layer(&mut self, layer_name: &str) -> bool {
    self
      .layers
      .find_neutral_mut(|layer| {
        (layer.name() == layer_name).then(|| {
          layer.set_visible(!layer.is_visible());
        })
      })
      .is_some()
  }

  /// Zoom in
  pub fn zoom_in(&mut self) {
    self.transform.zoom *= 1.5;
  }

  /// Zoom out
  pub fn zoom_out(&mut self) {
    self.transform.zoom /= 1.5;
  }

  /// Zoom to fit all geometries
  pub fn zoom_fit(&mut self) {
    // TODO: Implement zoom to fit functionality
    // This would require calculating bounding box of all geometries
  }
}
