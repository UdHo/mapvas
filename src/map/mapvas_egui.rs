use std::{
  rc::Rc,
  sync::{Mutex, MutexGuard, mpsc::Receiver},
};

use crate::{
  map::coordinates::{PixelPosition, Transform},
  parser::FileParser,
  parser::GrepParser,
  parser::Parser,
  remote::Remote,
};
use arboard::Clipboard;
use egui::{Event, InputState, PointerButton, Rect, Response, Sense, Ui, Widget};
use helpers::{MAX_ZOOM, MIN_ZOOM, fit_to_screen, set_coordinate_to_pixel, show_box};
use layer::Layer;
use log::{debug, info};

use super::{coordinates::BoundingBox, map_event::MapEvent};

mod helpers;

mod layer;

pub struct Map {
  transform: Transform,
  layers: Rc<Mutex<Vec<Box<dyn Layer>>>>,
  recv: Rc<Receiver<MapEvent>>,
  ctx: egui::Context,
  remote: Remote,
}

impl Map {
  #[must_use]
  pub fn new(ctx: egui::Context) -> (Self, Remote, Rc<dyn MapLayerHolder>) {
    let tile_layer = layer::TileLayer::new(ctx.clone());
    let shape_layer = layer::ShapeLayer::new();
    let shape_layer_sender = shape_layer.get_sender();

    let screenshot_layer = layer::ScreenshotLayer::new(ctx.clone());
    let screenshot_layer_sender = screenshot_layer.get_sender();
    let (send, recv) = std::sync::mpsc::channel();

    let layers: Rc<Mutex<Vec<Box<dyn Layer>>>> = Rc::new(Mutex::new(vec![
      Box::new(tile_layer),
      Box::new(shape_layer),
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
    };
    (
      Self {
        transform: Transform::invalid(),
        layers,
        recv: recv.into(),
        ctx,
        remote: remote.clone(),
      },
      remote,
      map_data_holder,
    )
  }

  fn handle_keys(&mut self, events: impl Iterator<Item = Event>, rect: Rect) {
    for event in events {
      if let Event::Key {
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
            let middle = self.transform.invert().apply(rect.center().into());
            self.zoom_center(0.9, middle);
          }
          egui::Key::Plus | egui::Key::Equals => {
            let middle = self.transform.invert().apply(rect.center().into());
            self.zoom_center(1. / 0.9, middle);
          }

          egui::Key::F => {
            self.show_bounding_box(rect);
          }

          egui::Key::V => self.paste(),
          egui::Key::C => self.copy(),
          egui::Key::S => {
            let _ = self
              .remote
              .screenshot
              .send(MapEvent::Screenshot(helpers::current_time_screenshot_name()));
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
    let bb = layer_guard
      .iter()
      .filter_map(|l| l.bounding_box())
      .fold(BoundingBox::get_invalid(), |acc, bb| acc.extend(&bb));
    show_box(&mut self.transform, &bb, rect);
  }

  fn paste(&self) {
    let sender = self.remote.layer.clone();
    rayon::spawn(move || {
      if let Ok(text) = Clipboard::new().expect("clipboard").get_text() {
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
        self.zoom_center(delta, cursor);
      }
    }
  }

  fn zoom_center(&mut self, delta: f32, center: PixelPosition) {
    if self.transform.zoom * delta < MIN_ZOOM || self.transform.zoom * delta > MAX_ZOOM {
      return;
    }
    let hover_pos: PixelPosition = self.transform.invert().apply(center);
    self.transform.zoom(delta);
    set_coordinate_to_pixel(hover_pos, center, &mut self.transform);
  }

  fn handle_map_events(&mut self, rect: Rect) {
    let events = self.recv.try_iter().collect::<Vec<_>>();
    for event in &events {
      match event {
        MapEvent::Focus => self.show_bounding_box(rect),
        MapEvent::Clear => self.clear(),
        _ => (),
      }
    }
    if !events.is_empty() {
      self.ctx.request_repaint();
    }
  }

  fn clear(&mut self) {
    let mut layer_guard = self.layers.try_lock();
    layer_guard.iter_mut().for_each(|l| l.clear());
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
        PixelPosition { x: 500., y: 500. },
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
    response
  }
}

pub trait MapLayerReader {
  fn get_layers(&mut self) -> Box<dyn Iterator<Item = &mut Box<dyn Layer>> + '_>;
  fn get_layer_contents(&self);
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

  fn get_layer_contents(&self) {
    // TODO
  }
}
