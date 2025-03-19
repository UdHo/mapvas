use super::Layer;
use crate::map::{
  coordinates::{BoundingBox, Coordinate, PixelPosition, Transform},
  geometry_collection::{DEFAULT_STYLE, Geometry, Style},
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
use tracing::instrument;

struct Shape {
  shape: egui::Shape,
}

impl Shape {
  /*pub fn from_shape_with_transform(event: &map_event::Shape, transform: &Transform) -> Self {
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
        fill: fill_color(event.style.fill, event.style.color.into()),
        stroke: PathStroke::new(2.0, event.style.color),
      })
    };

    Self { shape }
  }*/
}

#[instrument]
fn fill_color(style: FillStyle, color: Color32) -> Color32 {
  match style {
    FillStyle::NoFill => Color32::TRANSPARENT,
    FillStyle::Solid => color,
    FillStyle::Transparent => color.gamma_multiply(0.4),
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
  shape_map: HashMap<String, Vec<Geometry<PixelPosition>>>,
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
      if let MapEvent::Layer(EventLayer { id, geometries }) = event {
        let l = self.shape_map.entry(id).or_default();
        l.extend(geometries.into_iter());
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
      shape.draw(ui.painter(), transform);
    }
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    let bb = self
      .shape_map
      .values()
      .flat_map(|s| s.iter().map(Geometry::bounding_box))
      .fold(BoundingBox::default(), |acc, b| acc.extend(&b));

    bb.is_valid().then_some(bb)
  }

  fn clear(&mut self) {
    self.shape_map.clear();
  }
}

type Painter = egui::Painter;

/// An abstraction for anything that can be drawn on the map that is dependent on coordinates/the
/// transformation.
pub trait Drawable {
  fn draw(&self, painter: &Painter, transform: &Transform);
}

impl Drawable for egui::Shape {
  fn draw(&self, painter: &Painter, _transform: &Transform) {
    painter.add(self.clone());
  }
}

impl Drawable for Shape {
  fn draw(&self, painter: &Painter, transform: &Transform) {
    self.shape.draw(painter, transform);
  }
}

impl<C: Coordinate> Drawable for Geometry<C> {
  fn draw(&self, painter: &Painter, transform: &Transform) {
    for el in self
      .flat_iterate_with_merged_style(&Style::default())
      .filter(Geometry::is_visible)
    {
      let shape = match el {
        Geometry::GeometryCollection(_, _) => {
          unreachable!("GeometryCollections should be flattened")
        }
        Geometry::Point(coord, metadata) => {
          let color = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE).color();
          egui::Shape::Circle(CircleShape {
            center: transform.apply(coord.as_pixel_position()).into(),
            radius: 3.0,
            fill: color,
            stroke: Stroke::new(0.0, color),
          })
        }
        Geometry::LineString(coord, metadata) => {
          let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
          egui::Shape::Path(PathShape {
            points: coord
              .iter()
              .map(|c| transform.apply(c.as_pixel_position()).into())
              .collect(),
            closed: false,
            fill: style.fill_color(),
            stroke: PathStroke::new(2.0, style.color()),
          })
        }
        Geometry::Polygon(vec, metadata) => {
          let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
          egui::Shape::Path(PathShape {
            points: vec
              .iter()
              .map(|c| transform.apply(c.as_pixel_position()).into())
              .collect(),
            closed: true,
            fill: style.fill_color(),
            stroke: PathStroke::new(2.0, style.color()),
          })
        }
      };
      painter.add(shape);
    }
  }
}
