use egui::{
  Shape, Stroke,
  epaint::{CircleShape, PathShape, PathStroke},
};

use crate::{
  config::HeadingStyle,
  map::{
    coordinates::{BoundingBox, Coordinate, PixelCoordinate, Transform},
    distance,
    geometry_collection::{DEFAULT_STYLE, Geometry, Style},
  },
};

type Painter = egui::Painter;

const DEFAULT_STROKE_WIDTH: f32 = 4.0;
const DEFAULT_POINT_RADIUS: f32 = 4.0;

/// An abstraction for anything that can be drawn on the map that is dependent on coordinates/the
/// transformation.
pub trait Drawable {
  fn draw(&self, painter: &Painter, transform: &Transform);
  fn draw_with_style(&self, painter: &Painter, transform: &Transform, _heading_style: HeadingStyle) {
    // Default implementation falls back to the old draw method
    self.draw(painter, transform);
  }
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
    self.draw_with_style(painter, transform, HeadingStyle::default());
  }

  fn draw_with_style(&self, painter: &Painter, transform: &Transform, heading_style: HeadingStyle) {
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
          let center = transform.apply(coord.as_pixel_coordinate()).into();

          // Draw the point as a circle
          let circle_shape = Shape::Circle(CircleShape {
            center,
            radius: DEFAULT_POINT_RADIUS,
            fill: color,
            stroke: Stroke::new(0.0, color),
          });
          painter.add(circle_shape);

          // Draw heading arrow if present
          if let Some(heading) = metadata.heading {
            let heading_shape = create_heading_arrow(center, heading, color, heading_style);
            painter.add(heading_shape);
          }

          // Return a no-op shape since we've already drawn directly to painter
          Shape::Noop
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

/// Create a heading arrow shape pointing in the specified direction
fn create_heading_arrow(
  center: egui::Pos2,
  heading_degrees: f32,
  color: egui::Color32,
  style: HeadingStyle,
) -> Shape {
  // Convert heading from degrees to radians (0° = North, clockwise)
  let heading_rad = (heading_degrees - 90.0).to_radians(); // Adjust so 0° points up

  match style {
    HeadingStyle::Arrow => create_arrow_shape(center, heading_rad, color),
    HeadingStyle::Line => create_line_shape(center, heading_rad, color),
    HeadingStyle::Chevron => create_chevron_shape(center, heading_rad, color),
    HeadingStyle::Needle => create_needle_shape(center, heading_rad, color),
    HeadingStyle::Sector => create_sector_shape(center, heading_rad, color),
    HeadingStyle::Rectangle => create_rectangle_shape(center, heading_rad, color),
  }
}

/// Create traditional arrow shape (filled triangle)
fn create_arrow_shape(center: egui::Pos2, heading_rad: f32, color: egui::Color32) -> Shape {
  let arrow_length = DEFAULT_POINT_RADIUS + 8.0;
  let arrow_width = 4.0;

  let tip_x = center.x + arrow_length * heading_rad.cos();
  let tip_y = center.y + arrow_length * heading_rad.sin();
  let tip = egui::Pos2::new(tip_x, tip_y);

  let base_offset = arrow_width / 2.0;
  let perp_angle = heading_rad + std::f32::consts::PI / 2.0;

  let base_left_x = center.x + base_offset * perp_angle.cos();
  let base_left_y = center.y + base_offset * perp_angle.sin();
  let base_left = egui::Pos2::new(base_left_x, base_left_y);

  let base_right_x = center.x - base_offset * perp_angle.cos();
  let base_right_y = center.y - base_offset * perp_angle.sin();
  let base_right = egui::Pos2::new(base_right_x, base_right_y);

  Shape::Path(PathShape {
    points: vec![tip, base_left, base_right],
    closed: true,
    fill: color,
    stroke: PathStroke::new(1.0, color),
  })
}

/// Create simple line shape
fn create_line_shape(center: egui::Pos2, heading_rad: f32, color: egui::Color32) -> Shape {
  let length = DEFAULT_POINT_RADIUS + 6.0;

  let end_x = center.x + length * heading_rad.cos();
  let end_y = center.y + length * heading_rad.sin();
  let end = egui::Pos2::new(end_x, end_y);

  Shape::LineSegment {
    points: [center, end],
    stroke: Stroke::new(2.0, color),
  }
}

/// Create chevron/V-shape
fn create_chevron_shape(center: egui::Pos2, heading_rad: f32, color: egui::Color32) -> Shape {
  let length = DEFAULT_POINT_RADIUS + 4.0;
  let angle_offset = 0.5; // ~30 degrees

  let tip_x = center.x + length * heading_rad.cos();
  let tip_y = center.y + length * heading_rad.sin();
  let tip = egui::Pos2::new(tip_x, tip_y);

  let left_angle = heading_rad - angle_offset;
  let right_angle = heading_rad + angle_offset;
  let back_length = length * 0.6;

  let left_x = tip.x - back_length * left_angle.cos();
  let left_y = tip.y - back_length * left_angle.sin();
  let left = egui::Pos2::new(left_x, left_y);

  let right_x = tip.x - back_length * right_angle.cos();
  let right_y = tip.y - back_length * right_angle.sin();
  let right = egui::Pos2::new(right_x, right_y);

  Shape::Path(PathShape {
    points: vec![left, tip, right],
    closed: false,
    fill: egui::Color32::TRANSPARENT,
    stroke: PathStroke::new(2.0, color),
  })
}

/// Create needle/compass style
fn create_needle_shape(center: egui::Pos2, heading_rad: f32, color: egui::Color32) -> Shape {
  let length = DEFAULT_POINT_RADIUS + 8.0;
  let head_size = 2.0;

  let tip_x = center.x + length * heading_rad.cos();
  let tip_y = center.y + length * heading_rad.sin();
  let tip = egui::Pos2::new(tip_x, tip_y);

  let perpendicular = heading_rad + std::f32::consts::PI / 2.0;
  let head_left = egui::Pos2::new(
    tip.x - head_size * heading_rad.cos() + head_size * 0.5 * perpendicular.cos(),
    tip.y - head_size * heading_rad.sin() + head_size * 0.5 * perpendicular.sin(),
  );
  let head_right = egui::Pos2::new(
    tip.x - head_size * heading_rad.cos() - head_size * 0.5 * perpendicular.cos(),
    tip.y - head_size * heading_rad.sin() - head_size * 0.5 * perpendicular.sin(),
  );

  Shape::Path(PathShape {
    points: vec![center, tip, head_left, tip, head_right],
    closed: false,
    fill: egui::Color32::TRANSPARENT,
    stroke: PathStroke::new(1.5, color),
  })
}

/// Create sector/pie slice shape
fn create_sector_shape(center: egui::Pos2, heading_rad: f32, color: egui::Color32) -> Shape {
  let radius = DEFAULT_POINT_RADIUS + 4.0;
  let sector_angle = 0.6; // ~35 degrees total width

  let start_angle = heading_rad - sector_angle / 2.0;
  let end_angle = heading_rad + sector_angle / 2.0;

  let mut points = vec![center];

  // Create arc points
  for i in 0..=8 {
    #[allow(clippy::cast_precision_loss)]
    let angle = start_angle + (end_angle - start_angle) * i as f32 / 8.0;
    let x = center.x + radius * angle.cos();
    let y = center.y + radius * angle.sin();
    points.push(egui::Pos2::new(x, y));
  }

  Shape::Path(PathShape {
    points,
    closed: true,
    fill: color.gamma_multiply(0.3),
    stroke: PathStroke::new(1.0, color),
  })
}

/// Create oriented rectangle shape
fn create_rectangle_shape(center: egui::Pos2, heading_rad: f32, color: egui::Color32) -> Shape {
  let length = DEFAULT_POINT_RADIUS + 6.0;
  let width = 3.0;

  let forward = egui::Vec2::new(heading_rad.cos(), heading_rad.sin());
  let right = egui::Vec2::new(-heading_rad.sin(), heading_rad.cos());

  let half_length = length / 2.0;
  let half_width = width / 2.0;

  let corners = [
    center + forward * half_length + right * half_width,
    center + forward * half_length - right * half_width,
    center - forward * half_length - right * half_width,
    center - forward * half_length + right * half_width,
  ];

  Shape::Path(PathShape {
    points: corners.to_vec(),
    closed: true,
    fill: color,
    stroke: PathStroke::new(1.0, color),
  })
}
