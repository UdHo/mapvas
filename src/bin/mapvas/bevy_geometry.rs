use bevy::{prelude::*, window::PrimaryWindow};
use mapvas::map::{
  coordinates::{CANVAS_SIZE, PixelCoordinate},
  geometry_collection::{DEFAULT_STYLE, Geometry, Style},
  mapvas_egui::{GeometrySnapshot, MapViewport},
};

use crate::bevy_tiles::NativeMapViewport;

const POINT_RADIUS: f32 = 4.0;
const HEADING_LENGTH: f32 = 16.0;
const MAX_HEATMAP_POINTS_PER_FRAME: usize = 20_000;

pub struct NativeGeometryPlugin;

impl Plugin for NativeGeometryPlugin {
  fn build(&self, app: &mut App) {
    app.add_systems(Update, draw_native_geometry);
  }
}

#[derive(Clone, Copy, Default)]
struct GeometryStats {
  geometries: usize,
  points: usize,
  line_segments: usize,
  polygons: usize,
  heatmap_points: usize,
}

#[derive(Resource)]
pub struct NativeGeometryLayer {
  enabled: bool,
  replace_egui_geometry: bool,
  snapshot_version: u64,
  geometries: Vec<Geometry<PixelCoordinate>>,
  snapshot_stats: GeometryStats,
  draw_stats: GeometryStats,
}

impl Default for NativeGeometryLayer {
  fn default() -> Self {
    Self {
      enabled: false,
      replace_egui_geometry: true,
      snapshot_version: 0,
      geometries: Vec::new(),
      snapshot_stats: GeometryStats::default(),
      draw_stats: GeometryStats::default(),
    }
  }
}

impl NativeGeometryLayer {
  pub fn ui(&mut self, ui: &mut egui::Ui) {
    ui.collapsing("Native Geometry Preview", |ui| {
      ui.checkbox(&mut self.enabled, "visible");
      ui.checkbox(&mut self.replace_egui_geometry, "replace egui geometry");

      ui.separator();
      ui.label("Snapshot:");
      stat_row(ui, "Geometries:", self.snapshot_stats.geometries);
      stat_row(ui, "Points:", self.snapshot_stats.points);
      stat_row(ui, "Line segments:", self.snapshot_stats.line_segments);
      stat_row(ui, "Polygons:", self.snapshot_stats.polygons);
      stat_row(ui, "Heatmap points:", self.snapshot_stats.heatmap_points);

      ui.separator();
      ui.label("Drawn this frame:");
      stat_row(ui, "Geometries:", self.draw_stats.geometries);
      stat_row(ui, "Points:", self.draw_stats.points);
      stat_row(ui, "Line segments:", self.draw_stats.line_segments);
      stat_row(ui, "Polygons:", self.draw_stats.polygons);
      stat_row(ui, "Heatmap points:", self.draw_stats.heatmap_points);
    });
  }

  pub fn update_snapshot(&mut self, snapshot: GeometrySnapshot) {
    if self.snapshot_version == snapshot.version {
      return;
    }

    self.snapshot_version = snapshot.version;
    self.snapshot_stats = snapshot_stats(&snapshot.geometries);
    self.geometries = snapshot.geometries;
  }

  #[must_use]
  pub fn needs_snapshot(&self, version: u64) -> bool {
    self.snapshot_version != version
  }

  #[must_use]
  pub fn replaces_egui_geometry(&self) -> bool {
    self.enabled && self.replace_egui_geometry
  }
}

fn draw_native_geometry(
  mut layer: ResMut<NativeGeometryLayer>,
  viewport: Res<NativeMapViewport>,
  windows: Query<&Window, With<PrimaryWindow>>,
  mut gizmos: Gizmos,
) {
  let mut draw_stats = GeometryStats::default();
  if !layer.enabled {
    layer.draw_stats = draw_stats;
    return;
  }
  let Some(viewport) = viewport.get() else {
    layer.draw_stats = draw_stats;
    return;
  };
  let Ok(window) = windows.single() else {
    layer.draw_stats = draw_stats;
    return;
  };

  let left_x = viewport_left_x(viewport);
  for geometry in &layer.geometries {
    draw_geometry(
      geometry,
      viewport,
      window,
      left_x,
      &mut gizmos,
      &mut draw_stats,
    );
  }
  layer.draw_stats = draw_stats;
}

