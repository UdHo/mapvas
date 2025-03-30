use super::{Layer, LayerProperties};
use crate::map::{
  coordinates::{BoundingBox, Coordinate, PixelPosition, Transform},
  geometry_collection::{DEFAULT_STYLE, Geometry, Style},
  map_event::{Layer as EventLayer, MapEvent},
};
use egui::{
  Rect, Stroke, Ui,
  epaint::{CircleShape, PathShape, PathStroke},
};
use std::{
  collections::HashMap,
  sync::{
    Arc,
    mpsc::{Receiver, Sender},
  },
};

/// A layer that draws shapes on the map.
pub struct ShapeLayer {
  shape_map: HashMap<String, Vec<Geometry<PixelPosition>>>,
  recv: Arc<Receiver<MapEvent>>,
  send: Sender<MapEvent>,
  layer_properties: LayerProperties,
}

impl ShapeLayer {
  #[must_use]
  pub fn new() -> Self {
    let (send, recv) = std::sync::mpsc::channel();

    Self {
      shape_map: HashMap::new(),
      recv: recv.into(),
      send,
      layer_properties: LayerProperties::default(),
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

const NAME: &str = "Shape Layer";

impl Layer for ShapeLayer {
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, _rect: Rect) {
    self.handle_new_shapes();

    if !self.visible() {
      return;
    }

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

  fn name(&self) -> &str {
    NAME
  }

  fn visible(&self) -> bool {
    self.layer_properties.visible
  }

  fn visible_mut(&mut self) -> &mut bool {
    &mut self.layer_properties.visible
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
