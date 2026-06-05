use bevy::{
  asset::RenderAssetUsages,
  mesh::{Indices, PrimitiveTopology},
  prelude::*,
  window::PrimaryWindow,
};
use mapvas::{
  config::{Config, HeadingStyle},
  map::{
    color::Color as MapColor,
    coordinates::{CANVAS_SIZE, PixelCoordinate, PixelPosition, PixelRect},
    geometry_collection::{DEFAULT_STYLE, Geometry, Style},
    viewport::{GeometrySnapshot, MapViewport},
  },
};

use crate::bevy_map::BevyMapViewport;

const POINT_RADIUS: f32 = 4.0;
const HIGHLIGHT_POINT_RADIUS: f32 = 10.0;
const GEOMETRY_STROKE_WIDTH: f32 = 4.0;
const HIGHLIGHT_STROKE_WIDTH: f32 = 6.0;
const MAX_HEATMAP_POINTS_PER_FRAME: usize = 20_000;
const GEOMETRY_FILL_Z: f32 = -5.0;
const HIGHLIGHT_FILL_Z: f32 = -4.5;
const POINT_FILL_Z: f32 = -4.0;

pub struct BevyGeometryPlugin;

impl Plugin for BevyGeometryPlugin {
  fn build(&self, app: &mut App) {
    app
      .init_gizmo_group::<BevyGeometryGizmos>()
      .init_gizmo_group::<BevyHighlightGizmos>()
      .add_systems(Startup, configure_bevy_geometry_gizmos)
      .add_systems(Update, draw_bevy_geometry);
  }
}

#[derive(Default, Reflect, GizmoConfigGroup)]
#[reflect(Default)]
struct BevyGeometryGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
#[reflect(Default)]
struct BevyHighlightGizmos;

#[derive(Clone, Copy, Default)]
struct GeometryStats {
  geometries: usize,
  points: usize,
  line_segments: usize,
  polygons: usize,
  heatmap_points: usize,
}

#[derive(Resource)]
pub struct BevyGeometryLayer {
  enabled: bool,
  snapshot_version: u64,
  geometries_version: u64,
  geometries: Vec<Geometry<PixelCoordinate>>,
  highlighted_geometries: Vec<Geometry<PixelCoordinate>>,
  snapshot_stats: GeometryStats,
  draw_stats: GeometryStats,
  fill_entities: Vec<Entity>,
  fill_snapshot_version: u64,
  point_fill_entities: Vec<Entity>,
  point_fill_snapshot_version: u64,
  highlight_fill_entities: Vec<Entity>,
  highlight_fill_snapshot_version: u64,
  heading_style: HeadingStyle,
}

impl Default for BevyGeometryLayer {
  fn default() -> Self {
    Self {
      enabled: true,
      snapshot_version: u64::MAX,
      geometries_version: u64::MAX,
      geometries: Vec::new(),
      highlighted_geometries: Vec::new(),
      snapshot_stats: GeometryStats::default(),
      draw_stats: GeometryStats::default(),
      fill_entities: Vec::new(),
      fill_snapshot_version: u64::MAX,
      point_fill_entities: Vec::new(),
      point_fill_snapshot_version: u64::MAX,
      highlight_fill_entities: Vec::new(),
      highlight_fill_snapshot_version: u64::MAX,
      heading_style: HeadingStyle::default(),
    }
  }
}

#[derive(Clone, Copy)]
struct GeometryBounds {
  min_x: f32,
  min_y: f32,
  max_x: f32,
  max_y: f32,
}

impl GeometryBounds {
  fn min(self) -> PixelCoordinate {
    PixelCoordinate {
      x: self.min_x,
      y: self.min_y,
    }
  }
}

#[derive(Component)]
struct BevyPolygonFill {
  bounds: GeometryBounds,
  origin: PixelCoordinate,
  wrap_offset: f32,
}

#[derive(Component)]
struct BevyHighlightPolygonFill {
  bounds: GeometryBounds,
  origin: PixelCoordinate,
  wrap_offset: f32,
}

#[derive(Component)]
struct BevyPointFill {
  coord: PixelCoordinate,
}

struct PolygonFillSpec {
  mesh: Mesh,
  color: Color,
  bounds: GeometryBounds,
  origin: PixelCoordinate,
}

