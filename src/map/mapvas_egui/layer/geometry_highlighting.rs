use crate::map::{
  coordinates::{PixelCoordinate, Transform},
  geometry_collection::{DEFAULT_STYLE, Geometry, Metadata},
};
use egui::{
  Color32, ColorImage, Painter, Pos2, Rect, Shape, Stroke,
  epaint::{CircleShape, PathShape, PathStroke},
};
use std::collections::HashMap;
use tiny_skia::{
  FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke as SkiaStroke,
  Transform as SkiaTransform,
};

/// Bold highlight stroke width, in screen pixels.
const HIGHLIGHT_STROKE_WIDTH: f32 = 6.0;

/// Manages geometry highlighting with unique IDs
pub struct GeometryHighlighter {
  highlighted_geometry_id: Option<u64>,
  next_geometry_id: u64,
  geometry_id_map: HashMap<(String, usize, Vec<usize>), u64>,
  just_highlighted: bool,
}

impl GeometryHighlighter {
  pub fn new() -> Self {
    Self {
      highlighted_geometry_id: None,
      next_geometry_id: 1,
      geometry_id_map: HashMap::new(),
      just_highlighted: false,
    }
  }

  /// Check if a geometry is currently highlighted
  pub fn is_highlighted(&self, layer_id: &str, shape_idx: usize, nested_path: &[usize]) -> bool {
    let geometry_key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
    if let Some(geometry_id) = self.geometry_id_map.get(&geometry_key) {
      self.highlighted_geometry_id == Some(*geometry_id)
    } else {
      false
    }
  }

  /// Highlight a geometry by its path
  pub fn highlight_geometry(&mut self, layer_id: &str, shape_idx: usize, nested_path: &[usize]) {
    let geometry_id = self.get_or_create_geometry_id(layer_id, shape_idx, nested_path);
    self.highlight_geometry_by_id(geometry_id);
  }

  /// Highlight a geometry by its unique ID
  pub fn highlight_geometry_by_id(&mut self, geometry_id: u64) {
    self.highlighted_geometry_id = Some(geometry_id);
    self.just_highlighted = true;
  }

  /// Clear highlighting
  pub fn clear_highlighting(&mut self) {
    self.highlighted_geometry_id = None;
    self.just_highlighted = false;
  }

  /// Check if any geometry is highlighted
  pub fn has_highlighted_geometry(&self) -> bool {
    self.highlighted_geometry_id.is_some()
  }

  /// Check if highlighting was just set (for pagination updates)
  pub fn was_just_highlighted(&mut self) -> bool {
    let result = self.just_highlighted;
    self.just_highlighted = false;
    result
  }

  /// Get the currently highlighted geometry if any
  pub fn get_highlighted_geometry(&self) -> Option<(String, usize, Vec<usize>)> {
    if let Some(highlighted_id) = self.highlighted_geometry_id {
      // Find the geometry path that matches the highlighted ID
      for ((layer_id, shape_idx, nested_path), geometry_id) in &self.geometry_id_map {
        if *geometry_id == highlighted_id {
          return Some((layer_id.clone(), *shape_idx, nested_path.clone()));
        }
      }
    }
    None
  }

  /// Generate a unique ID for a geometry
  fn generate_geometry_id(&mut self) -> u64 {
    let id = self.next_geometry_id;
    self.next_geometry_id += 1;
    id
  }

  /// Get or create a unique ID for a geometry at the given path
  fn get_or_create_geometry_id(
    &mut self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
  ) -> u64 {
    let key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
    if let Some(&existing_id) = self.geometry_id_map.get(&key) {
      existing_id
    } else {
      let new_id = self.generate_geometry_id();
      self.geometry_id_map.insert(key.clone(), new_id);
      new_id
    }
  }
}

/// Draw highlighted geometry with solid colors
pub fn draw_highlighted_geometry(
  geometry: &Geometry<PixelCoordinate>,
  painter: &Painter,
  transform: &Transform,
  _highlight_all: bool,
) {
  match geometry {
    Geometry::Point(coord, metadata) => {
      draw_highlighted_point(*coord, metadata, painter, transform);
    }
    Geometry::LineString(coords, metadata) => {
      draw_highlighted_linestring(coords, metadata, painter, transform);
    }
    Geometry::Polygon(_, _) | Geometry::Heatmap(_, _) => {
      // Polygons render via tiny-skia (`rasterize_highlighted_polygons`);
      // heatmaps are not individually highlightable.
    }
    Geometry::GeometryCollection(geometries, _) => {
      for nested_geometry in geometries {
        draw_highlighted_geometry(nested_geometry, painter, transform, false);
      }
    }
  }
}

/// Draw a highlighted point
fn draw_highlighted_point(
  coord: PixelCoordinate,
  metadata: &Metadata,
  painter: &Painter,
  transform: &Transform,
) {
  let center = transform.apply(coord).into();
  let base_color = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE).color();

  // Draw highlighted point filled with original color
  let highlight_fill = base_color;
  let highlight_stroke = base_color;

  let circle_shape = Shape::Circle(CircleShape {
    center,
    radius: 10.0,
    fill: highlight_fill,
    stroke: Stroke::new(2.0, highlight_stroke),
  });
  painter.add(circle_shape);
}

