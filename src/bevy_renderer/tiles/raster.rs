use std::collections::HashSet;

use crate::map::{
  coordinates::{PixelPosition, PixelRect, Tile, TilePriority},
  viewport::MapViewport,
};
use bevy::{prelude::*, sprite::Anchor};

use super::super::{map::BevyMapViewport, surface::BevyRenderSurface};
use super::{
  BevyGridTileSprite, BevyOsmAttributionBackground, BevyOsmAttributionText,
  BevyTileLabelBackground, BevyTileLabelText, BevyTileLayer, BevyTileOverlayGizmos,
  BevyTileRuntime, BevyTileSprite, CoordinateDisplayMode,
  screen::{
    coordinate_tile_rects, screen_to_bevy_2d, transform_for_screen_pos, transform_for_screen_rect,
    transform_for_screen_rect_at_z,
  },
};

const TILE_LABEL_WIDTH: f32 = 100.0;
const TILE_LABEL_HEIGHT: f32 = 60.0;
const TILE_LABEL_TEXT_OFFSET_X: f32 = 8.0;
const TILE_LABEL_TEXT_OFFSET_Y: f32 = 8.0;
const TILE_LABEL_FONT_SIZE: f32 = 11.0;
const TILE_LABEL_BACKGROUND_Z: f32 = 40.0;
const TILE_LABEL_TEXT_Z: f32 = 41.0;
const OSM_ATTRIBUTION_TEXT: &str = "© OpenStreetMap contributors";
const OSM_ATTRIBUTION_WIDTH: f32 = 158.0;
const OSM_ATTRIBUTION_HEIGHT: f32 = 20.0;
const OSM_ATTRIBUTION_MARGIN: f32 = 8.0;
const OSM_ATTRIBUTION_FONT_SIZE: f32 = 11.0;
const OSM_ATTRIBUTION_BACKGROUND_Z: f32 = 50.0;
const OSM_ATTRIBUTION_TEXT_Z: f32 = 51.0;

struct TileLabel {
  background_rect: PixelRect,
  background_size: Vec2,
  text_pos: PixelPosition,
  text: String,
}