struct PointFillSpec {
  coord: PixelCoordinate,
  color: Color,
}

fn configure_bevy_geometry_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
  let (config, _) = config_store.config_mut::<BevyGeometryGizmos>();
  config.line.width = GEOMETRY_STROKE_WIDTH;
  config.line.joints = GizmoLineJoint::Round(4);
  config.depth_bias = -1.0;

  let (config, _) = config_store.config_mut::<BevyHighlightGizmos>();
  config.line.width = HIGHLIGHT_STROKE_WIDTH;
  config.line.joints = GizmoLineJoint::Round(4);
  config.depth_bias = -1.0;
}

impl BevyGeometryLayer {
  pub fn ui(&mut self, ui: &mut egui::Ui) {
    ui.collapsing("Bevy Geometry Layer", |ui| {
      ui.checkbox(&mut self.enabled, "visible");

      ui.separator();
      ui.label("Snapshot:");
      stat_row(ui, "Geometries:", self.snapshot_stats.geometries);
      stat_row(ui, "Points:", self.snapshot_stats.points);
      stat_row(ui, "Line segments:", self.snapshot_stats.line_segments);
      stat_row(ui, "Polygons:", self.snapshot_stats.polygons);
      stat_row(ui, "Heatmap points:", self.snapshot_stats.heatmap_points);
      stat_row(ui, "Highlighted:", self.highlighted_geometries.len());

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
    let GeometrySnapshot {
      version,
      geometry_version,
      geometries,
      highlighted_geometries,
    } = snapshot;

    if self.snapshot_version == version {
      return;
    }

    self.snapshot_version = version;
    if self.geometries_version != geometry_version {
      self.geometries_version = geometry_version;
      self.snapshot_stats = snapshot_stats(&geometries);
      self.geometries = geometries;
    }
    self.highlighted_geometries = highlighted_geometries;
  }

  pub fn update_config(&mut self, config: &Config) {
    self.heading_style = config.heading_style;
  }

  #[must_use]
  pub fn snapshot_version(&self) -> u64 {
    self.snapshot_version
  }

  #[must_use]
  pub fn geometry_version(&self) -> u64 {
    self.geometries_version
  }
}

fn draw_bevy_geometry(
  mut commands: Commands,
  mut layer: ResMut<BevyGeometryLayer>,
  viewport: Res<BevyMapViewport>,
  windows: Query<&Window, With<PrimaryWindow>>,
  mut meshes: ResMut<Assets<Mesh>>,
  mut materials: ResMut<Assets<ColorMaterial>>,
  mut fills: Query<
    (&BevyPolygonFill, &mut Transform, &mut Visibility),
    (Without<BevyHighlightPolygonFill>, Without<BevyPointFill>),
  >,
  mut highlight_fills: Query<
    (&BevyHighlightPolygonFill, &mut Transform, &mut Visibility),
    (Without<BevyPolygonFill>, Without<BevyPointFill>),
  >,
  mut point_fills: Query<
    (&BevyPointFill, &mut Transform, &mut Visibility),
    (Without<BevyPolygonFill>, Without<BevyHighlightPolygonFill>),
  >,
  mut gizmos: Gizmos<BevyGeometryGizmos>,
  mut highlight_gizmos: Gizmos<BevyHighlightGizmos>,
) {
  let mut draw_stats = GeometryStats::default();
  if !layer.enabled {
    set_fill_visibility(&mut fills, false);
    set_highlight_fill_visibility(&mut highlight_fills, false);
    set_point_fill_visibility(&mut point_fills, false);
    layer.draw_stats = draw_stats;
    return;
  }
  let Some(viewport) = viewport.get() else {
    set_fill_visibility(&mut fills, false);
    set_highlight_fill_visibility(&mut highlight_fills, false);
    set_point_fill_visibility(&mut point_fills, false);
    layer.draw_stats = draw_stats;
    return;
  };
  let Ok(window) = windows.single() else {
    set_fill_visibility(&mut fills, false);
    set_highlight_fill_visibility(&mut highlight_fills, false);
    set_point_fill_visibility(&mut point_fills, false);
    layer.draw_stats = draw_stats;
    return;
  };

  rebuild_polygon_fills_if_needed(&mut commands, &mut meshes, &mut materials, &mut layer);
  update_polygon_fills(viewport, window, &mut fills);
  rebuild_point_fills_if_needed(&mut commands, &mut meshes, &mut materials, &mut layer);
  rebuild_highlight_polygon_fills_if_needed(&mut commands, &mut meshes, &mut materials, &mut layer);
  update_highlight_polygon_fills(viewport, window, &mut highlight_fills);

  let left_x = viewport_left_x(viewport);
  update_point_fills(viewport, window, left_x, &mut point_fills);
  let heading_style = layer.heading_style;
  for geometry in &layer.geometries {
    draw_geometry(
      geometry,
      viewport,
      window,
      left_x,
      heading_style,
      &mut gizmos,
      &mut draw_stats,
    );
  }
  for geometry in &layer.highlighted_geometries {
    draw_highlighted_geometry(geometry, viewport, window, left_x, &mut highlight_gizmos);
  }
  layer.draw_stats = draw_stats;
}

fn set_fill_visibility(
  fills: &mut Query<
    (&BevyPolygonFill, &mut Transform, &mut Visibility),
    (Without<BevyHighlightPolygonFill>, Without<BevyPointFill>),
  >,
  visible: bool,
) {
  for (_, _, mut visibility) in fills.iter_mut() {
    *visibility = if visible {
      Visibility::Visible
    } else {
      Visibility::Hidden
    };
  }
}

fn set_highlight_fill_visibility(
  fills: &mut Query<
    (&BevyHighlightPolygonFill, &mut Transform, &mut Visibility),
    (Without<BevyPolygonFill>, Without<BevyPointFill>),
  >,
  visible: bool,
) {
  for (_, _, mut visibility) in fills.iter_mut() {
    *visibility = if visible {
      Visibility::Visible
    } else {
      Visibility::Hidden
    };
  }
}

fn set_point_fill_visibility(
  fills: &mut Query<
    (&BevyPointFill, &mut Transform, &mut Visibility),
    (Without<BevyPolygonFill>, Without<BevyHighlightPolygonFill>),
  >,
  visible: bool,
) {
  for (_, _, mut visibility) in fills.iter_mut() {
    *visibility = if visible {
      Visibility::Visible
    } else {
      Visibility::Hidden
    };
  }
}

fn rebuild_polygon_fills_if_needed(
  commands: &mut Commands,
  meshes: &mut Assets<Mesh>,
  materials: &mut Assets<ColorMaterial>,
  layer: &mut BevyGeometryLayer,
) {
  if layer.fill_snapshot_version == layer.geometries_version {
    return;
  }

  for entity in layer.fill_entities.drain(..) {
    commands.entity(entity).despawn();
  }

  let fill_specs = polygon_fill_specs(&layer.geometries);
  let mut fill_entities = Vec::new();
  for spec in fill_specs {
    let mesh = meshes.add(spec.mesh);
    let material = materials.add(ColorMaterial::from(spec.color));
    for wrap_offset in [-CANVAS_SIZE, 0.0, CANVAS_SIZE] {
      let entity = commands
        .spawn((
          Mesh2d(mesh.clone()),
          MeshMaterial2d(material.clone()),
          Transform::default(),
          BevyPolygonFill {
            bounds: spec.bounds,
            origin: spec.origin,
            wrap_offset,
          },
        ))
        .id();
      fill_entities.push(entity);
    }
  }

  layer.fill_entities = fill_entities;
  layer.fill_snapshot_version = layer.geometries_version;
}

fn rebuild_point_fills_if_needed(
  commands: &mut Commands,
  meshes: &mut Assets<Mesh>,
  materials: &mut Assets<ColorMaterial>,
  layer: &mut BevyGeometryLayer,
) {
  if layer.point_fill_snapshot_version == layer.geometries_version {
    return;
  }

  for entity in layer.point_fill_entities.drain(..) {
    commands.entity(entity).despawn();
  }

  let mesh = meshes.add(Circle::new(POINT_RADIUS).mesh().resolution(16));
  let fill_specs = point_fill_specs(&layer.geometries);
  let mut point_fill_entities = Vec::new();
  for spec in fill_specs {
    let material = materials.add(ColorMaterial::from(spec.color));
    let entity = commands
      .spawn((
        Mesh2d(mesh.clone()),
        MeshMaterial2d(material),
        Transform::default(),
        Visibility::Hidden,
        BevyPointFill { coord: spec.coord },
      ))
      .id();
    point_fill_entities.push(entity);
  }

  layer.point_fill_entities = point_fill_entities;
  layer.point_fill_snapshot_version = layer.geometries_version;
}

fn rebuild_highlight_polygon_fills_if_needed(
  commands: &mut Commands,
  meshes: &mut Assets<Mesh>,
  materials: &mut Assets<ColorMaterial>,
  layer: &mut BevyGeometryLayer,
) {
  if layer.highlight_fill_snapshot_version == layer.snapshot_version {
    return;
  }

  for entity in layer.highlight_fill_entities.drain(..) {
    commands.entity(entity).despawn();
  }

  let fill_specs = highlighted_polygon_fill_specs(&layer.highlighted_geometries);
  let mut fill_entities = Vec::new();
  for spec in fill_specs {
    let mesh = meshes.add(spec.mesh);
    let material = materials.add(ColorMaterial::from(spec.color));
    for wrap_offset in [-CANVAS_SIZE, 0.0, CANVAS_SIZE] {
      let entity = commands
        .spawn((
          Mesh2d(mesh.clone()),
          MeshMaterial2d(material.clone()),
          Transform::default(),
          BevyHighlightPolygonFill {
            bounds: spec.bounds,
            origin: spec.origin,
            wrap_offset,
          },
        ))
        .id();
      fill_entities.push(entity);
    }
  }

  layer.highlight_fill_entities = fill_entities;
  layer.highlight_fill_snapshot_version = layer.snapshot_version;
}

fn update_polygon_fills(
  viewport: MapViewport,
  window: &Window,
  fills: &mut Query<
    (&BevyPolygonFill, &mut Transform, &mut Visibility),
    (Without<BevyHighlightPolygonFill>, Without<BevyPointFill>),
  >,
) {
  for (fill, mut transform, mut visibility) in fills.iter_mut() {
    let wrap_offset = polygon_fill_wrap_offset(fill.bounds, viewport, fill.wrap_offset);
    *transform =
      polygon_fill_transform(viewport, window, fill.origin, wrap_offset, GEOMETRY_FILL_Z);
    *visibility = if bounds_intersects_viewport(fill.bounds, viewport, wrap_offset) {
      Visibility::Visible
    } else {
      Visibility::Hidden
    };
  }
}

fn update_highlight_polygon_fills(
  viewport: MapViewport,
  window: &Window,
  fills: &mut Query<
    (&BevyHighlightPolygonFill, &mut Transform, &mut Visibility),
    (Without<BevyPolygonFill>, Without<BevyPointFill>),
  >,
) {
  for (fill, mut transform, mut visibility) in fills.iter_mut() {
    let wrap_offset = polygon_fill_wrap_offset(fill.bounds, viewport, fill.wrap_offset);
    *transform =
      polygon_fill_transform(viewport, window, fill.origin, wrap_offset, HIGHLIGHT_FILL_Z);
    *visibility = if bounds_intersects_viewport(fill.bounds, viewport, wrap_offset) {
      Visibility::Visible
    } else {
      Visibility::Hidden
    };
  }
}

fn update_point_fills(
  viewport: MapViewport,
  window: &Window,
  left_x: f32,
  fills: &mut Query<
    (&BevyPointFill, &mut Transform, &mut Visibility),
    (Without<BevyPolygonFill>, Without<BevyHighlightPolygonFill>),
  >,
) {
  for (fill, mut transform, mut visibility) in fills.iter_mut() {
    let Some(screen) = coordinate_to_screen(fill.coord, viewport, left_x) else {
      *visibility = Visibility::Hidden;
      continue;
    };
    if viewport.contains(screen) {
      let center = screen_to_bevy(screen, window);
      *transform = Transform::from_xyz(center.x, center.y, POINT_FILL_Z);
      *visibility = Visibility::Visible;
    } else {
      *visibility = Visibility::Hidden;
    }
  }
}

fn polygon_fill_wrap_offset(
  bounds: GeometryBounds,
  viewport: MapViewport,
  copy_offset: f32,
) -> f32 {
  world_offset(bounds.min_x, viewport_left_x(viewport)) + copy_offset
}

fn polygon_fill_transform(
  viewport: MapViewport,
  window: &Window,
  origin: PixelCoordinate,
  wrap_offset: f32,
  z: f32,
) -> Transform {
  Transform::from_xyz(
    viewport.transform.trans.x + (origin.x + wrap_offset) * viewport.transform.zoom
      - window.width() / 2.0,
    window.height() / 2.0 - viewport.transform.trans.y - origin.y * viewport.transform.zoom,
    z,
  )
  .with_scale(Vec3::new(
    viewport.transform.zoom,
    -viewport.transform.zoom,
    1.0,
  ))
}

fn polygon_fill_specs(geometries: &[Geometry<PixelCoordinate>]) -> Vec<PolygonFillSpec> {
  let mut fills = Vec::new();
  for geometry in geometries {
    collect_polygon_fill_specs(geometry, &mut fills);
  }
  fills
}

fn highlighted_polygon_fill_specs(
  geometries: &[Geometry<PixelCoordinate>],
) -> Vec<PolygonFillSpec> {
  let mut fills = Vec::new();
  for geometry in geometries {
    collect_highlighted_polygon_fill_specs(geometry, &mut fills);
  }
  fills
}

fn point_fill_specs(geometries: &[Geometry<PixelCoordinate>]) -> Vec<PointFillSpec> {
  let mut fills = Vec::new();
  for geometry in geometries {
    collect_point_fill_specs(geometry, &mut fills);
  }
  fills
}

fn collect_polygon_fill_specs(
  geometry: &Geometry<PixelCoordinate>,
  fills: &mut Vec<PolygonFillSpec>,
) {
  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        collect_polygon_fill_specs(geometry, fills);
      }
    }
    Geometry::Polygon(coords, metadata) => {
      let fill_color = metadata
        .style
        .as_ref()
        .unwrap_or(&DEFAULT_STYLE)
        .fill_color()
        .gamma_multiply(0.7);
      if fill_color.a() == 0 {
        return;
      }
      let Some(bounds) = bounds_from_coords(coords) else {
        return;
      };
      let origin = bounds.min();
      let Some(mesh) = polygon_fill_mesh(coords, origin) else {
        return;
      };
      fills.push(PolygonFillSpec {
        mesh,
        color: map_color_to_bevy(fill_color),
        bounds,
        origin,
      });
    }
    Geometry::Point(_, _) | Geometry::LineString(_, _) | Geometry::Heatmap(_, _) => {}
  }
}

