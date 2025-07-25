use std::{
  rc::Rc,
  sync::{Mutex, MutexGuard, mpsc::Receiver},
};

use crate::{
  map::coordinates::{Coordinate, PixelCoordinate, Transform, WGS84Coordinate},
  parser::{FileParser, GrepParser, JsonParser, Parser},
  remote::Remote,
};
use arboard::Clipboard;
use egui::{InputState, PointerButton, Rect, Response, Sense, Ui, Widget};
use helpers::{
  MAX_ZOOM, MIN_ZOOM, fit_to_screen, point_to_coordinate, set_coordinate_to_pixel, show_box,
};
use layer::Layer;
use log::{debug, info};

use super::{
  coordinates::{BoundingBox, PixelPosition},
  geometry_collection::Metadata,
  map_event::MapEvent,
};

mod helpers;

mod layer;
pub use layer::ParameterUpdate;

#[derive(Debug, Clone)]
pub struct GeometryInfo {
  pub layer_id: String,
  pub geometry_type: String,
  pub coordinate: PixelCoordinate,
  pub wgs84: WGS84Coordinate,
  pub metadata: Metadata,
  pub details: String,
}

pub struct Map {
  transform: Transform,
  layers: Rc<Mutex<Vec<Box<dyn Layer>>>>,
  recv: Rc<Receiver<MapEvent>>,
  ctx: egui::Context,
  remote: Remote,
  right_click: Option<PixelCoordinate>,
  keys: Vec<String>,
  geometry_info: Option<GeometryInfo>,
}

impl Map {
  #[must_use]
  pub fn new(ctx: egui::Context) -> (Self, Remote, Rc<dyn MapLayerHolder>) {
    let cfg = crate::config::Config::new();

    let tile_layer = layer::TileLayer::from_config(ctx.clone(), &cfg);
    let shape_layer = layer::ShapeLayer::new();
    let shape_layer_sender = shape_layer.get_sender();

    let (command, command_sender) = layer::CommandLayer::new();

    let screenshot_layer = layer::ScreenshotLayer::new(ctx.clone());
    let screenshot_layer_sender = screenshot_layer.get_sender();
    let (send, recv) = std::sync::mpsc::channel();

    let keys = command.register_keys().fold(Vec::new(), |mut acc, key| {
      if !acc.contains(&key.to_string()) {
        acc.push(key.to_string());
      }
      acc
    });

    let layers: Rc<Mutex<Vec<Box<dyn Layer>>>> = Rc::new(Mutex::new(vec![
      Box::new(tile_layer),
      Box::new(shape_layer),
      Box::new(command),
      Box::new(screenshot_layer),
    ]));

    let map_data_holder = Rc::new(MapLayerHolderImpl::new(layers.clone()));

    let remote = Remote {
      layer: shape_layer_sender.clone(),
      focus: send.clone(),
      clear: send.clone(),
      shutdown: shape_layer_sender,
      screenshot: screenshot_layer_sender,
      update: ctx.clone(),
      command: command_sender,
    };
    (
      Self {
        transform: Transform::invalid(),
        layers,
        recv: recv.into(),
        ctx,
        remote: remote.clone(),
        right_click: None,
        keys,
        geometry_info: None,
      },
      remote,
      map_data_holder,
    )
  }

  fn handle_double_click(&mut self, pos: egui::Pos2) {
    log::debug!("Map: Double-click detected at {pos:?}");

    if let Ok(mut layers) = self.layers.lock() {
      log::debug!(
        "Map: Finding closest geometry across {} layers",
        layers.len()
      );

      // Find the closest geometry across all layers
      let mut closest_distance = f64::INFINITY;
      let mut closest_layer_idx: Option<usize> = None;

      // Find the closest geometry across all layers - treat all layers the same
      for (i, layer) in layers.iter_mut().enumerate() {
        if let Some(distance) = layer.closest_geometry_with_selection(pos, &self.transform) {
          log::debug!(
            "Map: Layer '{}' can handle at distance: {:.2}",
            layer.name(),
            distance
          );

          if distance < closest_distance {
            closest_distance = distance;
            closest_layer_idx = Some(i);
          }
        }
      }

      // If a layer handled the selection, we're done
      if let Some(layer_idx) = closest_layer_idx {
        let layer_name = layers[layer_idx].name();
        log::debug!(
          "Map: Layer '{layer_name}' handled the selection at distance {closest_distance:.2}"
        );
        return;
      }
    }

    log::debug!("Map: No layer handled the double-click");
    // If no layer handled the event, clear geometry info
    self.geometry_info = None;
  }

