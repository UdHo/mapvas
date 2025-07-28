use egui::{
  Shape, Stroke,
  epaint::{CircleShape, PathShape, PathStroke},
};

use crate::map::{
  coordinates::{BoundingBox, Coordinate, PixelCoordinate, Transform},
  distance,
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
  /// Get the underlying geometry if this drawable is a geometry
  fn as_geometry(&self) -> Option<&Geometry<PixelCoordinate>> {
    None
  }
  /// Calculate distance from this drawable to a point
  fn distance_to_point(&self, point: PixelCoordinate) -> Option<f64> {
    if let Some(geometry) = self.as_geometry() {
      distance::distance_to_geometry(geometry, point)
    } else if let Some(bbox) = self.bounding_box() {
      if bbox.is_valid() {
        let center = bbox.center();
        let dx = center.x - point.x;
        let dy = center.y - point.y;
        Some(f64::from(dx * dx + dy * dy).sqrt())
      } else {
        None
      }
    } else {
      None
    }
  }
}

impl Drawable for Shape {
  fn draw(&self, painter: &Painter, _transform: &Transform) {
    painter.add(self.clone());
  }
}

impl<C: Coordinate + 'static> Drawable for Geometry<C> {
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
          Shape::Circle(CircleShape {
            center: transform.apply(coord.as_pixel_coordinate()).into(),
            radius: DEFAULT_POINT_RADIUS,
            fill: color,
            stroke: Stroke::new(0.0, color),
          })
        }
        Geometry::LineString(coord, metadata) => {
          let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
          Shape::Path(PathShape {
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
          Shape::Path(PathShape {
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

  fn as_geometry(&self) -> Option<&Geometry<PixelCoordinate>> {
    None
  }

  fn distance_to_point(&self, point: PixelCoordinate) -> Option<f64> {
    let converted = self.convert_to_pixel_coordinates();
    distance::distance_to_geometry(&converted, point)
  }
}

impl<C: Coordinate> Geometry<C> {
  fn convert_to_pixel_coordinates(&self) -> Geometry<PixelCoordinate> {
    match self {
      Geometry::Point(coord, metadata) => {
        Geometry::Point(coord.as_pixel_coordinate(), metadata.clone())
      }
      Geometry::LineString(coords, metadata) => Geometry::LineString(
        coords.iter().map(Coordinate::as_pixel_coordinate).collect(),
        metadata.clone(),
      ),
      Geometry::Polygon(coords, metadata) => Geometry::Polygon(
        coords.iter().map(Coordinate::as_pixel_coordinate).collect(),
        metadata.clone(),
      ),
      Geometry::GeometryCollection(geometries, metadata) => Geometry::GeometryCollection(
        geometries
          .iter()
          .map(Geometry::convert_to_pixel_coordinates)
          .collect(),
        metadata.clone(),
      ),
    }
  }
}
