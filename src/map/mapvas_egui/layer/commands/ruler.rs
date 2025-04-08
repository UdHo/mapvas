use std::iter::once;
use std::rc::Rc;

use crate::map::coordinates::Coordinate;
use crate::map::{
  coordinates::{PixelCoordinate, distance_in_meters},
  geometry_collection::{Geometry, Metadata, Style},
  mapvas_egui::layer::drawable::Drawable,
};

use super::{Command, ParameterUpdate, update_closest};

pub struct Ruler {
  origin: Option<PixelCoordinate>,
  destination: Option<PixelCoordinate>,
  locked: bool,
  visible: bool,
}

const ORIGIN: &str = "origin";
const DESTINATION: &str = "destination";

impl Default for Ruler {
  fn default() -> Self {
    Self {
      origin: None,
      destination: None,
      locked: false,
      visible: true,
    }
  }
}

impl Command for Ruler {
  fn update_paramters(&mut self, parameters: ParameterUpdate) {
    match parameters {
      ParameterUpdate::Update(key, origin) if key == ORIGIN => self.origin = origin,
      ParameterUpdate::Update(key, destination) if key == DESTINATION => {
        self.destination = destination;
      }
      ParameterUpdate::DragUpdate(mouse_pos, delta, trans) => {
        let mut points: Vec<&mut PixelCoordinate> = self
          .origin
          .iter_mut()
          .chain(self.destination.iter_mut())
          .collect();
        update_closest(mouse_pos, trans, delta, &mut points);
      }
      ParameterUpdate::Update(_, _) => {}
    }
  }

  fn register_keys(&self) -> Box<dyn Iterator<Item = &str> + '_> {
    Box::new(once(ORIGIN).chain(once(DESTINATION)))
  }

  fn run(&mut self) {}

  fn result(&self) -> Option<Rc<dyn Drawable>> {
    let mut geom = vec![];
    if let Some(origin) = self.origin {
      geom.push(Geometry::Point(
        origin,
        Metadata::default().with_style(Style::default().with_color(egui::Color32::GREEN)),
      ));
    }
    if let Some(destination) = self.destination {
      geom.push(Geometry::Point(
        destination,
        Metadata::default().with_style(Style::default().with_color(egui::Color32::RED)),
      ));
    }
    if let (Some(origin), Some(destination)) = (self.origin, self.destination) {
      let dist = distance_in_meters(origin.as_wgs84(), destination.as_wgs84());
      geom.push(Geometry::LineString(
        vec![origin, destination],
        Metadata::default().with_label(format!("Dist: {dist:.2}m")),
      ));
    }
    Some(Rc::new(Geometry::GeometryCollection(
      geom,
      Metadata::default(),
    )))
  }

  fn locked(&mut self) -> &mut bool {
    &mut self.locked
  }

  fn visible(&mut self) -> &mut bool {
    &mut self.visible
  }

  fn is_locked(&self) -> bool {
    self.locked
  }

  fn is_visible(&self) -> bool {
    self.visible
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    ui.label(
      self
        .origin
        .map(|o| o.as_wgs84())
        .map(|o| format!("Origin: {o:?}"))
        .unwrap_or_default(),
    );
    ui.label(
      self
        .destination
        .map(|o| o.as_wgs84())
        .map(|d| format!("Destination: {d:?}"))
        .unwrap_or_default(),
    );
    if let (Some(origin), Some(destination)) = (self.origin, self.destination) {
      let dist = distance_in_meters(origin.as_wgs84(), destination.as_wgs84());
      ui.label(format!("Distance: {dist:.2}m"));
    }
  }

  fn name(&self) -> &'static str {
    "ruler"
  }

  fn bounding_box(&self) -> crate::map::coordinates::BoundingBox {
    crate::map::coordinates::BoundingBox::from_iterator(
      self.origin.iter().chain(self.destination.iter()).copied(),
    )
  }
}
