use super::{Layer, LayerProperties, drawable::Drawable as _};
use crate::map::{
  coordinates::{BoundingBox, PixelCoordinate, Transform},
  geometry_collection::Geometry,
  map_event::{Layer as EventLayer, MapEvent},
};
use egui::{Rect, Ui};
use std::{
  collections::HashMap,
  sync::{
    Arc,
    mpsc::{Receiver, Sender},
  },
};

/// A layer that draws shapes on the map.
pub struct ShapeLayer {
  shape_map: HashMap<String, Vec<Geometry<PixelCoordinate>>>,
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
      match event {
        MapEvent::Layer(EventLayer { id, geometries }) => {
          let l = self.shape_map.entry(id).or_default();
          l.extend(geometries.into_iter());
        }
        _ => {
        }
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
  fn process_pending_events(&mut self) {
    // Process any pending layer data immediately
    self.handle_new_shapes();
  }

  fn discard_pending_events(&mut self) {
    // Drain and discard any pending events without processing them
    for _event in self.recv.try_iter() {
      // Just drain the channel, don't process the events
    }
  }

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

  fn ui_content(&mut self, ui: &mut Ui) {
    ui.collapsing("Shapes", |ui| {
      for (id, shapes) in &mut self.shape_map {
        ui.collapsing(id.clone(), |ui| {
          for shape in shapes {
            ui.vertical(|ui| {
              ui.label(format!("{shape:?}"));
            });
          }
        });
      }
    });
  }
}
