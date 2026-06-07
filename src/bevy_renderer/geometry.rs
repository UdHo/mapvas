use crate::{
  config::{Config, HeadingStyle},
  map::{
    color::Color as MapColor,
    coordinates::{
      CANVAS_SIZE, PixelCoordinate, PixelPosition, PixelRect, Tile, TileCoordinate,
      tile_zoom_for_transform, tiles_in_box,
    },
    geometry_collection::{DEFAULT_STYLE, Geometry, Style},
    mapvas_egui::layer::geometry_rasterizer,
    viewport::{GeometrySnapshot, MapViewport},
  },
};
use std::collections::HashMap;

use bevy::{
  asset::RenderAssetUsages,
  mesh::{Indices, PrimitiveTopology},
  prelude::*,
  render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rstar::{AABB, RTree, RTreeObject};

use super::{map::BevyMapViewport, surface::BevyRenderSurface};

const HIGHLIGHT_POINT_RADIUS: f32 = 10.0;
const HIGHLIGHT_STROKE_WIDTH: f32 = 6.0;
const HIGHLIGHT_FILL_Z: f32 = -4.5;
const GEOMETRY_TILE_PIXEL_SIZE: u32 = 512;
const GEOMETRY_TILE_Z: f32 = -4.8;

pub struct BevyGeometryPlugin;

impl Plugin for BevyGeometryPlugin {
  fn build(&self, app: &mut App) {
    app
      .init_gizmo_group::<BevyHighlightGizmos>()
      .add_systems(Startup, configure_bevy_geometry_gizmos)
      .add_systems(Update, draw_bevy_geometry);
  }
}

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
  geometry_tile_entries: HashMap<Tile, GeometryTileEntry>,
  geometry_tile_index: RTree<GeometryTileIndexEntry>,
  geometry_tile_index_version: u64,
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
      geometry_tile_entries: HashMap::new(),
      geometry_tile_index: RTree::new(),
      geometry_tile_index_version: u64::MAX,
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

#[derive(Component)]
struct BevyGeometryTileSprite;

struct GeometryTileEntry {
  image: Handle<Image>,
  entities: Vec<Entity>,
}

struct GeometryTileIndexEntry {
  geometry_index: usize,
  envelope: AABB<[f32; 2]>,
}

impl RTreeObject for GeometryTileIndexEntry {
  type Envelope = AABB<[f32; 2]>;

  fn envelope(&self) -> Self::Envelope {
    self.envelope
  }
}

struct PolygonFillSpec {
  mesh: Mesh,
  color: Color,
  bounds: GeometryBounds,
  origin: PixelCoordinate,
}

struct PolygonFillInstance {
  mesh: Handle<Mesh>,
  material: Handle<ColorMaterial>,
  bounds: GeometryBounds,
  origin: PixelCoordinate,
  entities: Vec<Entity>,
}

fn configure_bevy_geometry_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
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
  surface: Res<BevyRenderSurface>,
  mut images: ResMut<Assets<Image>>,
  mut meshes: ResMut<Assets<Mesh>>,
  mut materials: ResMut<Assets<ColorMaterial>>,
  mut geometry_tile_sprites: Query<
    (Entity, &mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyGeometryTileSprite>,
      Without<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
  mut fills: Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
      Without<BevyPointFill>,
      Without<BevyGeometryTileSprite>,
    ),
  >,
  mut highlight_fills: Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyHighlightPolygonFill>,
      Without<BevyPolygonFill>,
      Without<BevyPointFill>,
      Without<BevyGeometryTileSprite>,
    ),
  >,
  mut highlight_gizmos: Gizmos<BevyHighlightGizmos>,
) {
  let draw_stats = GeometryStats::default();
  if !layer.enabled {
    hide_geometry_tile_sprites(&mut geometry_tile_sprites);
    set_fill_visibility(&mut fills, false);
    set_highlight_fill_visibility(&mut highlight_fills, false);
    layer.draw_stats = draw_stats;
    return;
  }
  let Some(viewport) = viewport.get() else {
    hide_geometry_tile_sprites(&mut geometry_tile_sprites);
    set_fill_visibility(&mut fills, false);
    set_highlight_fill_visibility(&mut highlight_fills, false);
    layer.draw_stats = draw_stats;
    return;
  };
  let surface = *surface;

  rebuild_geometry_tile_index_if_needed(&mut commands, &mut layer);
  update_geometry_tiles(
    &mut commands,
    viewport,
    surface,
    &mut images,
    &mut layer,
    &mut geometry_tile_sprites,
  );
  set_fill_visibility(&mut fills, false);
  rebuild_highlight_polygon_fills_if_needed(&mut commands, &mut meshes, &mut materials, &mut layer);
  update_highlight_polygon_fills(
    &mut commands,
    viewport,
    surface,
    &mut layer.highlight_polygon_fill_instances,
    &mut highlight_fills,
  );

  for geometry in &layer.highlighted_geometries {
    draw_highlighted_geometry(geometry, viewport, surface, &mut highlight_gizmos);
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
      Without<BevyGeometryTileSprite>,
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
      Without<BevyGeometryTileSprite>,
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

fn hide_geometry_tile_sprites(
  sprites: &mut Query<
    (Entity, &mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyGeometryTileSprite>,
      Without<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
) {
  for (_, _, _, mut visibility) in sprites.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn rebuild_geometry_tile_index_if_needed(commands: &mut Commands, layer: &mut BevyGeometryLayer) {
  if layer.geometry_tile_index_version == layer.geometries_version {
    return;
  }

  for (_, entry) in layer.geometry_tile_entries.drain() {
    for entity in entry.entities {
      commands.entity(entity).despawn();
    }
  }

  let entries = layer
    .geometries
    .iter()
    .enumerate()
    .filter_map(|(geometry_index, geometry)| {
      let bbox = geometry.bounding_box();
      bbox.is_valid().then(|| GeometryTileIndexEntry {
        geometry_index,
        envelope: AABB::from_corners([bbox.min_x(), bbox.min_y()], [bbox.max_x(), bbox.max_y()]),
      })
    })
    .collect::<Vec<_>>();

  layer.geometry_tile_index = RTree::bulk_load(entries);
  layer.geometry_tile_index_version = layer.geometries_version;
}

fn update_geometry_tiles(
  commands: &mut Commands,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  images: &mut Assets<Image>,
  layer: &mut BevyGeometryLayer,
  sprites: &mut Query<
    (Entity, &mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyGeometryTileSprite>,
      Without<BevyPolygonFill>,
      Without<BevyHighlightPolygonFill>,
      Without<BevyPointFill>,
    ),
  >,
) {
  let visible_tiles = visible_geometry_tiles(viewport);
  for tile in &visible_tiles {
    if !layer.geometry_tile_entries.contains_key(tile) {
      if let Some(image) = rasterize_geometry_tile(layer, *tile) {
        layer.geometry_tile_entries.insert(
          *tile,
          GeometryTileEntry {
            image: images.add(image),
            entities: Vec::new(),
          },
        );
      }
    }
  }

  let mut visible_set = std::collections::HashSet::new();
  for tile in visible_tiles {
    let Some(entry) = layer.geometry_tile_entries.get_mut(&tile) else {
      continue;
    };
    visible_set.insert(tile);
    let tile_rects = geometry_tile_screen_rects(viewport, tile);
    let mut visible_entity_count = 0;
    for tile_rect in tile_rects {
      let transform = geometry_tile_transform(tile_rect, surface);
      let custom_size = Vec2::new(tile_rect.width(), tile_rect.height());
      let image = entry.image.clone();

      if let Some(entity) = entry.entities.get(visible_entity_count).copied() {
        if let Ok((_, mut sprite_transform, mut sprite, mut visibility)) = sprites.get_mut(entity) {
          *sprite_transform = transform;
          sprite.image = image;
          sprite.custom_size = Some(custom_size);
          *visibility = Visibility::Visible;
        } else {
          entry.entities[visible_entity_count] =
            spawn_geometry_tile_sprite(commands, image, transform, custom_size);
        }
      } else {
        entry.entities.push(spawn_geometry_tile_sprite(
          commands,
          image,
          transform,
          custom_size,
        ));
      }

      visible_entity_count += 1;
    }

    for entity in entry.entities.iter().skip(visible_entity_count).copied() {
      if let Ok((_, _, _, mut visibility)) = sprites.get_mut(entity) {
        *visibility = Visibility::Hidden;
      }
    }
  }

  for (tile, entry) in &mut layer.geometry_tile_entries {
    if visible_set.contains(tile) {
      continue;
    }
    for entity in &entry.entities {
      if let Ok((_, _, _, mut visibility)) = sprites.get_mut(*entity) {
        *visibility = Visibility::Hidden;
      }
    }
  }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn visible_geometry_tiles(viewport: MapViewport) -> Vec<Tile> {
  let zoom = tile_zoom_for_transform(&viewport.transform).min(19);
  let inv = viewport.transform.invert();
  let min_pos = TileCoordinate::from_pixel_position(inv.apply(viewport.min()), zoom);
  let max_pos = TileCoordinate::from_pixel_position(inv.apply(viewport.max()), zoom);
  tiles_in_box(min_pos, max_pos).collect()
}

fn rasterize_geometry_tile(layer: &BevyGeometryLayer, tile: Tile) -> Option<Image> {
  let (nw, se) = tile.position();
  let query = AABB::from_corners([nw.x, nw.y], [se.x, se.y]);
  let geometries = layer
    .geometry_tile_index
    .locate_in_envelope_intersecting(query)
    .filter_map(|entry| layer.geometries.get(entry.geometry_index))
    .collect::<Vec<_>>();
  if geometries.is_empty() {
    return None;
  }

  let tile_transform = geometry_tile_raster_transform(tile);
  #[allow(clippy::cast_precision_loss)]
  let tile_rect = PixelRect::from_min_size(
    PixelPosition { x: 0.0, y: 0.0 },
    PixelPosition {
      x: GEOMETRY_TILE_PIXEL_SIZE as f32,
      y: GEOMETRY_TILE_PIXEL_SIZE as f32,
    },
  );
  let pixmap = geometry_rasterizer::rasterize_geometries(
    geometries.into_iter(),
    &tile_transform,
    tile_rect,
    layer.heading_style,
  )?;

  Some(pixmap_to_bevy_image(&pixmap))
}

fn geometry_tile_raster_transform(tile: Tile) -> crate::map::coordinates::Transform {
  let (nw, se) = tile.position();
  let world_size = se.x - nw.x;
  #[allow(clippy::cast_precision_loss)]
  let zoom_factor = GEOMETRY_TILE_PIXEL_SIZE as f32 / world_size;

  crate::map::coordinates::Transform::default()
    .zoomed(zoom_factor)
    .translated(PixelPosition {
      x: -nw.x * zoom_factor,
      y: -nw.y * zoom_factor,
    })
}

fn pixmap_to_bevy_image(pixmap: &tiny_skia::Pixmap) -> Image {
  let mut straight = Vec::with_capacity(pixmap.data().len());
  for pixel in pixmap.data().chunks_exact(4) {
    let alpha = pixel[3];
    if alpha == 0 {
      straight.extend_from_slice(&[0, 0, 0, 0]);
      continue;
    }

    let inv_alpha = 255.0_f32 / f32::from(alpha);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
      straight.push((f32::from(pixel[0]) * inv_alpha).min(255.0) as u8);
      straight.push((f32::from(pixel[1]) * inv_alpha).min(255.0) as u8);
      straight.push((f32::from(pixel[2]) * inv_alpha).min(255.0) as u8);
    }
    straight.push(alpha);
  }

  Image::new(
    Extent3d {
      width: pixmap.width(),
      height: pixmap.height(),
      depth_or_array_layers: 1,
    },
    TextureDimension::D2,
    straight,
    TextureFormat::Rgba8UnormSrgb,
    RenderAssetUsages::RENDER_WORLD,
  )
}

fn geometry_tile_screen_rects(viewport: MapViewport, tile: Tile) -> Vec<PixelRect> {
  let (nw, se) = tile.position();
  let wrap_offsets = visible_wrap_offsets(nw.x, se.x, viewport);
  wrap_offsets
    .into_iter()
    .filter_map(|wrap_offset| {
      let shifted_nw = PixelCoordinate {
        x: nw.x + wrap_offset,
        y: nw.y,
      };
      let shifted_se = PixelCoordinate {
        x: se.x + wrap_offset,
        y: se.y,
      };
      let rect = PixelRect::from_min_max(
        viewport.transform.apply(shifted_nw),
        viewport.transform.apply(shifted_se),
      );
      rect.intersects(viewport.rect).then_some(rect)
    })
    .collect()
}

fn geometry_tile_transform(rect: PixelRect, surface: BevyRenderSurface) -> Transform {
  let center = rect.center();
  let bevy_pos = screen_to_bevy(center, surface);
  Transform::from_xyz(bevy_pos.x, bevy_pos.y, GEOMETRY_TILE_Z)
}

fn spawn_geometry_tile_sprite(
  commands: &mut Commands,
  image: Handle<Image>,
  transform: Transform,
  custom_size: Vec2,
) -> Entity {
  let mut sprite = Sprite::from_image(image);
  sprite.custom_size = Some(custom_size);
  commands
    .spawn((sprite, transform, BevyGeometryTileSprite))
    .id()
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

fn update_highlight_polygon_fills(
  commands: &mut Commands,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  fill_instances: &mut [PolygonFillInstance],
  fills: &mut Query<
    (&mut Transform, &mut Visibility),
    (
      With<BevyHighlightPolygonFill>,
      Without<BevyPolygonFill>,
      Without<BevyPointFill>,
      Without<BevyGeometryTileSprite>,
    ),
  >,
) {
  for instance in fill_instances {
    let wrap_offsets = bounds_visible_wrap_offsets(instance.bounds, viewport);
    for (entity_index, wrap_offset) in wrap_offsets.iter().copied().enumerate() {
      let transform = polygon_fill_transform(
        viewport,
        surface,
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
  surface: BevyRenderSurface,
  origin: PixelCoordinate,
  wrap_offset: f32,
  z: f32,
) -> Transform {
  Transform::from_xyz(
    viewport.transform.trans.x + (origin.x + wrap_offset) * viewport.transform.zoom
      - surface.width() / 2.0,
    surface.height() / 2.0 - viewport.transform.trans.y - origin.y * viewport.transform.zoom,
    z,
  )
  .with_scale(Vec3::new(
    viewport.transform.zoom,
    -viewport.transform.zoom,
    1.0,
  ))
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

fn draw_highlighted_geometry(
  geometry: &Geometry<PixelCoordinate>,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  if !geometry_intersects_viewport(geometry, viewport) {
    return;
  }

  match geometry {
    Geometry::GeometryCollection(geometries, _) => {
      for geometry in geometries {
        draw_highlighted_geometry(geometry, viewport, surface, gizmos);
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
          screen_to_bevy(screen, surface),
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
        surface,
        highlight_color(metadata.style.as_ref()),
        gizmos,
      );
    }
    Geometry::Polygon(coords, metadata) => {
      draw_highlighted_lines(
        coords,
        true,
        viewport,
        surface,
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
  surface: BevyRenderSurface,
  color: Color,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  if coords.len() < 2 {
    return;
  }

  for segment in coords.windows(2) {
    draw_highlighted_segment(segment[0], segment[1], viewport, surface, color, gizmos);
  }

  if closed && let (Some(first), Some(last)) = (coords.first(), coords.last()) {
    draw_highlighted_segment(*last, *first, viewport, surface, color, gizmos);
  }
}

fn draw_highlighted_segment(
  start: PixelCoordinate,
  end: PixelCoordinate,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  color: Color,
  gizmos: &mut Gizmos<BevyHighlightGizmos>,
) {
  for (screen_start, screen_end) in segment_screen_positions(start, end, viewport) {
    gizmos.line_2d(
      screen_to_bevy(screen_start, surface),
      screen_to_bevy(screen_end, surface),
      color,
    );
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

fn screen_to_bevy(screen: PixelPosition, surface: BevyRenderSurface) -> Vec2 {
  Vec2::new(
    screen.x - surface.width() / 2.0,
    surface.height() / 2.0 - screen.y,
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
