use crate::{
  config::HeadingStyle,
  map::{
    coordinates::{Coordinate, PixelCoordinate, Transform},
    geometry_collection::{DEFAULT_STYLE, Geometry, Style},
  },
};
use egui::{Color32, Rect};
use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, Transform as SkiaTransform};

const DEFAULT_STROKE_WIDTH: f32 = 4.0;
const DEFAULT_POINT_RADIUS: f32 = 4.0;

fn color32_to_skia(c: Color32) -> tiny_skia::Color {
  tiny_skia::Color::from_rgba8(c.r(), c.g(), c.b(), c.a())
}

/// Convert a `PixelCoordinate` to screen-relative position within the pixmap.
fn to_screen(coord: &impl Coordinate, transform: &Transform, rect: Rect) -> (f32, f32) {
  let pos = transform.apply(coord.as_pixel_coordinate());
  (pos.x - rect.min.x, pos.y - rect.min.y)
}

/// Rasterize all provided geometries into a `Pixmap` with transparent background.
///
/// Each geometry is drawn with its own style (color, fill). The result can be
/// loaded as an egui texture for cached rendering.
#[allow(clippy::module_name_repetitions)]
pub fn rasterize_geometries<'a>(
  geometries: impl Iterator<Item = &'a Geometry<PixelCoordinate>>,
  transform: &Transform,
  rect: Rect,
  heading_style: HeadingStyle,
) -> Option<Pixmap> {
  #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
  let w = rect.width() as u32;
  #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
  let h = rect.height() as u32;
  if w == 0 || h == 0 {
    return None;
  }
  let mut pixmap = Pixmap::new(w, h)?;
  // Pixmap starts fully transparent by default.

  for geometry in geometries {
    rasterize_geometry(&mut pixmap, geometry, transform, rect, heading_style);
  }

  Some(pixmap)
}

fn rasterize_geometry(
  pixmap: &mut Pixmap,
  geometry: &Geometry<PixelCoordinate>,
  transform: &Transform,
  rect: Rect,
  heading_style: HeadingStyle,
) {
  for el in geometry
    .flat_iterate_with_merged_style(&Style::default())
    .filter(Geometry::is_visible)
  {
    match el {
      Geometry::GeometryCollection(_, _) => {
        unreachable!("GeometryCollections should be flattened")
      }
      Geometry::Point(coord, metadata) => {
        let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
        let color = style.color().gamma_multiply(0.7);
        let (cx, cy) = to_screen(&coord, transform, rect);

        draw_circle(pixmap, cx, cy, DEFAULT_POINT_RADIUS, color, color);

        if let Some(heading) = metadata.heading {
          draw_heading(pixmap, cx, cy, heading, color, heading_style);
        }
      }
      Geometry::LineString(coords, metadata) => {
        let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
        let stroke_color = style.color().gamma_multiply(0.7);
        let points: Vec<(f32, f32)> = coords
          .iter()
          .map(|c| to_screen(c, transform, rect))
          .collect();
        draw_path(pixmap, &points, false, Color32::TRANSPARENT, stroke_color);
      }
      Geometry::Polygon(coords, metadata) => {
        let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
        let stroke_color = style.color().gamma_multiply(0.7);
        let fill_color = if style.fill_color() == Color32::TRANSPARENT {
          Color32::TRANSPARENT
        } else {
          style.fill_color().gamma_multiply(0.7)
        };
        let points: Vec<(f32, f32)> = coords
          .iter()
          .map(|c| to_screen(c, transform, rect))
          .collect();
        draw_path(pixmap, &points, true, fill_color, stroke_color);
      }
    }
  }
}

