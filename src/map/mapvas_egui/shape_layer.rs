use std::{
  collections::HashMap,
  iter::once,
  sync::mpsc::{Receiver, Sender},
};

use egui::{
  epaint::{PathShape, PathStroke},
  Color32, Rect, Ui,
};

use crate::map::{
  coordinates::{Coordinate, PixelPosition, Transform},
  map_event::{self, Color, FillStyle, Layer as EventLayer, MapEvent, Style},
};

use super::Layer;

struct Shape {
  shape: egui::Shape,
}

impl Shape {
  pub fn from_shape_with_transform(event: &map_event::Shape, transform: &Transform) -> Self {
    let points = event
      .coordinates
      .iter()
      .map(|coord| PixelPosition::into((*coord).into()))
      .map(|pos| transform.apply(pos).into())
      .collect();

    Self {
      shape: egui::Shape::Path(PathShape {
        points,
        closed: event.style.fill != FillStyle::NoFill,
        fill: event.style.color.into(),
        stroke: PathStroke::new(1.0, event.style.color),
      }),
    }
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
  #[expect(dead_code)]
  ctx: egui::Context,
}

impl ShapeLayer {
  #[must_use]
  pub fn new(ctx: egui::Context) -> Self {
    let (send, recv) = std::sync::mpsc::channel();

    let s = map_event::Shape {
      coordinates: vec![Coordinate::new(52., 12.), Coordinate::new(52., 10.)],
      style: Style {
        color: Color::Black,
        fill: FillStyle::NoFill,
      },
      visible: true,
      label: None,
    };

    Self {
      shape_map: HashMap::from_iter(once(("test".to_string(), vec![s]))),
      recv,
      send,
      ctx,
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
}