pub(super) fn update_bevy_tiles(
  mut commands: Commands,
  mut layer: ResMut<BevyTileLayer>,
  viewport: Res<BevyMapViewport>,
  runtime: Res<BevyTileRuntime>,
  surface: Res<BevyRenderSurface>,
  mut sprites: Query<(Entity, &mut Transform, &mut Sprite, &mut Visibility), With<BevyTileSprite>>,
  mut grid_sprites: Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (With<BevyGridTileSprite>, Without<BevyTileSprite>),
  >,
  mut label_backgrounds: Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyTileLabelBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelText>,
    ),
  >,
  mut label_texts: Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyTileLabelText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
    ),
  >,
  mut attribution_backgrounds: Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyOsmAttributionBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionText>,
    ),
  >,
  mut attribution_texts: Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyOsmAttributionText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionBackground>,
    ),
  >,
  mut overlay_gizmos: Gizmos<BevyTileOverlayGizmos>,
) {
  let stale_entities = layer.stale_entities.drain(..).collect::<HashSet<_>>();
  for entity in &stale_entities {
    commands.entity(*entity).despawn();
  }

  if !layer.visible {
    hide_tile_sprites(&mut sprites);
    despawn_untracked_tile_sprites(
      &mut commands,
      &tracked_tile_entities(&layer),
      &stale_entities,
      &mut sprites,
    );
    hide_grid_tile_sprites(&mut grid_sprites);
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
    hide_osm_attribution(&mut attribution_backgrounds, &mut attribution_texts);
    return;
  }

  let Some(viewport) = viewport.get() else {
    hide_tile_sprites(&mut sprites);
    despawn_untracked_tile_sprites(
      &mut commands,
      &tracked_tile_entities(&layer),
      &stale_entities,
      &mut sprites,
    );
    hide_grid_tile_sprites(&mut grid_sprites);
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
    hide_osm_attribution(&mut attribution_backgrounds, &mut attribution_texts);
    return;
  };
  let surface = *surface;

  let visible_tiles = layer.visible_tiles(viewport);
  let runtime_handle = runtime.0.clone();
  let native_vector_active = layer.native_vector_tiles_active();
  let show_osm_attribution = layer
    .tile_loader()
    .is_some_and(|loader| loader.requires_osm_attribution())
    && (!layer.loaded_tiles.is_empty() || !layer.loaded_native_vector_tiles.is_empty())
    && layer.coordinate_display_mode != CoordinateDisplayMode::GridOnly;

  if !native_vector_active {
    for tile in &visible_tiles {
      if !layer.loaded_tiles.contains_key(tile) {
        layer.request_tile(*tile, TilePriority::Current, runtime_handle.clone());
      }
    }
  }

  if layer.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
    if native_vector_active {
      layer.request_preload_native_vector_tiles(&visible_tiles, runtime_handle);
    } else {
      layer.request_preload_tiles(&visible_tiles, runtime_handle);
    }
    hide_tile_sprites(&mut sprites);
    update_grid_tile_sprites(
      &mut commands,
      viewport,
      surface,
      &visible_tiles,
      &mut grid_sprites,
    );
    draw_coordinate_tile_borders(
      viewport,
      surface,
      &visible_tiles,
      layer.coordinate_display_mode,
      &mut overlay_gizmos,
    );
    update_tile_labels(
      &mut commands,
      viewport,
      surface,
      &visible_tiles,
      &mut label_backgrounds,
      &mut label_texts,
    );
    update_osm_attribution(
      &mut commands,
      viewport,
      surface,
      show_osm_attribution,
      &mut attribution_backgrounds,
      &mut attribution_texts,
    );
    return;
  }
  hide_grid_tile_sprites(&mut grid_sprites);

  draw_coordinate_tile_borders(
    viewport,
    surface,
    &visible_tiles,
    layer.coordinate_display_mode,
    &mut overlay_gizmos,
  );
  if layer.coordinate_display_mode == CoordinateDisplayMode::Off {
    hide_tile_labels(&mut label_backgrounds, &mut label_texts);
  } else {
    update_tile_labels(
      &mut commands,
      viewport,
      surface,
      &visible_tiles,
      &mut label_backgrounds,
      &mut label_texts,
    );
  }
  update_osm_attribution(
    &mut commands,
    viewport,
    surface,
    show_osm_attribution,
    &mut attribution_backgrounds,
    &mut attribution_texts,
  );
  if native_vector_active {
    hide_tile_sprites(&mut sprites);
    return;
  }

  let mut tiles_to_draw = visible_tiles
    .iter()
    .filter_map(|tile| loaded_tile_or_parent(&layer, *tile))
    .collect::<Vec<_>>();
  tiles_to_draw.sort_unstable_by_key(|tile| tile.zoom);
  tiles_to_draw.dedup();
  tiles_to_draw.reverse();

  let draw_set: HashSet<Tile> = tiles_to_draw.iter().copied().collect();
  for tile in &tiles_to_draw {
    let tile = *tile;
    let tile_rects = coordinate_tile_rects(viewport, tile);
    let tint = tile_tint(layer.coordinate_display_mode, tile);

    let entry = layer.loaded_tiles.get_mut(&tile).expect("tile was checked");
    let mut visible_entity_count = 0;

    for tile_rect in tile_rects {
      let transform = transform_for_screen_rect(tile_rect, surface, tile.zoom);
      let custom_size = Vec2::new(tile_rect.width(), tile_rect.height());
      let image = entry.image.clone();

      if let Some(entity) = entry.entities.get(visible_entity_count).copied() {
        if let Ok((_, mut sprite_transform, mut sprite, mut visibility)) = sprites.get_mut(entity) {
          *sprite_transform = transform;
          sprite.image = image;
          sprite.custom_size = Some(custom_size);
          sprite.color = tint;
          *visibility = Visibility::Visible;
        } else {
          entry.entities[visible_entity_count] =
            spawn_tile_sprite(&mut commands, image, transform, custom_size, tint);
        }
      } else {
        entry.entities.push(spawn_tile_sprite(
          &mut commands,
          image,
          transform,
          custom_size,
          tint,
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

  layer.request_preload_tiles(&visible_tiles, runtime_handle);

  for (tile, entry) in &mut layer.loaded_tiles {
    if draw_set.contains(tile) {
      continue;
    }
    for entity in &entry.entities {
      if let Ok((_, _, _, mut visibility)) = sprites.get_mut(*entity) {
        *visibility = Visibility::Hidden;
      }
    }
  }

  despawn_untracked_tile_sprites(
    &mut commands,
    &tracked_tile_entities(&layer),
    &stale_entities,
    &mut sprites,
  );
}

fn hide_tile_sprites(
  sprites: &mut Query<(Entity, &mut Transform, &mut Sprite, &mut Visibility), With<BevyTileSprite>>,
) {
  for (_, _, _, mut visibility) in sprites.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn tracked_tile_entities(layer: &BevyTileLayer) -> HashSet<Entity> {
  layer
    .loaded_tiles
    .values()
    .flat_map(|entry| entry.entities.iter().copied())
    .collect()
}

fn spawn_tile_sprite(
  commands: &mut Commands,
  image: Handle<Image>,
  transform: Transform,
  custom_size: Vec2,
  tint: Color,
) -> Entity {
  let mut sprite = Sprite::from_image(image);
  sprite.custom_size = Some(custom_size);
  sprite.color = tint;
  commands.spawn((sprite, transform, BevyTileSprite)).id()
}

fn despawn_untracked_tile_sprites(
  commands: &mut Commands,
  tracked_entities: &HashSet<Entity>,
  stale_entities: &HashSet<Entity>,
  sprites: &mut Query<(Entity, &mut Transform, &mut Sprite, &mut Visibility), With<BevyTileSprite>>,
) {
  for (entity, _, _, mut visibility) in sprites.iter_mut() {
    if tracked_entities.contains(&entity) || stale_entities.contains(&entity) {
      continue;
    }
    *visibility = Visibility::Hidden;
    commands.entity(entity).despawn();
  }
}

fn hide_grid_tile_sprites(
  sprites: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (With<BevyGridTileSprite>, Without<BevyTileSprite>),
  >,
) {
  for (_, _, mut visibility) in sprites.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn hide_tile_labels(
  backgrounds: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyTileLabelBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelText>,
    ),
  >,
  texts: &mut Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyTileLabelText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
    ),
  >,
) {
  for (_, _, mut visibility) in backgrounds.iter_mut() {
    *visibility = Visibility::Hidden;
  }
  for (_, _, mut visibility) in texts.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn hide_osm_attribution(
  backgrounds: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyOsmAttributionBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionText>,
    ),
  >,
  texts: &mut Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyOsmAttributionText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionBackground>,
    ),
  >,
) {
  for (_, _, mut visibility) in backgrounds.iter_mut() {
    *visibility = Visibility::Hidden;
  }
  for (_, _, mut visibility) in texts.iter_mut() {
    *visibility = Visibility::Hidden;
  }
}

fn update_grid_tile_sprites(
  commands: &mut Commands,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  visible_tiles: &[Tile],
  sprites: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (With<BevyGridTileSprite>, Without<BevyTileSprite>),
  >,
) {
  let mut sprite_iter = sprites.iter_mut();
  for tile in visible_tiles {
    for tile_rect in coordinate_tile_rects(viewport, *tile) {
      let transform = transform_for_screen_rect(tile_rect, surface, tile.zoom);
      let custom_size = Vec2::new(tile_rect.width(), tile_rect.height());
      let color = grid_tile_color(*tile);

      if let Some((mut sprite_transform, mut sprite, mut visibility)) = sprite_iter.next() {
        *sprite_transform = transform;
        sprite.custom_size = Some(custom_size);
        sprite.color = color;
        *visibility = Visibility::Visible;
      } else {
        commands.spawn((
          Sprite::from_color(color, custom_size),
          transform,
          BevyGridTileSprite,
        ));
      }
    }
  }

  for (_, _, mut visibility) in sprite_iter {
    *visibility = Visibility::Hidden;
  }
}

fn update_tile_labels(
  commands: &mut Commands,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  visible_tiles: &[Tile],
  backgrounds: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyTileLabelBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelText>,
    ),
  >,
  texts: &mut Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyTileLabelText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
    ),
  >,
) {
  let mut background_iter = backgrounds.iter_mut();
  let mut text_iter = texts.iter_mut();
  for tile in visible_tiles {
    for tile_rect in coordinate_tile_rects(viewport, *tile) {
      let Some(label) = tile_label(*tile, tile_rect) else {
        continue;
      };

      if let Some((mut transform, mut sprite, mut visibility)) = background_iter.next() {
        *transform =
          transform_for_screen_rect_at_z(label.background_rect, surface, TILE_LABEL_BACKGROUND_Z);
        sprite.custom_size = Some(label.background_size);
        sprite.color = tile_label_background_color();
        *visibility = Visibility::Visible;
      } else {
        commands.spawn((
          Sprite::from_color(tile_label_background_color(), label.background_size),
          transform_for_screen_rect_at_z(label.background_rect, surface, TILE_LABEL_BACKGROUND_Z),
          BevyTileLabelBackground,
        ));
      }

      if let Some((mut transform, mut text, mut visibility)) = text_iter.next() {
        *transform = transform_for_screen_pos(label.text_pos, surface, TILE_LABEL_TEXT_Z);
        text.0 = label.text;
        *visibility = Visibility::Visible;
      } else {
        commands.spawn((
          Text2d::new(label.text),
          TextFont::from_font_size(TILE_LABEL_FONT_SIZE),
          TextColor(Color::WHITE),
          TextLayout::new_with_justify(Justify::Left),
          Anchor::TOP_LEFT,
          transform_for_screen_pos(label.text_pos, surface, TILE_LABEL_TEXT_Z),
          BevyTileLabelText,
        ));
      }
    }
  }

  for (_, _, mut visibility) in background_iter {
    *visibility = Visibility::Hidden;
  }
  for (_, _, mut visibility) in text_iter {
    *visibility = Visibility::Hidden;
  }
}

fn update_osm_attribution(
  commands: &mut Commands,
  viewport: MapViewport,
  surface: BevyRenderSurface,
  visible: bool,
  backgrounds: &mut Query<
    (&mut Transform, &mut Sprite, &mut Visibility),
    (
      With<BevyOsmAttributionBackground>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionText>,
    ),
  >,
  texts: &mut Query<
    (&mut Transform, &mut Text2d, &mut Visibility),
    (
      With<BevyOsmAttributionText>,
      Without<BevyTileSprite>,
      Without<BevyGridTileSprite>,
      Without<BevyTileLabelBackground>,
      Without<BevyTileLabelText>,
      Without<BevyOsmAttributionBackground>,
    ),
  >,
) {
  if !visible {
    hide_osm_attribution(backgrounds, texts);
    return;
  }

  let rect = osm_attribution_rect(viewport.rect);
  let size = Vec2::new(OSM_ATTRIBUTION_WIDTH, OSM_ATTRIBUTION_HEIGHT);

  let mut background_iter = backgrounds.iter_mut();
  if let Some((mut transform, mut sprite, mut visibility)) = background_iter.next() {
    *transform = transform_for_screen_rect_at_z(rect, surface, OSM_ATTRIBUTION_BACKGROUND_Z);
    sprite.custom_size = Some(size);
    sprite.color = osm_attribution_background_color();
    *visibility = Visibility::Visible;
  } else {
    commands.spawn((
      Sprite::from_color(osm_attribution_background_color(), size),
      transform_for_screen_rect_at_z(rect, surface, OSM_ATTRIBUTION_BACKGROUND_Z),
      BevyOsmAttributionBackground,
    ));
  }
  for (_, _, mut visibility) in background_iter {
    *visibility = Visibility::Hidden;
  }

  let mut text_iter = texts.iter_mut();
  if let Some((mut transform, mut text, mut visibility)) = text_iter.next() {
    *transform = transform_for_screen_rect_at_z(rect, surface, OSM_ATTRIBUTION_TEXT_Z);
    text.0 = OSM_ATTRIBUTION_TEXT.to_string();
    *visibility = Visibility::Visible;
  } else {
    commands.spawn((
      Text2d::new(OSM_ATTRIBUTION_TEXT),
      TextFont::from_font_size(OSM_ATTRIBUTION_FONT_SIZE),
      TextColor(Color::srgb(35.0 / 255.0, 35.0 / 255.0, 35.0 / 255.0)),
      TextLayout::new_with_justify(Justify::Center),
      transform_for_screen_rect_at_z(rect, surface, OSM_ATTRIBUTION_TEXT_Z),
      BevyOsmAttributionText,
    ));
  }
  for (_, _, mut visibility) in text_iter {
    *visibility = Visibility::Hidden;
  }
}

fn grid_tile_color(tile: Tile) -> Color {
  if (tile.x + tile.y).is_multiple_of(2) {
    Color::srgba(1.0, 240.0 / 255.0, 240.0 / 255.0, 120.0 / 255.0)
  } else {
    Color::srgba(240.0 / 255.0, 240.0 / 255.0, 1.0, 120.0 / 255.0)
  }
}

fn tile_label(tile: Tile, tile_rect: PixelRect) -> Option<TileLabel> {
  if tile_rect.width() <= TILE_LABEL_WIDTH || tile_rect.height() <= TILE_LABEL_HEIGHT {
    return None;
  }

  let background_size = Vec2::new(TILE_LABEL_WIDTH, TILE_LABEL_HEIGHT);
  let background_min = PixelPosition {
    x: tile_rect.center().x - TILE_LABEL_WIDTH * 0.5,
    y: tile_rect.center().y - TILE_LABEL_HEIGHT * 0.5,
  };
  let background_rect = PixelRect::from_min_size(
    background_min,
    PixelPosition {
      x: TILE_LABEL_WIDTH,
      y: TILE_LABEL_HEIGHT,
    },
  );
  let text_pos = PixelPosition {
    x: background_rect.min.x + TILE_LABEL_TEXT_OFFSET_X,
    y: background_rect.min.y + TILE_LABEL_TEXT_OFFSET_Y,
  };
  Some(TileLabel {
    background_rect,
    background_size,
    text_pos,
    text: format!("Z:{}\nX:{}\nY:{}", tile.zoom, tile.x, tile.y),
  })
}

fn tile_label_background_color() -> Color {
  Color::srgba(0.0, 0.0, 0.0, 180.0 / 255.0)
}

fn osm_attribution_rect(clip_rect: PixelRect) -> PixelRect {
  PixelRect::from_min_size(
    PixelPosition {
      x: clip_rect.max.x - OSM_ATTRIBUTION_WIDTH - OSM_ATTRIBUTION_MARGIN,
      y: clip_rect.max.y - OSM_ATTRIBUTION_HEIGHT - OSM_ATTRIBUTION_MARGIN,
    },
    PixelPosition {
      x: OSM_ATTRIBUTION_WIDTH,
      y: OSM_ATTRIBUTION_HEIGHT,
    },
  )
}

fn osm_attribution_background_color() -> Color {
  Color::srgba(1.0, 1.0, 1.0, 210.0 / 255.0)
}

fn draw_coordinate_tile_borders(
  viewport: MapViewport,
  surface: BevyRenderSurface,
  visible_tiles: &[Tile],
  mode: CoordinateDisplayMode,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  if mode == CoordinateDisplayMode::Off {
    return;
  }

  let color = tile_border_color(mode);
  for tile in visible_tiles {
    for tile_rect in coordinate_tile_rects(viewport, *tile) {
      draw_tile_border(tile_rect, surface, color, gizmos);
    }
  }
}

fn draw_tile_border(
  rect: PixelRect,
  surface: BevyRenderSurface,
  color: Color,
  gizmos: &mut Gizmos<BevyTileOverlayGizmos>,
) {
  let min = rect.min;
  let max = rect.max;
  let top_left = screen_to_bevy_2d(min, surface);
  let top_right = screen_to_bevy_2d(PixelPosition { x: max.x, y: min.y }, surface);
  let bottom_right = screen_to_bevy_2d(max, surface);
  let bottom_left = screen_to_bevy_2d(PixelPosition { x: min.x, y: max.y }, surface);

  gizmos.line_2d(top_left, top_right, color);
  gizmos.line_2d(top_right, bottom_right, color);
  gizmos.line_2d(bottom_right, bottom_left, color);
  gizmos.line_2d(bottom_left, top_left, color);
}

fn tile_border_color(mode: CoordinateDisplayMode) -> Color {
  if mode == CoordinateDisplayMode::GridOnly {
    Color::srgb(80.0 / 255.0, 80.0 / 255.0, 80.0 / 255.0)
  } else {
    Color::srgb(100.0 / 255.0, 100.0 / 255.0, 100.0 / 255.0)
  }
}

fn loaded_tile_or_parent(layer: &BevyTileLayer, mut tile: Tile) -> Option<Tile> {
  loop {
    if layer.loaded_tiles.contains_key(&tile) {
      return Some(tile);
    }
    tile = tile.parent()?;
  }
}

fn tile_tint(mode: CoordinateDisplayMode, tile: Tile) -> Color {
  if mode != CoordinateDisplayMode::Overlay {
    return Color::WHITE;
  }

  if (tile.x + tile.y).is_multiple_of(2) {
    Color::srgba(1.0, 240.0 / 255.0, 240.0 / 255.0, 1.0)
  } else {
    Color::srgba(240.0 / 255.0, 240.0 / 255.0, 1.0, 1.0)
  }
}