fn collect_highlighted_polygon_fill_specs(
  geometry: &Geometry<PixelCoordinate>,
  fills: &mut Vec<PolygonFillSpec>,
) {
  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        collect_highlighted_polygon_fill_specs(geometry, fills);
      }
    }
    Geometry::Polygon(coords, metadata) => {
      let color = metadata.style.as_ref().unwrap_or(&DEFAULT_STYLE).color();
      if color.a() == 0 {
        return;
      }
      let Some(bounds) = bounds_from_coords(coords) else {
        return;
      };
      let origin = bounds.min();
      let Some(mesh) = polygon_fill_mesh(coords, origin) else {
        return;
      };
      fills.push(PolygonFillSpec {
        mesh,
        color: map_color_to_bevy(color),
        bounds,
        origin,
      });
    }
    Geometry::Point(_, _) | Geometry::LineString(_, _) | Geometry::Heatmap(_, _) => {}
  }
}

fn collect_point_fill_specs(geometry: &Geometry<PixelCoordinate>, fills: &mut Vec<PointFillSpec>) {
  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        collect_point_fill_specs(geometry, fills);
      }
    }
    Geometry::Point(coord, metadata) => {
      if !coord.is_valid() {
        return;
      }
      fills.push(PointFillSpec {
        coord: *coord,
        color: style_color(metadata.style.as_ref()),
      });
    }
    Geometry::LineString(_, _) | Geometry::Polygon(_, _) | Geometry::Heatmap(_, _) => {}
  }
}