  fn show_geometry_info(&mut self, ui: &mut Ui) {
    if let Some(info) = &self.geometry_info {
      let mut close_requested = false;

      egui::Window::new("Geometry Information")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 10.0))
        .show(ui.ctx(), |ui| {
          ui.set_min_width(250.0);

          ui.heading(&info.geometry_type);
          ui.separator();

          ui.horizontal(|ui| {
            ui.label("Layer:");
            ui.strong(&info.layer_id);
          });

          ui.horizontal(|ui| {
            ui.label("Position:");
            ui.label(format!("{:.6}, {:.6}", info.wgs84.lat, info.wgs84.lon));
          });

          ui.horizontal(|ui| {
            ui.label("Details:");
            ui.label(&info.details);
          });

          if let Some(label) = &info.metadata.label {
            ui.horizontal(|ui| {
              ui.label("Label:");
              ui.strong(label);
            });
          }

          if let Some(style) = &info.metadata.style {
            ui.horizontal(|ui| {
              ui.label("Style:");
              let color = egui::RichText::new("●").color(style.color());
              ui.label(color);
              if style.fill_color() != egui::Color32::TRANSPARENT {
                let fill_color = egui::RichText::new("■").color(style.fill_color());
                ui.label(fill_color);
              }
            });
          }

          ui.separator();

          if ui.button("Close").clicked() {
            close_requested = true;
          }
        });

