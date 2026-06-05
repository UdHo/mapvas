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
  polygon_fill_instances: Vec<PolygonFillInstance>,
  fill_snapshot_version: u64,
  point_fill_instances: Vec<PointFillInstance>,
  point_fill_snapshot_version: u64,
  highlight_polygon_fill_instances: Vec<PolygonFillInstance>,
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
      polygon_fill_instances: Vec::new(),
      fill_snapshot_version: u64::MAX,
      point_fill_instances: Vec::new(),
      point_fill_snapshot_version: u64::MAX,
      highlight_polygon_fill_instances: Vec::new(),
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
struct BevyPolygonFill;

#[derive(Component)]
struct BevyHighlightPolygonFill;

#[derive(Component)]
struct BevyPointFill;

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

struct PolygonFillInstance {
  mesh: Handle<Mesh>,
  material: Handle<ColorMaterial>,
  bounds: GeometryBounds,
  origin: PixelCoordinate,
  entities: Vec<Entity>,
}

struct PointFillInstance {
  mesh: Handle<Mesh>,
  material: Handle<ColorMaterial>,
  coord: PixelCoordinate,
  entities: Vec<Entity>,
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
    (&mut Transform, &mut Visibility),
    (
      With<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
  mut highlight_fills: Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyHighlightPolygonFill>,
      Without<BevyPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
  mut point_fills: Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyPointFill>,
      Without<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
    ),
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
  update_polygon_fills(
    &mut commands,
    viewport,
    window,
    &mut layer.polygon_fill_instances,
    &mut fills,
  );
  rebuild_point_fills_if_needed(&mut commands, &mut meshes, &mut materials, &mut layer);
  rebuild_highlight_polygon_fills_if_needed(&mut commands, &mut meshes, &mut materials, &mut layer);
  update_highlight_polygon_fills(
    &mut commands,
    viewport,
    window,
    &mut layer.highlight_polygon_fill_instances,
    &mut highlight_fills,
  );

  update_point_fills(
    &mut commands,
    viewport,
    window,
    &mut layer.point_fill_instances,
    &mut point_fills,
  );
  let heading_style = layer.heading_style;
  for geometry in &layer.geometries {
    draw_geometry(
      geometry,
      viewport,
      window,
      heading_style,
      &mut gizmos,
      &mut draw_stats,
    );
  }
  for geometry in &layer.highlighted_geometries {
    draw_highlighted_geometry(geometry, viewport, window, &mut highlight_gizmos);
  }
  layer.draw_stats = draw_stats;
}

fn set_fill_visibility(
  fills: &mut Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
  visible: bool,
) {
  for (_, mut visibility) in fills.iter_mut() {
    *visibility = if visible {
      Visibility::Visible
    } else {
      Visibility::Hidden
    };
  }
}

fn set_highlight_fill_visibility(
  fills: &mut Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyHighlightPolygonFill>,
      Without<BevyPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
  visible: bool,
) {
  for (_, mut visibility) in fills.iter_mut() {
    *visibility = if visible {
      Visibility::Visible
    } else {
      Visibility::Hidden
    };
  }
}