fn polygon_fill_mesh(coords: &[PixelCoordinate], origin: PixelCoordinate) -> Option<Mesh> {
  let vertices = polygon_vertices(coords);
  if vertices.len() < 3 {
    return None;
  }

  let mut indices = Vec::with_capacity((vertices.len() - 2) * 3);
  let mut earcut = earcut::Earcut::new();
  earcut.earcut(
    vertices.iter().map(|coord| [coord.x, coord.y]),
    &[],
    &mut indices,
  );
  if indices.is_empty() {
    return None;
  }

  let positions = vertices
    .iter()
    .map(|coord| [coord.x - origin.x, coord.y - origin.y, 0.0])
    .collect::<Vec<_>>();
  let normals = vec![[0.0, 0.0, 1.0]; vertices.len()];
  let uvs = vec![[0.0, 0.0]; vertices.len()];

  Some(
    Mesh::new(
      PrimitiveTopology::TriangleList,
      RenderAssetUsages::default(),
    )
    .with_inserted_indices(Indices::U32(indices))
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs),
  )
}

fn polygon_vertices(coords: &[PixelCoordinate]) -> Vec<PixelCoordinate> {
  let mut vertices = coords
    .iter()
    .copied()
    .filter(PixelCoordinate::is_valid)
    .collect::<Vec<_>>();
  if vertices
    .first()
    .zip(vertices.last())
    .is_some_and(|(first, last)| first.x == last.x && first.y == last.y)
  {
    vertices.pop();
  }
  vertices
}