/// Draw a highlighted linestring
fn draw_highlighted_linestring(
  coords: &[PixelCoordinate],
  metadata: &Metadata,
  painter: &Painter,
  transform: &Transform,
) {
  let base_color = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE).color();

  // Draw highlighted linestring as solid thick line
  let shape = Shape::Path(PathShape {
    points: coords.iter().map(|c| transform.apply(*c).into()).collect(),
    closed: false,
    fill: Color32::TRANSPARENT,
    stroke: PathStroke::new(6.0, base_color), // Thicker and solid
  });
  painter.add(shape);
}

/// Rasterize all polygons inside `geometry` (recursively) into a pixmap
/// clipped to `viewport`, drawing each polygon's fill and bold solid stroke
/// via tiny-skia. egui can't draw these correctly: its fan-triangulator
/// produces fold-overs on concave/bridged polygons, and its `PathStroke` has
/// no `LineJoin` control so sharp angles produce miter spikes. tiny-skia
/// uses winding-rule fill and `LineJoin::Round`, so neither happens.
pub fn rasterize_highlighted_polygons(
  geometry: &Geometry<PixelCoordinate>,
  transform: &Transform,
  viewport: Rect,
) -> Option<(ColorImage, Rect)> {
  let mut bbox: Option<Rect> = None;
  collect_polygon_bbox(geometry, transform, &mut bbox);
  // Pad for the stroke half-width plus a feathering margin.
  let pad = HIGHLIGHT_STROKE_WIDTH * 0.5 + 1.0;
  let bbox = bbox?.expand(pad);
  let clipped = bbox.intersect(viewport);
  if clipped.width() <= 0.0 || clipped.height() <= 0.0 {
    return None;
  }
  #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
  let w = clipped.width().ceil() as u32;
  #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
  let h = clipped.height().ceil() as u32;
  if w == 0 || h == 0 {
    return None;
  }
  let mut pixmap = Pixmap::new(w, h)?;
  fill_polygons_into_pixmap(geometry, transform, &mut pixmap, clipped.min);

  // tiny-skia stores premultiplied RGBA; un-premultiply for ColorImage.
  let mut straight = Vec::with_capacity(pixmap.data().len());
  for p in pixmap.data().chunks_exact(4) {
    let a = p[3];
    if a == 0 {
      straight.extend_from_slice(&[0, 0, 0, 0]);
    } else {
      let inv = 255.0_f32 / f32::from(a);
      #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
      {
        straight.push((f32::from(p[0]) * inv).min(255.0) as u8);
        straight.push((f32::from(p[1]) * inv).min(255.0) as u8);
        straight.push((f32::from(p[2]) * inv).min(255.0) as u8);
      }
      straight.push(a);
    }
  }
  Some((
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &straight),
    clipped,
  ))
}

fn collect_polygon_bbox(
  geometry: &Geometry<PixelCoordinate>,
  transform: &Transform,
  acc: &mut Option<Rect>,
) {
  match geometry {
    Geometry::Polygon(coords, _) => {
      for c in coords {
        let p: Pos2 = transform.apply(*c).into();
        let r = Rect::from_min_max(p, p);
        *acc = Some(acc.map_or(r, |a| a.union(r)));
      }
    }
    Geometry::GeometryCollection(geoms, _) => {
      for g in geoms {
        collect_polygon_bbox(g, transform, acc);
      }
    }
    _ => {}
  }
}

fn fill_polygons_into_pixmap(
  geometry: &Geometry<PixelCoordinate>,
  transform: &Transform,
  pixmap: &mut Pixmap,
  origin: Pos2,
) {
  match geometry {
    Geometry::Polygon(coords, metadata) => {
      let style = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE);
      let color = style.color();
      let mut iter = coords.iter();
      let Some(first) = iter.next() else { return };
      let mut pb = PathBuilder::new();
      let p: Pos2 = transform.apply(*first).into();
      pb.move_to(p.x - origin.x, p.y - origin.y);
      for c in iter {
        let p: Pos2 = transform.apply(*c).into();
        pb.line_to(p.x - origin.x, p.y - origin.y);
      }
      pb.close();
      let Some(path) = pb.finish() else { return };
      let mut paint = Paint::default();
      paint.set_color(tiny_skia::Color::from_rgba8(
        color.r(),
        color.g(),
        color.b(),
        color.a(),
      ));
      paint.anti_alias = true;
      pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        SkiaTransform::identity(),
        None,
      );
      // Bold solid stroke with round joins to suppress miter spikes on
      // bridged-multipolygon path reversals.
      let stroke = SkiaStroke {
        width: HIGHLIGHT_STROKE_WIDTH,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..SkiaStroke::default()
      };
      pixmap.stroke_path(&path, &paint, &stroke, SkiaTransform::identity(), None);
    }
    Geometry::GeometryCollection(geoms, _) => {
      for g in geoms {
        fill_polygons_into_pixmap(g, transform, pixmap, origin);
      }
    }
    _ => {}
  }
}
