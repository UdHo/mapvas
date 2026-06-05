use egui::Rect;

use super::{
  coordinates::{PixelCoordinate, Transform},
  geometry_collection::Geometry,
};

#[derive(Debug, Clone, Copy)]
pub struct MapViewport {
  pub transform: Transform,
  pub rect: Rect,
}

#[derive(Clone, Default)]
pub struct GeometrySnapshot {
  pub version: u64,
  pub geometries: Vec<Geometry<PixelCoordinate>>,
}
