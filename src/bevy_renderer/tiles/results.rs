use crate::map::tile_renderer::TileImage;
use bevy::{
  asset::RenderAssetUsages,
  prelude::*,
  render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::{
  BevyNativeVectorTileEntry, BevyTileEntry, BevyTileLayer, BevyTileResult,
  NativeVectorTileLabelInstance, NativeVectorTileMeshInstance,
};

pub(super) fn collect_finished_tiles(
  mut layer: ResMut<BevyTileLayer>,
  mut images: ResMut<Assets<Image>>,
  mut meshes: ResMut<Assets<Mesh>>,
  mut materials: ResMut<Assets<ColorMaterial>>,
) {
  let mut results = Vec::new();
  if let Ok(receiver) = layer.receiver.lock() {
    while let Ok(result) = receiver.try_recv() {
      results.push(result);
    }
  }

  for result in results {
    match result {
      BevyTileResult::Ready {
        generation,
        tile,
        image,
      } => {
        if generation != layer.generation {
          continue;
        }
        layer.in_flight_tiles.remove(&tile);
        let image = images.add(tile_image_to_bevy(image));
        if let Some(entry) = layer.loaded_tiles.get_mut(&tile) {
          entry.image = image;
        } else {
          layer.loaded_tiles.insert(
            tile,
            BevyTileEntry {
              image,
              entities: Vec::new(),
            },
          );
        }
      }
      BevyTileResult::NativeVectorReady {
        generation,
        tile,
        meshes: mesh_specs,
        labels: label_specs,
        debug_features,
      } => {
        if generation != layer.generation {
          continue;
        }
        layer.in_flight_tiles.remove(&tile);
        let instances = mesh_specs
          .into_iter()
          .map(|spec| NativeVectorTileMeshInstance {
            mesh: meshes.add(spec.mesh),
            material: materials.add(ColorMaterial::from(spec.color)),
            bounds: spec.bounds,
            origin: spec.origin,
            z: spec.z,
            entities: Vec::new(),
          })
          .collect::<Vec<_>>();
        let labels = label_specs
          .into_iter()
          .map(|spec| NativeVectorTileLabelInstance {
            coord: spec.coord,
            text: spec.text,
            base_font_size: spec.base_font_size,
            text_color: spec.text_color,
            show_marker: spec.show_marker,
            centered: spec.centered,
            max_point_zoom: spec.max_point_zoom,
            tile_world_size: spec.tile_world_size,
            entities: Vec::new(),
          })
          .collect::<Vec<_>>();
        if let Some(entry) = layer.loaded_native_vector_tiles.insert(
          tile,
          BevyNativeVectorTileEntry {
            instances,
            labels,
            debug_features,
          },
        ) {
          layer.stale_native_vector_entities.extend(
            entry
              .instances
              .into_iter()
              .flat_map(|instance| instance.entities)
              .chain(entry.labels.into_iter().flat_map(|label| {
                label
                  .entities
                  .into_iter()
                  .flat_map(|copy| [copy.marker, copy.text])
              })),
          );
        }
      }
      BevyTileResult::Failed { generation, tile } => {
        if generation != layer.generation {
          continue;
        }
        layer.in_flight_tiles.remove(&tile);
      }
    }
  }
}

fn tile_image_to_bevy(image: TileImage) -> Image {
  Image::new(
    Extent3d {
      width: image.size[0] as u32,
      height: image.size[1] as u32,
      depth_or_array_layers: 1,
    },
    TextureDimension::D2,
    image.rgba,
    TextureFormat::Rgba8UnormSrgb,
    RenderAssetUsages::RENDER_WORLD,
  )
}