fn bounds_from_coords(coords: &[PixelCoordinate]) -> Option<GeometryBounds> {
  let mut coords = coords.iter().copied().filter(PixelCoordinate::is_valid);
  let first = coords.next()?;
  let mut bounds = GeometryBounds {
    min_x: first.x,
    min_y: first.y,
    max_x: first.x,
    max_y: first.y,
  };

  for coord in coords {
    bounds.min_x = bounds.min_x.min(coord.x);
    bounds.min_y = bounds.min_y.min(coord.y);
    bounds.max_x = bounds.max_x.max(coord.x);
    bounds.max_y = bounds.max_y.max(coord.y);
  }

  Some(bounds)
}

fn bounds_intersects_viewport(
  bounds: GeometryBounds,
  viewport: MapViewport,
  wrap_offset: f32,
) -> bool {
  let inv = viewport.transform.invert();
  let min_world = inv.apply(viewport.min());
  let max_world = inv.apply(viewport.max());
  let min_y = min_world.y.min(max_world.y);
  let max_y = min_world.y.max(max_world.y);
  if bounds.max_y < min_y || bounds.min_y > max_y {
    return false;
  }

  let min_x = min_world.x.min(max_world.x);
  let max_x = min_world.x.max(max_world.x);
  bounds.max_x + wrap_offset >= min_x && bounds.min_x + wrap_offset <= max_x
}

