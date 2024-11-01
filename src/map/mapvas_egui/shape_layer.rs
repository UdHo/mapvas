use std::collections::HashMap;

use egui::{Rect, Ui};

use crate::map::{
  coordinates::{PixelPosition, Transform},
  map_event::Style,
};

use super::Layer;

struct Shape {
  coords: Vec<PixelPosition>,
  style: Style,
  text: Option<String>,
}

struct ShapeLayer {
  shape_map: HashMap<String, Vec<Shape>>,
}

impl Layer for ShapeLayer {
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, rect: Rect) {}
}