      if close_requested {
        self.geometry_info = None;
      }
    }
  }

  fn handle_keys(&mut self, events: impl Iterator<Item = egui::Event>, rect: Rect) {
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
            self.zoom_with_center(0.9, rect.center().into());
          }
          egui::Key::Plus | egui::Key::Equals => {
            self.zoom_with_center(1. / 0.9, rect.center().into());
          }

          egui::Key::F => {
            self.show_bounding_box(rect);
          }

          egui::Key::V => self.paste(),
          egui::Key::C => self.copy(),
          egui::Key::S => {
            // Don't take screenshot if any text input has focus
            let has_text_focus = self.ctx.memory(|mem| {
              if let Some(_focus_id) = mem.focused() {
                // If any widget has focus, assume it could be a text input to be safe
                // This prevents screenshots when typing in any input field
                true
              } else {
                false
              }
            });

            if !has_text_focus {
              let _ = self
                .remote
                .screenshot
                .send(MapEvent::Screenshot(helpers::current_time_screenshot_name()));
            }
          }
          _ => {
            debug!("Unhandled key pressed: {key:?} {modifiers:?}");
          }
        }
      }
    }
  }

  fn show_bounding_box(&mut self, rect: Rect) {
    let layer_guard = self.layers.try_lock().expect("Failed to lock layers");
    let mut bb = layer_guard
      .iter()
      .filter_map(|l| l.bounding_box())
      .fold(BoundingBox::get_invalid(), |acc, bb| acc.extend(&bb));
    if bb.is_valid() {
      if !bb.is_box() {
        bb.frame(0.02);
      }

      show_box(&mut self.transform, &bb, rect);
    }
  }

  fn focus_on_coordinate(
    &mut self,
    coordinate: WGS84Coordinate,
    zoom_level: Option<u8>,
    rect: Rect,
  ) {
    use crate::map::coordinates::PixelCoordinate;

    // Convert WGS84 coordinate to pixel coordinate
    let pixel_coord = PixelCoordinate::from(coordinate);

    // Set zoom level if specified
    if let Some(tile_zoom) = zoom_level {
      const TILE_SIZE: f32 = 512.0;
      let screen_size = rect.width().max(rect.height());
      let new_transform_zoom = 2f32.powi(i32::from(tile_zoom) - 2) * TILE_SIZE / screen_size;
      self.transform.zoom = new_transform_zoom.clamp(1.0, 524_288.0);
    } else {
      // Default to a good zoom level for viewing a location (equivalent to zoom level 15)
      const TILE_SIZE: f32 = 512.0;
      let screen_size = rect.width().max(rect.height());
      let default_zoom = 2f32.powi(15 - 2) * TILE_SIZE / screen_size;
      self.transform.zoom = default_zoom.clamp(1.0, 524_288.0);
    }

    // Center the map on the coordinate
    helpers::set_coordinate_to_pixel(pixel_coord, rect.center().into(), &mut self.transform);

    log::info!(
      "Focused on coordinate: {:.4}, {:.4} with transform zoom: {:.2}, tile_zoom: {:?}",
      coordinate.lat,
      coordinate.lon,
      self.transform.zoom,
      zoom_level
    );
  }

  fn paste(&self) {
    let sender = self.remote.layer.clone();
    rayon::spawn(move || {
      if let Ok(text) = Clipboard::new().expect("clipboard").get_text() {
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
        let cursor = response.hover_pos().unwrap_or_default().into();
        self.zoom_with_center(delta, cursor);
      }
    }
  }

  fn zoom_with_center(&mut self, delta: f32, center: PixelPosition) {
    if self.transform.zoom * delta < MIN_ZOOM || self.transform.zoom * delta > MAX_ZOOM {
      return;
    }
    let hover_coord: PixelCoordinate = self.transform.invert().apply(center);
    self.transform.zoom(delta);
    set_coordinate_to_pixel(hover_coord, center, &mut self.transform);
  }

  fn handle_map_events(&mut self, rect: Rect) {
    let events = self.recv.try_iter().collect::<Vec<_>>();

    // Process Clear events first (they should happen before new data)
    for event in &events {
      if matches!(event, MapEvent::Clear) {
        self.clear();
      }
    }

    // Check if we have focus or screenshot events - process pending layer data before these
    let has_focus_event = events.iter().any(|e| matches!(e, MapEvent::Focus));
    let has_screenshot_event = events.iter().any(|e| matches!(e, MapEvent::Screenshot(_)));

    if has_focus_event || has_screenshot_event {
      self.process_pending_layer_data();
    }

    // Process remaining events (Focus and Screenshot)
    for event in &events {
      match event {
        MapEvent::Focus => self.show_bounding_box(rect),
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
        MapEvent::Screenshot(_) => {
          // Screenshots are handled by their dedicated layer, but we forward them
          // here to ensure proper timing after focus events
          let _ = self.remote.screenshot.send(event.clone());
        }
        MapEvent::Clear => {
          // Already processed above
        }
        _ => (),
      }
    }
    if !events.is_empty() {
      self.ctx.request_repaint();
    }
  }

  fn process_pending_layer_data(&mut self) {
    // Force all layers to process any pending data immediately
    if let Ok(mut layer_guard) = self.layers.try_lock() {
      for layer in layer_guard.iter_mut() {
        // For shape layers, this will process any pending MapEvent::Layer events
        // For other layers, this is typically a no-op
        layer.process_pending_events();
      }
    }
  }

  fn clear(&mut self) {
    // First, discard any pending layer events to prevent them from being processed later
    if let Ok(mut layer_guard) = self.layers.try_lock() {
      for layer in layer_guard.iter_mut() {
        layer.discard_pending_events();
      }
    }

    // Then clear the actual layer data
    let mut layer_guard = self.layers.try_lock();
    if let Ok(layers) = layer_guard.as_mut() {
      for layer in layers.iter_mut() {
        layer.clear();
      }
    }
  }

  fn handle_dropped_files(&self, ctx: &egui::Context) {
    for file in ctx.input(|i| i.raw.dropped_files.clone()) {
      if let Some(file) = file.path {
        let sender = self.remote.layer.clone();
        let update = self.remote.update.clone();
        tokio::spawn(async move {
          // Buf Reader of file:
          let file = std::fs::File::open(file).inspect_err(|e| {
            log::error!("Failed to open file: {e:?}");
          });
          if let Ok(file) = file {
            let read = Box::new(std::io::BufReader::new(file));
            for map_event in GrepParser::new(false).parse(read) {
              let _ = sender.send(map_event);
              update.request_repaint();
            }
          }
        });
      }
    }
  }

  #[expect(clippy::unused_self)]
  fn copy(&self) {
    // TODO
  }
}