fn set_point_fill_visibility(
  fills: &mut Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyPointFill>,
      Without<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
    ),
  >,
  visible: bool,
) {
  for (_, mut visibility) in fills.iter_mut() {
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

  for instance in layer.polygon_fill_instances.drain(..) {
    for entity in instance.entities {
      commands.entity(entity).despawn();
    }
  }

  let fill_specs = polygon_fill_specs(&layer.geometries);
  let mut fill_instances = Vec::new();
  for spec in fill_specs {
    let mesh = meshes.add(spec.mesh);
    let material = materials.add(ColorMaterial::from(spec.color));
    fill_instances.push(PolygonFillInstance {
      mesh,
      material,
      bounds: spec.bounds,
      origin: spec.origin,
      entities: Vec::new(),
    });
  }

  layer.polygon_fill_instances = fill_instances;
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

  for instance in layer.point_fill_instances.drain(..) {
    for entity in instance.entities {
      commands.entity(entity).despawn();
    }
  }

  let mesh = meshes.add(Circle::new(POINT_RADIUS).mesh().resolution(16));
  let fill_specs = point_fill_specs(&layer.geometries);
  let mut point_fill_instances = Vec::new();
  for spec in fill_specs {
    let material = materials.add(ColorMaterial::from(spec.color));
    point_fill_instances.push(PointFillInstance {
      mesh: mesh.clone(),
      material,
      coord: spec.coord,
      entities: Vec::new(),
    });
  }

  layer.point_fill_instances = point_fill_instances;
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

  for instance in layer.highlight_polygon_fill_instances.drain(..) {
    for entity in instance.entities {
      commands.entity(entity).despawn();
    }
  }

  let fill_specs = highlighted_polygon_fill_specs(&layer.highlighted_geometries);
  let mut fill_instances = Vec::new();
  for spec in fill_specs {
    let mesh = meshes.add(spec.mesh);
    let material = materials.add(ColorMaterial::from(spec.color));
    fill_instances.push(PolygonFillInstance {
      mesh,
      material,
      bounds: spec.bounds,
      origin: spec.origin,
      entities: Vec::new(),
    });
  }

  layer.highlight_polygon_fill_instances = fill_instances;
  layer.highlight_fill_snapshot_version = layer.snapshot_version;
}

fn update_polygon_fills(
  commands: &mut Commands,
  viewport: MapViewport,
  window: &Window,
  fill_instances: &mut [PolygonFillInstance],
  fills: &mut Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
) {
  for instance in fill_instances {
    let wrap_offsets = bounds_visible_wrap_offsets(instance.bounds, viewport);
    for (entity_index, wrap_offset) in wrap_offsets.iter().copied().enumerate() {
      let transform = polygon_fill_transform(
        viewport,
        window,
        instance.origin,
        wrap_offset,
        GEOMETRY_FILL_Z,
      );
      if let Some(entity) = instance.entities.get(entity_index).copied() {
        if let Ok((mut fill_transform, mut visibility)) = fills.get_mut(entity) {
          *fill_transform = transform;
          *visibility = Visibility::Visible;
        } else {
          instance.entities[entity_index] =
            spawn_polygon_fill(commands, instance, transform, BevyPolygonFill);
        }
      } else {
        let entity = spawn_polygon_fill(commands, instance, transform, BevyPolygonFill);
        instance.entities.push(entity);
      }
    }

    hide_extra_fill_entities(&instance.entities, wrap_offsets.len(), fills);
  }
}

fn update_highlight_polygon_fills(
  commands: &mut Commands,
  viewport: MapViewport,
  window: &Window,
  fill_instances: &mut [PolygonFillInstance],
  fills: &mut Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyHighlightPolygonFill>,
      Without<BevyPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
) {
  for instance in fill_instances {
    let wrap_offsets = bounds_visible_wrap_offsets(instance.bounds, viewport);
    for (entity_index, wrap_offset) in wrap_offsets.iter().copied().enumerate() {
      let transform = polygon_fill_transform(
        viewport,
        window,
        instance.origin,
        wrap_offset,
        HIGHLIGHT_FILL_Z,
      );
      if let Some(entity) = instance.entities.get(entity_index).copied() {
        if let Ok((mut fill_transform, mut visibility)) = fills.get_mut(entity) {
          *fill_transform = transform;
          *visibility = Visibility::Visible;
        } else {
          instance.entities[entity_index] =
            spawn_polygon_fill(commands, instance, transform, BevyHighlightPolygonFill);
        }
      } else {
        let entity = spawn_polygon_fill(commands, instance, transform, BevyHighlightPolygonFill);
        instance.entities.push(entity);
      }
    }

    hide_extra_fill_entities(&instance.entities, wrap_offsets.len(), fills);
  }
}

fn update_point_fills(
  commands: &mut Commands,
  viewport: MapViewport,
  window: &Window,
  point_instances: &mut [PointFillInstance],
  fills: &mut Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyPointFill>,
      Without<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
    ),
  >,
) {
  for instance in point_instances {
    let screens = coordinate_screen_positions(instance.coord, viewport);
    for (entity_index, screen) in screens.iter().copied().enumerate() {
      let center = screen_to_bevy(screen, window);
      let transform = Transform::from_xyz(center.x, center.y, POINT_FILL_Z);
      if let Some(entity) = instance.entities.get(entity_index).copied() {
        if let Ok((mut fill_transform, mut visibility)) = fills.get_mut(entity) {
          *fill_transform = transform;
          *visibility = Visibility::Visible;
        } else {
          instance.entities[entity_index] = spawn_point_fill(commands, instance, transform);
        }
      } else {
        let entity = spawn_point_fill(commands, instance, transform);
        instance.entities.push(entity);
      }
    }

    hide_extra_fill_entities(&instance.entities, screens.len(), fills);
  }
}

fn spawn_polygon_fill<M: Component>(
  commands: &mut Commands,
  instance: &PolygonFillInstance,
  transform: Transform,
  marker: M,
) -> Entity {
  commands
    .spawn((
      Mesh2d(instance.mesh.clone()),
      MeshMaterial2d(instance.material.clone()),
      transform,
      marker,
    ))
    .id()
}

