use egui::{Event, InputState, PointerButton, Rect, Response, Sense, Ui, Widget};
use helpers::{fit_to_screen, set_coordinate_to_pixel, show_box, MAX_ZOOM, MIN_ZOOM};
use log::debug;
use tile_layer::TileLayer;

use crate::{
  map::coordinates::{PixelPosition, Transform},
  remote::Remote,
};

use super::coordinates::BoundingBox;

mod helpers;
mod shape_layer;
mod tile_layer;

/// A layer that can be drawn on the map.
trait Layer {
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, rect: Rect);
  fn clear(&mut self) {}
  fn bounding_box(&self) -> Option<BoundingBox> {
    None
  }
}

#[derive(Default)]
pub struct Map {
  transform: Transform,
  layers: Vec<Box<dyn Layer>>,
}

impl Map {
  #[must_use]
  pub fn new(ctx: egui::Context) -> (Self, Remote) {
    let tile_layer = TileLayer::new(ctx.clone());
    let shape_layer = shape_layer::ShapeLayer::new(ctx.clone());
    let remote = Remote {
      layer: shape_layer.get_sender(),
      focus: shape_layer.get_sender(),
      clear: shape_layer.get_sender(),
      shutdown: shape_layer.get_sender(),
      screenshot: shape_layer.get_sender(),
    };
    (
      Self {
        transform: Transform::invalid(),
        layers: vec![Box::new(tile_layer), Box::new(shape_layer)],
      },
      remote,
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
          egui::Key::Delete => self.layers.iter_mut().for_each(|l| l.clear()),

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
            let bb = self.layers.iter().filter_map(|l| l.bounding_box()).fold(
              BoundingBox::get_invalid(),
              |mut acc, bb| {
                acc.extend(&bb);
                acc
              },
            );
            show_box(&mut self.transform, &bb, rect);
          }

          egui::Key::V => todo!(),
          egui::Key::C => todo!(),
          _ => {
            debug!("Unhandled key pressed: {key:?} {modifiers:?}");
          }
        }
      }
    }
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
      eprintln!("Clicked {:?} {:?}", response, response.hover_pos());
    }

    if response.dragged() && response.dragged_by(PointerButton::Primary) {
      self.transform.translate(PixelPosition {
        x: response.drag_delta().x,
        y: response.drag_delta().y,
      });
    }

    fit_to_screen(&mut self.transform, &rect);

    if ui.is_rect_visible(rect) {
      for layer in &mut self.layers {
        layer.draw(ui, &self.transform, rect);
      }
    }
    debug!("Transform: {:?}", self.transform);

    response
  }
}
