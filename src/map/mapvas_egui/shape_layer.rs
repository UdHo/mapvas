use super::Layer;
use crate::map::{
  coordinates::{BoundingBox, PixelPosition, Transform},
  map_event::{self, FillStyle, Layer as EventLayer, MapEvent},
};
use egui::{
  Color32, Rect, Stroke, Ui,
  epaint::{CircleShape, PathShape, PathStroke},
};
use std::{
  collections::HashMap,
  sync::mpsc::{Receiver, Sender},
};

struct Shape {
  shape: egui::Shape,
}

impl Shape {
  pub fn from_shape_with_transform(event: &map_event::Shape, transform: &Transform) -> Self {
    let points: Vec<_> = event
      .coordinates
      .iter()
      .map(|coord| PixelPosition::into((*coord).into()))
      .map(|pos| transform.apply(pos).into())
      .collect();

    let shape = if points.len() == 1 {
      egui::Shape::Circle(CircleShape {
        center: points[0],
        radius: 3.0,
        fill: Into::<Color32>::into(event.style.color),
        stroke: Stroke::new(0.0, event.style.color),
      })
    } else {
      egui::Shape::Path(PathShape {
        points,
        closed: event.style.fill != FillStyle::NoFill,
        fill: event.style.color.into(),
        stroke: PathStroke::new(2.0, event.style.color),
      })
    };

    Self { shape }
  }
}

impl From<&map_event::Shape> for Shape {
  fn from(shape: &map_event::Shape) -> Self {
    Self {
      shape: egui::Shape::Path(PathShape {
        points: shape
          .coordinates
          .iter()
          .map(|coord| PixelPosition::into((*coord).into()))
          .collect(),
        closed: shape.style.fill != FillStyle::NoFill,
        fill: Color32::TRANSPARENT,
        stroke: PathStroke::new(1.0, shape.style.color),
      }),
    }
  }
}

pub struct ShapeLayer {
  shape_map: HashMap<String, Vec<map_event::Shape>>,
  recv: Receiver<MapEvent>,
  send: Sender<MapEvent>,
}

impl ShapeLayer {
  #[must_use]
  pub fn new() -> Self {
    let (send, recv) = std::sync::mpsc::channel();

    Self {
      shape_map: HashMap::new(),
      recv,
      send,
    }
  }

  fn handle_new_shapes(&mut self) {
    for event in self.recv.try_iter() {
      if let MapEvent::Layer(EventLayer { id, shapes }) = event {
        let l = self.shape_map.entry(id).or_default();
        l.extend(shapes);
      }
    }
  }

  #[must_use]
  pub fn get_sender(&self) -> Sender<MapEvent> {
    self.send.clone()
  }
}

impl Layer for ShapeLayer {
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, _rect: Rect) {
    self.handle_new_shapes();
    for shape in self.shape_map.values().flatten() {
      ui.painter()
        .add(Shape::from_shape_with_transform(shape, transform).shape);
    }
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    let bb = BoundingBox::from_iterator(
      self
        .shape_map
        .values()
        .flatten()
        .flat_map(|s| s.coordinates.iter().map(|c| PixelPosition::from(*c))),
    );
    bb.is_valid().then_some(bb)
  }

  fn clear(&mut self) {
    self.shape_map.clear();
  }
}