fn draw_circle(
  pixmap: &mut Pixmap,
  cx: f32,
  cy: f32,
  radius: f32,
  fill_color: Color32,
  stroke_color: Color32,
) {
  let mut pb = PathBuilder::new();
  pb.push_circle(cx, cy, radius);
  let Some(path) = pb.finish() else { return };

  // Fill
  if fill_color != Color32::TRANSPARENT {
    let mut paint = Paint::default();
    paint.set_color(color32_to_skia(fill_color));
    paint.anti_alias = true;
    pixmap.fill_path(
      &path,
      &paint,
      tiny_skia::FillRule::Winding,
      SkiaTransform::identity(),
      None,
    );
  }

  // Stroke
  let mut paint = Paint::default();
  paint.set_color(color32_to_skia(stroke_color));
  paint.anti_alias = true;
  let stroke = Stroke {
    width: DEFAULT_STROKE_WIDTH,
    ..Stroke::default()
  };
  pixmap.stroke_path(&path, &paint, &stroke, SkiaTransform::identity(), None);
}

fn draw_path(
  pixmap: &mut Pixmap,
  points: &[(f32, f32)],
  closed: bool,
  fill_color: Color32,
  stroke_color: Color32,
) {
  if points.len() < 2 {
    return;
  }

  let mut pb = PathBuilder::new();
  pb.move_to(points[0].0, points[0].1);
  for &(x, y) in &points[1..] {
    pb.line_to(x, y);
  }
  if closed {
    pb.close();
  }
  let Some(path) = pb.finish() else { return };

  // Fill (for closed polygons)
  if closed && fill_color != Color32::TRANSPARENT {
    let mut paint = Paint::default();
    paint.set_color(color32_to_skia(fill_color));
    paint.anti_alias = true;
    pixmap.fill_path(
      &path,
      &paint,
      tiny_skia::FillRule::EvenOdd,
      SkiaTransform::identity(),
      None,
    );
  }

  // Stroke
  let mut paint = Paint::default();
  paint.set_color(color32_to_skia(stroke_color));
  paint.anti_alias = true;
  let stroke = Stroke {
    width: DEFAULT_STROKE_WIDTH,
    ..Stroke::default()
  };
  pixmap.stroke_path(&path, &paint, &stroke, SkiaTransform::identity(), None);
}

fn draw_heading(
  pixmap: &mut Pixmap,
  cx: f32,
  cy: f32,
  heading_degrees: f32,
  color: Color32,
  style: HeadingStyle,
) {
  let heading_rad = (heading_degrees - 90.0).to_radians();

  match style {
    HeadingStyle::Arrow => draw_heading_arrow(pixmap, cx, cy, heading_rad, color),
    HeadingStyle::Line => draw_heading_line(pixmap, cx, cy, heading_rad, color),
    HeadingStyle::Chevron => draw_heading_chevron(pixmap, cx, cy, heading_rad, color),
    HeadingStyle::Needle => draw_heading_needle(pixmap, cx, cy, heading_rad, color),
    HeadingStyle::Sector => draw_heading_sector(pixmap, cx, cy, heading_rad, color),
    HeadingStyle::Rectangle => draw_heading_rectangle(pixmap, cx, cy, heading_rad, color),
  }
}

fn draw_heading_arrow(pixmap: &mut Pixmap, cx: f32, cy: f32, heading_rad: f32, color: Color32) {
  let arrow_length = DEFAULT_POINT_RADIUS + 8.0;
  let arrow_width = 4.0;

  let tip_x = cx + arrow_length * heading_rad.cos();
  let tip_y = cy + arrow_length * heading_rad.sin();

  let base_offset = arrow_width / 2.0;
  let perp = heading_rad + std::f32::consts::PI / 2.0;

  let points = vec![
    (tip_x, tip_y),
    (cx + base_offset * perp.cos(), cy + base_offset * perp.sin()),
    (cx - base_offset * perp.cos(), cy - base_offset * perp.sin()),
  ];
  draw_path(pixmap, &points, true, color, color);
}

fn draw_heading_line(pixmap: &mut Pixmap, cx: f32, cy: f32, heading_rad: f32, color: Color32) {
  let length = DEFAULT_POINT_RADIUS + 6.0;
  let end = (cx + length * heading_rad.cos(), cy + length * heading_rad.sin());

  let mut pb = PathBuilder::new();
  pb.move_to(cx, cy);
  pb.line_to(end.0, end.1);
  if let Some(path) = pb.finish() {
    let mut paint = Paint::default();
    paint.set_color(color32_to_skia(color));
    paint.anti_alias = true;
    let stroke = Stroke {
      width: 2.0,
      ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, SkiaTransform::identity(), None);
  }
}