impl Widget for &mut Map {
  fn ui(self, ui: &mut Ui) -> Response {
    let size = ui.available_size();
    let (rect, /*mut*/ response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    if self.transform.is_invalid() {
      fit_to_screen(&mut self.transform, &rect);
      set_coordinate_to_pixel(
        PixelCoordinate { x: 500., y: 500. },
        rect.center().into(),
        &mut self.transform,
      );

      assert!(
        !self.transform.is_invalid(),
        "Transform: {:?}",
        self.transform
      );
    }

    self.handle_dropped_files(ui.ctx());
    self.handle_mouse_wheel(ui, &response);

    let events = ui.input(|i: &InputState| {
      i.events
        .iter()
        .filter(|e| matches!(e, egui::Event::Key { .. }))
        .cloned()
        .collect::<Vec<_>>()
    });
    self.handle_keys(events.into_iter(), rect);

    if response.clicked() {
      info!("Clicked at: {:?}", response.hover_pos());
    }

    if response.double_clicked() {
      if let Some(pos) = response.hover_pos() {
        self.handle_double_click(pos);
      }
    }

    if !response.context_menu_opened() {
      self.right_click = None;
    }
    if response.secondary_clicked() {
      self.right_click = response
        .hover_pos()
        .map(|p| point_to_coordinate(p.into(), &self.transform));
    }

    if let Some(right_click) = self.right_click {
      response.context_menu(|ui| {
        ui.set_min_width(140.);
        let wgs84 = right_click.as_wgs84();
        ui.label(format!("{:.6},{:.6}", wgs84.lat, wgs84.lon));
        for key in &self.keys {
          if ui.button(key).clicked() {
            let _ = self
              .remote
              .command
              .send(ParameterUpdate::Update(key.clone(), Some(right_click)))
              .inspect_err(|e| {
                log::error!("Failed to send {key}: {e:?}");
              });
            ui.close();
          }
        }
      });
    }

    if response.dragged() && response.dragged_by(PointerButton::Secondary) {
      if let Some(hover_pos) = response.hover_pos() {
        let delta = PixelPosition {
          x: response.drag_delta().x,
          y: response.drag_delta().y,
        };

        let _ = self.remote.command.send(ParameterUpdate::DragUpdate(
          hover_pos.into(),
          delta,
          self.transform,
        ));
      }
    }

    if response.dragged() && response.dragged_by(PointerButton::Primary) {
      self.transform.translate(PixelPosition {
        x: response.drag_delta().x,
        y: response.drag_delta().y,
      });
    }

    fit_to_screen(&mut self.transform, &rect);

    if ui.is_rect_visible(rect) {
      if let Ok(mut layer_guard) = self.layers.try_lock().inspect_err(|e| {
        log::error!("Failed to lock layers: {e:?}");
      }) {
        for layer in layer_guard.iter_mut() {
          layer.draw(ui, &self.transform, rect);
        }
      }
    }
    // Handle map events last (and request repaint if there were any) to have all the other input
    // data handled first, so that screenshot or focus events do not miss parts.
    self.handle_map_events(rect);

    // Show geometry info popup if available
    self.show_geometry_info(ui);

    response
  }
}

impl Map {
  #[must_use]
  pub fn has_highlighted_geometry(&self) -> bool {
    if let Ok(layers) = self.layers.lock() {
      for layer in layers.iter() {
        if layer.has_highlighted_geometry() {
          return true;
        }
      }
    }
    false
  }
}

pub trait MapLayerReader {
  fn get_layers(&mut self) -> Box<dyn Iterator<Item = &mut Box<dyn Layer>> + '_>;
}

pub trait MapLayerHolder {
  fn get_reader(&self) -> Box<dyn MapLayerReader + '_>;
}

struct MapLayerHolderImpl(Rc<Mutex<Vec<Box<dyn Layer>>>>);
impl MapLayerHolderImpl {
  fn new(layers: Rc<Mutex<Vec<Box<dyn Layer>>>>) -> Self {
    Self(layers)
  }
}

impl MapLayerHolder for MapLayerHolderImpl {
  fn get_reader(&self) -> Box<dyn MapLayerReader + '_> {
    Box::new(MapLayerReaderImpl(self.0.lock().unwrap()))
  }
}

struct MapLayerReaderImpl<'a>(MutexGuard<'a, Vec<Box<dyn Layer>>>);

impl MapLayerReader for MapLayerReaderImpl<'_> {
  fn get_layers(&mut self) -> Box<dyn Iterator<Item = &mut Box<dyn Layer>> + '_> {
    Box::new(self.0.iter_mut())
  }
}