fn spawn_point_fill(
  commands: &mut Commands,
  instance: &PointFillInstance,
  transform: Transform,
) -> Entity {
  commands
    .spawn((
      Mesh2d(instance.mesh.clone()),
      MeshMaterial2d(instance.material.clone()),
      transform,
      BevyPointFill,
    ))
    .id()
}

fn hide_extra_fill_entities<F: bevy::ecs::query::QueryFilter>(
  entities: &[Entity],
  visible_entity_count: usize,
  fills: &mut Query<(&mut Transform, &mut Visibility), F>,
) {
  for entity in entities.iter().skip(visible_entity_count).copied() {
    if let Ok((_, mut visibility)) = fills.get_mut(entity) {
      *visibility = Visibility::Hidden;
    }
  }
}

fn bounds_visible_wrap_offsets(bounds: GeometryBounds, viewport: MapViewport) -> Vec<f32> {
  let (_, _, viewport_min_y, viewport_max_y) = viewport_world_bounds(viewport);
  if bounds.max_y < viewport_min_y || bounds.min_y > viewport_max_y {
    return Vec::new();
  }

  visible_wrap_offsets(bounds.min_x, bounds.max_x, viewport)
}

fn coordinate_screen_positions(
  coord: PixelCoordinate,
  viewport: MapViewport,
) -> Vec<PixelPosition> {
  if !coord.is_valid() {
    return Vec::new();
  }

  let (_, _, viewport_min_y, viewport_max_y) = viewport_world_bounds(viewport);
  if coord.y < viewport_min_y || coord.y > viewport_max_y {
    return Vec::new();
  }

  visible_wrap_offsets(coord.x, coord.x, viewport)
    .into_iter()
    .filter_map(|wrap_offset| {
      let shifted = PixelCoordinate {
        x: coord.x + wrap_offset,
        y: coord.y,
      };
      let screen = viewport.transform.apply(shifted);
      viewport.contains(screen).then_some(screen)
    })
    .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn visible_wrap_offsets(world_min_x: f32, world_max_x: f32, viewport: MapViewport) -> Vec<f32> {
  let (viewport_min_x, viewport_max_x, _, _) = viewport_world_bounds(viewport);
  let min_copy = ((viewport_min_x - world_max_x) / CANVAS_SIZE - 1e-6).ceil() as i32;
  let max_copy = ((viewport_max_x - world_min_x) / CANVAS_SIZE + 1e-6).floor() as i32;

  if min_copy > max_copy {
    return Vec::new();
  }

  (min_copy..=max_copy)
    .map(|copy| copy as f32 * CANVAS_SIZE)
    .collect()
}

fn viewport_world_bounds(viewport: MapViewport) -> (f32, f32, f32, f32) {
  let inv = viewport.transform.invert();
  let min_world = inv.apply(viewport.min());
  let max_world = inv.apply(viewport.max());
  (
    min_world.x.min(max_world.x),
    min_world.x.max(max_world.x),
    min_world.y.min(max_world.y),
    min_world.y.max(max_world.y),
  )
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

fn draw_geometry(
  geometry: &Geometry<PixelCoordinate>,
  viewport: MapViewport,
  window: &Window,
  heading_style: HeadingStyle,
  gizmos: &mut Gizmos<BevyGeometryGizmos>,
  stats: &mut GeometryStats,
) {
  if !geometry_intersects_viewport(geometry, viewport) {
    return;
  }

  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        draw_geometry(geometry, viewport, window, heading_style, gizmos, stats);
      }
    }
    Geometry::Point(coord, metadata) => {
      let screens = coordinate_screen_positions(*coord, viewport);
      if screens.is_empty() {
        return;
      }

      let color = style_color(metadata.style.as_ref());
      for screen in &screens {
        let center = screen_to_bevy(*screen, window);
        gizmos.circle_2d(center, POINT_RADIUS, color);
        if let Some(heading) = metadata.heading {
          draw_heading(gizmos, center, heading, color, heading_style);
        }
      }

      stats.geometries += 1;
      stats.points += screens.len();
    }
    Geometry::LineString(coords, metadata) => {
      let color = style_color(metadata.style.as_ref());
      stats.geometries += 1;
      stats.line_segments += draw_lines(coords, false, viewport, window, color, gizmos);
    }
    Geometry::Polygon(coords, metadata) => {
      let color = style_color(metadata.style.as_ref());
      stats.geometries += 1;
      stats.polygons += 1;
      stats.line_segments += draw_lines(coords, true, viewport, window, color, gizmos);
    }
    Geometry::Heatmap(coords, metadata) => {
      let color = style_color(metadata.style.as_ref());
      stats.geometries += 1;
      for coord in coords.iter().take(MAX_HEATMAP_POINTS_PER_FRAME) {
        for screen in coordinate_screen_positions(*coord, viewport) {
          gizmos.circle_2d(screen_to_bevy(screen, window), 1.5, color);
          stats.heatmap_points += 1;
        }
      }
    }
  }
}