fn draw_heading_chevron(pixmap: &mut Pixmap, cx: f32, cy: f32, heading_rad: f32, color: Color32) {
  let length = DEFAULT_POINT_RADIUS + 4.0;
  let angle_offset = 0.5;

  let tip_x = cx + length * heading_rad.cos();
  let tip_y = cy + length * heading_rad.sin();

  let left_angle = heading_rad - angle_offset;
  let right_angle = heading_rad + angle_offset;
  let back_length = length * 0.6;

  let points = vec![
    (
      tip_x - back_length * left_angle.cos(),
      tip_y - back_length * left_angle.sin(),
    ),
    (tip_x, tip_y),
    (
      tip_x - back_length * right_angle.cos(),
      tip_y - back_length * right_angle.sin(),
    ),
  ];
  draw_path(pixmap, &points, false, Color32::TRANSPARENT, color);
}

fn draw_heading_needle(pixmap: &mut Pixmap, cx: f32, cy: f32, heading_rad: f32, color: Color32) {
  let length = DEFAULT_POINT_RADIUS + 8.0;
  let head_size = 2.0;

  let tip_x = cx + length * heading_rad.cos();
  let tip_y = cy + length * heading_rad.sin();

  let perp = heading_rad + std::f32::consts::PI / 2.0;
  let head_left = (
    tip_x - head_size * heading_rad.cos() + head_size * 0.5 * perp.cos(),
    tip_y - head_size * heading_rad.sin() + head_size * 0.5 * perp.sin(),
  );
  let head_right = (
    tip_x - head_size * heading_rad.cos() - head_size * 0.5 * perp.cos(),
    tip_y - head_size * heading_rad.sin() - head_size * 0.5 * perp.sin(),
  );

  let points = vec![
    (cx, cy),
    (tip_x, tip_y),
    head_left,
    (tip_x, tip_y),
    head_right,
  ];
  draw_path(pixmap, &points, false, Color32::TRANSPARENT, color);
}

#[allow(clippy::cast_precision_loss)]
fn draw_heading_sector(pixmap: &mut Pixmap, cx: f32, cy: f32, heading_rad: f32, color: Color32) {
  let radius = DEFAULT_POINT_RADIUS + 4.0;
  let sector_angle = 0.6;

  let start_angle = heading_rad - sector_angle / 2.0;
  let end_angle = heading_rad + sector_angle / 2.0;

  let mut points = vec![(cx, cy)];
  for i in 0..=8 {
    let angle = start_angle + (end_angle - start_angle) * i as f32 / 8.0;
    points.push((cx + radius * angle.cos(), cy + radius * angle.sin()));
  }

  #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
  let fill_alpha = (f32::from(color.a()) * 0.3 / 0.7) as u8;
  let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), fill_alpha);
  draw_path(pixmap, &points, true, fill, color);
}

fn draw_heading_rectangle(
  pixmap: &mut Pixmap,
  cx: f32,
  cy: f32,
  heading_rad: f32,
  color: Color32,
) {
  let length = DEFAULT_POINT_RADIUS + 6.0;
  let width = 3.0;

  let (fw_x, fw_y) = (heading_rad.cos(), heading_rad.sin());
  let (rt_x, rt_y) = (-heading_rad.sin(), heading_rad.cos());

  let hl = length / 2.0;
  let hw = width / 2.0;

  let points = vec![
    (cx + fw_x * hl + rt_x * hw, cy + fw_y * hl + rt_y * hw),
    (cx + fw_x * hl - rt_x * hw, cy + fw_y * hl - rt_y * hw),
    (cx - fw_x * hl - rt_x * hw, cy - fw_y * hl - rt_y * hw),
    (cx - fw_x * hl + rt_x * hw, cy - fw_y * hl + rt_y * hw),
  ];
  draw_path(pixmap, &points, true, color, color);
}
