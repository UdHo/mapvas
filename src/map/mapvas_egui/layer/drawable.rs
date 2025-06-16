use egui::{
  Stroke,
  epaint::{CircleShape, PathShape, PathStroke},
};

use crate::map::{
  coordinates::{BoundingBox, Coordinate, Transform},
  geometry_collection::{DEFAULT_STYLE, Geometry, Style},
};

type Painter = egui::Painter;

const DEFAULT_STROKE_WIDTH: f32 = 4.0;
const DEFAULT_POINT_RADIUS: f32 = 4.0;

/// An abstraction for anything that can be drawn on the map that is dependent on coordinates/the
/// transformation.
pub trait Drawable {
  fn draw(&self, painter: &Painter, transform: &Transform);
  fn bounding_box(&self) -> Option<BoundingBox> {
    None
  }
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
            center: transform.apply(coord.as_pixel_coordinate()).into(),
            radius: DEFAULT_POINT_RADIUS,
            fill: color,
            stroke: Stroke::new(0.0, color),
          })
        }
        Geometry::LineString(coord, metadata) => {
          let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
          egui::Shape::Path(PathShape {
            points: coord
              .iter()
              .map(|c| transform.apply(c.as_pixel_coordinate()).into())
              .collect(),
            closed: false,
            fill: style.fill_color(),
            stroke: PathStroke::new(DEFAULT_STROKE_WIDTH, style.color()),
          })
        }
        Geometry::Polygon(vec, metadata) => {
          let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
          egui::Shape::Path(PathShape {
            points: vec
              .iter()
              .map(|c| transform.apply(c.as_pixel_coordinate()).into())
              .collect(),
            closed: true,
            fill: style.fill_color(),
            stroke: PathStroke::new(DEFAULT_STROKE_WIDTH, style.color()),
          })
        }
      };
      painter.add(shape);
    }
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    Some(Geometry::bounding_box(self))
  }
}