fn draw_geometry(
  geometry: &Geometry<PixelCoordinate>,
  viewport: MapViewport,
  window: &Window,
  left_x: f32,
  gizmos: &mut Gizmos,
  stats: &mut GeometryStats,
) {
  if !geometry_intersects_viewport(geometry, viewport, left_x) {
    return;
  }

  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        draw_geometry(geometry, viewport, window, left_x, gizmos, stats);
      }
    }
    Geometry::Point(coord, metadata) => {
      let Some(screen) = coordinate_to_screen(*coord, viewport, left_x) else {
        return;
      };
      if !viewport.rect.contains(screen) {
        return;
      }

      let color = style_color(metadata.style.as_ref());
      let center = screen_to_bevy(screen, window);
      gizmos.circle_2d(center, POINT_RADIUS, color);
      if let Some(heading) = metadata.heading {
        draw_heading(gizmos, center, heading, color);
      }

      stats.geometries += 1;
      stats.points += 1;
    }
    Geometry::LineString(coords, metadata) => {
      let color = style_color(metadata.style.as_ref());
      stats.geometries += 1;
      stats.line_segments += draw_lines(coords, false, viewport, window, left_x, color, gizmos);
    }
    Geometry::Polygon(coords, metadata) => {
      let color = style_color(metadata.style.as_ref());
      stats.geometries += 1;
      stats.polygons += 1;
      stats.line_segments += draw_lines(coords, true, viewport, window, left_x, color, gizmos);
    }
    Geometry::Heatmap(coords, metadata) => {
      let color = style_color(metadata.style.as_ref());
      stats.geometries += 1;
      for coord in coords.iter().take(MAX_HEATMAP_POINTS_PER_FRAME) {
        let Some(screen) = coordinate_to_screen(*coord, viewport, left_x) else {
          continue;
        };
        if !viewport.rect.contains(screen) {
          continue;
        }
        gizmos.circle_2d(screen_to_bevy(screen, window), 1.5, color);
        stats.heatmap_points += 1;
      }
    }
  }
}

fn draw_lines(
  coords: &[PixelCoordinate],
  closed: bool,
  viewport: MapViewport,
  window: &Window,
  left_x: f32,
  color: Color,
  gizmos: &mut Gizmos,
) -> usize {
  if coords.len() < 2 {
    return 0;
  }

  let mut drawn_segments = 0;
  for segment in coords.windows(2) {
    if draw_segment(
      segment[0], segment[1], viewport, window, left_x, color, gizmos,
    ) {
      drawn_segments += 1;
    }
  }

  if closed
    && let (Some(first), Some(last)) = (coords.first(), coords.last())
    && draw_segment(*last, *first, viewport, window, left_x, color, gizmos)
  {
    drawn_segments += 1;
  }

  drawn_segments
}

fn draw_segment(
  start: PixelCoordinate,
  end: PixelCoordinate,
  viewport: MapViewport,
  window: &Window,
  left_x: f32,
  color: Color,
  gizmos: &mut Gizmos,
) -> bool {
  let Some(screen_start) = coordinate_to_screen(start, viewport, left_x) else {
    return false;
  };
  let Some(screen_end) = coordinate_to_screen(end, viewport, left_x) else {
    return false;
  };
  if !segment_intersects_rect(screen_start, screen_end, viewport.rect) {
    return false;
  }

  gizmos.line_2d(
    screen_to_bevy(screen_start, window),
    screen_to_bevy(screen_end, window),
    color,
  );
  true
}

fn draw_heading(gizmos: &mut Gizmos, center: Vec2, heading_degrees: f32, color: Color) {
  let heading_rad = (heading_degrees - 90.0).to_radians();
  let direction = Vec2::new(heading_rad.cos(), -heading_rad.sin());
  gizmos.line_2d(center, center + direction * HEADING_LENGTH, color);
}