fn draw_geometry(
  geometry: &Geometry<PixelCoordinate>,
  viewport: MapViewport,
  window: &Window,
  left_x: f32,
  heading_style: HeadingStyle,
  gizmos: &mut Gizmos<BevyGeometryGizmos>,
  stats: &mut GeometryStats,
) {
  if !geometry_intersects_viewport(geometry, viewport, left_x) {
    return;
  }

  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        draw_geometry(
          geometry,
          viewport,
          window,
          left_x,
          heading_style,
          gizmos,
          stats,
        );
      }
    }
    Geometry::Point(coord, metadata) => {
      let Some(screen) = coordinate_to_screen(*coord, viewport, left_x) else {
        return;
      };
      if !viewport.contains(screen) {
        return;
      }

      let color = style_color(metadata.style.as_ref());
      let center = screen_to_bevy(screen, window);
      gizmos.circle_2d(center, POINT_RADIUS, color);
      if let Some(heading) = metadata.heading {
        draw_heading(gizmos, center, heading, color, heading_style);
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
        if !viewport.contains(screen) {
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
  gizmos: &mut Gizmos<BevyGeometryGizmos>,
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
  gizmos: &mut Gizmos<BevyGeometryGizmos>,
) -> bool {
  let Some((screen_start, screen_end)) = segment_to_screen(start, end, viewport, left_x) else {
    return false;
  };

  gizmos.line_2d(
    screen_to_bevy(screen_start, window),
    screen_to_bevy(screen_end, window),
    color,
  );
  true
}

fn draw_highlighted_geometry(
  geometry: &Geometry<PixelCoordinate>,
  viewport: MapViewport,
  window: &Window,
  left_x: f32,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  if !geometry_intersects_viewport(geometry, viewport, left_x) {
    return;
  }

  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        draw_highlighted_geometry(geometry, viewport, window, left_x, gizmos);
      }
    }
    Geometry::Point(coord, metadata) => {
      let Some(screen) = coordinate_to_screen(*coord, viewport, left_x) else {
        return;
      };
      if !viewport.contains(screen) {
        return;
      }
      gizmos.circle_2d(
        screen_to_bevy(screen, window),
        HIGHLIGHT_POINT_RADIUS,
        highlight_color(metadata.style.as_ref()),
      );
    }
    Geometry::LineString(coords, metadata) => {
      draw_highlighted_lines(
        coords,
        false,
        viewport,
        window,
        left_x,
        highlight_color(metadata.style.as_ref()),
        gizmos,
      );
    }
    Geometry::Polygon(coords, metadata) => {
      draw_highlighted_lines(
        coords,
        true,
        viewport,
        window,
        left_x,
        highlight_color(metadata.style.as_ref()),
        gizmos,
      );
    }
    Geometry::Heatmap(_, _) => {}
  }
}

