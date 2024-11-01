use std::{
  collections::HashMap,
  sync::mpsc::{Receiver, Sender},
};

use egui::{epaint::PathShape, Rect, Stroke, Ui};

use crate::map::{
  coordinates::{PixelPosition, Transform},
  map_event::{Layer as EventLayer, MapEvent, Shape as EventShape, Style},
};

use super::Layer;

struct Shape {
  shape: egui::Shape,
  text: Option<String>,
}

impl Shape {
  fn from(event_shape: EventShape) -> Self {
     let points = event_shape.coordinates.iter().map(PixelPosition::into).map(|pixe|).collect();
    let mut path = PathShape { points, closed: todo!(), fill: todo!(), stroke: todo!()  }
    Self {
      shape: egui::Shape::Path(path),
      text: event_shape.label,
    }
  }
}

impl From<EventShape> for Shape {
  fn from(shape: EventShape) -> Self {
    Self {
      text: shape.label,
    }
  }
}

struct ShapeLayer {
  shape_map: HashMap<String, Vec<egui::Shape>>,
  recv: Receiver<MapEvent>,
  send: Sender<MapEvent>,
  ctx: egui::Context,
}

impl ShapeLayer {
  #[must_use]
  pub fn new(ctx: egui::Context) -> Self {
    let (send, recv) = std::sync::mpsc::channel();

    Self {
      shape_map: HashMap::new(),
      recv,
      send,
      ctx,
    }
  }

  fn handle_new_shapes(&mut self) {
    for event in self.recv.try_iter() {
      if let MapEvent::Layer(EventLayer { id, shapes }) = event {
        let l = self.shape_map.entry(id).or_default();
        l.extend(shapes.into_iter().map(Shape::from));
      }
    }
  }

  pub fn get_sender(&self) -> Sender<MapEvent> {
    self.send.clone()
  }
}

impl Layer for ShapeLayer {
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, rect: Rect) {
    self.handle_new_shapes();

  }
}