fn coordinate_to_screen(
  coord: PixelCoordinate,
  viewport: MapViewport,
  left_x: f32,
) -> Option<egui::Pos2> {
  if !coord.is_valid() {
    return None;
  }
  let offset = world_offset(coord.x, left_x);
  let shifted = PixelCoordinate {
    x: coord.x + offset,
    y: coord.y,
  };
  Some(viewport.transform.apply(shifted).into())
}

fn screen_to_bevy(screen: egui::Pos2, window: &Window) -> Vec2 {
  Vec2::new(
    screen.x - window.width() / 2.0,
    window.height() / 2.0 - screen.y,
  )
}

fn geometry_intersects_viewport(
  geometry: &Geometry<PixelCoordinate>,
  viewport: MapViewport,
  left_x: f32,
) -> bool {
  let bbox = geometry.bounding_box();
  if !bbox.is_valid() {
    return false;
  }

  let inv = viewport.transform.invert();
  let min_world = inv.apply(viewport.rect.min.into());
  let max_world = inv.apply(viewport.rect.max.into());
  let min_y = min_world.y.min(max_world.y);
  let max_y = min_world.y.max(max_world.y);
  if bbox.max_y() < min_y || bbox.min_y() > max_y {
    return false;
  }

  let min_x = min_world.x.min(max_world.x);
  let max_x = min_world.x.max(max_world.x);
  let offset = world_offset(bbox.min_x(), left_x);
  for offset in [offset - CANVAS_SIZE, offset, offset + CANVAS_SIZE] {
    if bbox.max_x() + offset >= min_x && bbox.min_x() + offset <= max_x {
      return true;
    }
  }

  false
}

fn viewport_left_x(viewport: MapViewport) -> f32 {
  let inv = viewport.transform.invert();
  inv.apply(egui::pos2(viewport.rect.min.x, 0.0).into()).x
}

fn world_offset(world_x: f32, left_x: f32) -> f32 {
  ((left_x - world_x) / CANVAS_SIZE - 1e-6).ceil() * CANVAS_SIZE
}

fn segment_intersects_rect(start: egui::Pos2, end: egui::Pos2, rect: egui::Rect) -> bool {
  if rect.contains(start) || rect.contains(end) {
    return true;
  }

  let segment_rect = egui::Rect::from_min_max(
    egui::pos2(start.x.min(end.x), start.y.min(end.y)),
    egui::pos2(start.x.max(end.x), start.y.max(end.y)),
  );
  segment_rect.intersects(rect)
}

fn style_color(style: Option<&Style>) -> Color {
  color32_to_bevy(style.unwrap_or(&DEFAULT_STYLE).color().gamma_multiply(0.7))
}

fn color32_to_bevy(color: egui::Color32) -> Color {
  Color::srgba(
    f32::from(color.r()) / 255.0,
    f32::from(color.g()) / 255.0,
    f32::from(color.b()) / 255.0,
    f32::from(color.a()) / 255.0,
  )
}

fn snapshot_stats(geometries: &[Geometry<PixelCoordinate>]) -> GeometryStats {
  let mut stats = GeometryStats::default();
  for geometry in geometries {
    collect_stats(geometry, &mut stats);
  }
  stats
}

fn collect_stats(geometry: &Geometry<PixelCoordinate>, stats: &mut GeometryStats) {
  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        collect_stats(geometry, stats);
      }
    }
    Geometry::Point(_, _) => {
      stats.geometries += 1;
      stats.points += 1;
    }
    Geometry::LineString(coords, _) => {
      stats.geometries += 1;
      stats.line_segments += coords.len().saturating_sub(1);
    }
    Geometry::Polygon(coords, _) => {
      stats.geometries += 1;
      stats.polygons += 1;
      stats.line_segments += coords.len();
    }
    Geometry::Heatmap(coords, _) => {
      stats.geometries += 1;
      stats.heatmap_points += coords.len();
    }
  }
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: usize) {
  ui.horizontal(|ui| {
    ui.label(label);
    ui.label(value.to_string());
  });
}