fn draw_highlighted_lines(
  coords: &[PixelCoordinate],
  closed: bool,
  viewport: MapViewport,
  window: &Window,
  left_x: f32,
  color: Color,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  if coords.len() < 2 {
    return;
  }

  for segment in coords.windows(2) {
    draw_highlighted_segment(
      segment[0], segment[1], viewport, window, left_x, color, gizmos,
    );
  }

  if closed && let (Some(first), Some(last)) = (coords.first(), coords.last()) {
    draw_highlighted_segment(*last, *first, viewport, window, left_x, color, gizmos);
  }
}

fn draw_highlighted_segment(
  start: PixelCoordinate,
  end: PixelCoordinate,
  viewport: MapViewport,
  window: &Window,
  left_x: f32,
  color: Color,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  let Some((screen_start, screen_end)) = segment_to_screen(start, end, viewport, left_x) else {
    return;
  };

  gizmos.line_2d(
    screen_to_bevy(screen_start, window),
    screen_to_bevy(screen_end, window),
    color,
  );
}

fn draw_heading(
  gizmos: &mut Gizmos<BevyGeometryGizmos>,
  center: Vec2,
  heading_degrees: f32,
  color: Color,
  style: HeadingStyle,
) {
  let (forward, right) = heading_vectors(heading_degrees);

  match style {
    HeadingStyle::Arrow => {
      let length = POINT_RADIUS + 8.0;
      let half_width = 2.0;
      let tip = center + forward * length;
      let base_left = center + right * half_width;
      let base_right = center - right * half_width;
      draw_line_loop(gizmos, &[tip, base_left, base_right], color);
    }
    HeadingStyle::Line => {
      gizmos.line_2d(center, center + forward * (POINT_RADIUS + 6.0), color);
    }
    HeadingStyle::Chevron => {
      let length = POINT_RADIUS + 4.0;
      let tip = center + forward * length;
      let back = forward * (length * 0.55);
      let wing = right * (length * 0.35);
      gizmos.line_2d(tip - back + wing, tip, color);
      gizmos.line_2d(tip, tip - back - wing, color);
    }
    HeadingStyle::Needle => {
      let length = POINT_RADIUS + 8.0;
      let tip = center + forward * length;
      gizmos.line_2d(center, tip, color);
      let head_base = tip - forward * 2.0;
      gizmos.line_2d(tip, head_base + right, color);
      gizmos.line_2d(tip, head_base - right, color);
    }
    HeadingStyle::Sector => {
      let radius = POINT_RADIUS + 4.0;
      let sector_angle = 0.6;
      let heading_rad = (heading_degrees - 90.0).to_radians();
      let mut points = Vec::with_capacity(10);
      points.push(center);
      for index in 0..=8 {
        #[allow(clippy::cast_precision_loss)]
        let angle = heading_rad - sector_angle / 2.0 + sector_angle * index as f32 / 8.0;
        let direction = Vec2::new(angle.cos(), -angle.sin());
        points.push(center + direction * radius);
      }
      draw_heading_lines(gizmos, &points, color);
      if let Some(last) = points.last().copied() {
        gizmos.line_2d(last, center, color);
      }
    }
    HeadingStyle::Rectangle => {
      let half_length = (POINT_RADIUS + 6.0) / 2.0;
      let half_width = 1.5;
      let corners = [
        center + forward * half_length + right * half_width,
        center + forward * half_length - right * half_width,
        center - forward * half_length - right * half_width,
        center - forward * half_length + right * half_width,
      ];
      draw_line_loop(gizmos, &corners, color);
    }
  }
}