fn draw_lines(
  coords: &[PixelCoordinate],
  closed: bool,
  viewport: MapViewport,
  window: &Window,
  color: Color,
  gizmos: &mut Gizmos<BevyGeometryGizmos>,
) -> usize {
  if coords.len() < 2 {
    return 0;
  }

  let mut drawn_segments = 0;
  for segment in coords.windows(2) {
    drawn_segments += draw_segment(segment[0], segment[1], viewport, window, color, gizmos);
  }

  if closed && let (Some(first), Some(last)) = (coords.first(), coords.last()) {
    drawn_segments += draw_segment(*last, *first, viewport, window, color, gizmos);
  }

  drawn_segments
}

fn draw_segment(
  start: PixelCoordinate,
  end: PixelCoordinate,
  viewport: MapViewport,
  window: &Window,
  color: Color,
  gizmos: &mut Gizmos<BevyGeometryGizmos>,
) -> usize {
  let screen_segments = segment_screen_positions(start, end, viewport);
  for (screen_start, screen_end) in &screen_segments {
    gizmos.line_2d(
      screen_to_bevy(*screen_start, window),
      screen_to_bevy(*screen_end, window),
      color,
    );
  }

  screen_segments.len()
}

fn draw_highlighted_geometry(
  geometry: &Geometry<PixelCoordinate>,
  viewport: MapViewport,
  window: &Window,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  if !geometry_intersects_viewport(geometry, viewport) {
    return;
  }

  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        draw_highlighted_geometry(geometry, viewport, window, gizmos);
      }
    }
    Geometry::Point(coord, metadata) => {
      let screens = coordinate_screen_positions(*coord, viewport);
      if screens.is_empty() {
        return;
      }
      let color = highlight_color(metadata.style.as_ref());
      for screen in screens {
        gizmos.circle_2d(
          screen_to_bevy(screen, window),
          HIGHLIGHT_POINT_RADIUS,
          color,
        );
      }
    }
    Geometry::LineString(coords, metadata) => {
      draw_highlighted_lines(
        coords,
        false,
        viewport,
        window,
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
  color: Color,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  if coords.len() < 2 {
    return;
  }

  for segment in coords.windows(2) {
    draw_highlighted_segment(segment[0], segment[1], viewport, window, color, gizmos);
  }

  if closed && let (Some(first), Some(last)) = (coords.first(), coords.last()) {
    draw_highlighted_segment(*last, *first, viewport, window, color, gizmos);
  }
}

fn draw_highlighted_segment(
  start: PixelCoordinate,
  end: PixelCoordinate,
  viewport: MapViewport,
  window: &Window,
  color: Color,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  for (screen_start, screen_end) in segment_screen_positions(start, end, viewport) {
    gizmos.line_2d(
      screen_to_bevy(screen_start, window),
      screen_to_bevy(screen_end, window),
      color,
    );
  }
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

fn segment_screen_positions(
  start: PixelCoordinate,
  end: PixelCoordinate,
  viewport: MapViewport,
) -> Vec<(PixelPosition, PixelPosition)> {
  if !start.is_valid() || !end.is_valid() {
    return Vec::new();
  }

  visible_wrap_offsets(start.x.min(end.x), start.x.max(end.x), viewport)
    .into_iter()
    .filter_map(|wrap_offset| {
      let screen_start = viewport.transform.apply(PixelCoordinate {
        x: start.x + wrap_offset,
        y: start.y,
      });
      let screen_end = viewport.transform.apply(PixelCoordinate {
        x: end.x + wrap_offset,
        y: end.y,
      });
      segment_intersects_rect(screen_start, screen_end, viewport.rect)
        .then_some((screen_start, screen_end))
    })
    .collect()
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
) -> bool {
  let bbox = geometry.bounding_box();
  if !bbox.is_valid() {
    return false;
  }

  let (_, _, min_y, max_y) = viewport_world_bounds(viewport);
  if bbox.max_y() < min_y || bbox.min_y() > max_y {
    return false;
  }

  !visible_wrap_offsets(bbox.min_x(), bbox.max_x(), viewport).is_empty()
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