fn heading_vectors(heading_degrees: f32) -> (Vec2, Vec2) {
  let heading_rad = (heading_degrees - 90.0).to_radians();
  let forward = Vec2::new(heading_rad.cos(), -heading_rad.sin());
  let right = Vec2::new(forward.y, -forward.x);
  (forward, right)
}

fn draw_line_loop(gizmos: &mut Gizmos<BevyGeometryGizmos>, points: &[Vec2], color: Color) {
  draw_heading_lines(gizmos, points, color);
  if let (Some(first), Some(last)) = (points.first(), points.last()) {
    gizmos.line_2d(*last, *first, color);
  }
}

fn draw_heading_lines(gizmos: &mut Gizmos<BevyGeometryGizmos>, points: &[Vec2], color: Color) {
  for segment in points.windows(2) {
    gizmos.line_2d(segment[0], segment[1], color);
  }
}

fn coordinate_to_screen(
  coord: PixelCoordinate,
  viewport: MapViewport,
  left_x: f32,
) -> Option<PixelPosition> {
  if !coord.is_valid() {
    return None;
  }
  let offset = world_offset(coord.x, left_x);
  let shifted = PixelCoordinate {
    x: coord.x + offset,
    y: coord.y,
  };
  Some(viewport.transform.apply(shifted))
}

fn segment_to_screen(
  start: PixelCoordinate,
  end: PixelCoordinate,
  viewport: MapViewport,
  left_x: f32,
) -> Option<(PixelPosition, PixelPosition)> {
  if !start.is_valid() || !end.is_valid() {
    return None;
  }

  let offset = world_offset(start.x.min(end.x), left_x);
  for offset in [offset - CANVAS_SIZE, offset, offset + CANVAS_SIZE] {
    let screen_start = viewport.transform.apply(PixelCoordinate {
      x: start.x + offset,
      y: start.y,
    });
    let screen_end = viewport.transform.apply(PixelCoordinate {
      x: end.x + offset,
      y: end.y,
    });
    if segment_intersects_rect(screen_start, screen_end, viewport.rect) {
      return Some((screen_start, screen_end));
    }
  }

  None
}

fn screen_to_bevy(screen: PixelPosition, window: &Window) -> Vec2 {
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
  let min_world = inv.apply(viewport.min());
  let max_world = inv.apply(viewport.max());
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
  inv
    .apply(PixelPosition {
      x: viewport.min().x,
      y: 0.0,
    })
    .x
}

fn world_offset(world_x: f32, left_x: f32) -> f32 {
  ((left_x - world_x) / CANVAS_SIZE - 1e-6).ceil() * CANVAS_SIZE
}

fn segment_intersects_rect(start: PixelPosition, end: PixelPosition, rect: PixelRect) -> bool {
  if rect.contains(start) || rect.contains(end) {
    return true;
  }

  let segment_rect = PixelRect::from_min_max(
    PixelPosition {
      x: start.x.min(end.x),
      y: start.y.min(end.y),
    },
    PixelPosition {
      x: start.x.max(end.x),
      y: start.y.max(end.y),
    },
  );
  segment_rect.intersects(rect)
}

fn style_color(style: Option<&Style>) -> Color {
  map_color_to_bevy(style.unwrap_or(&DEFAULT_STYLE).color().gamma_multiply(0.7))
}

fn highlight_color(style: Option<&Style>) -> Color {
  map_color_to_bevy(style.unwrap_or(&DEFAULT_STYLE).color())
}

fn map_color_to_bevy(color: MapColor) -> Color {
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
