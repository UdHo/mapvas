use std::{collections::HashMap, sync::Arc};

use egui::{Color32, ColorImage, Rect, Ui, Widget};
use log::{error, info};

use crate::map::{
  coordinates::{TILE_SIZE, Tile, TileCoordinate, Transform, tiles_in_box},
  tile_loader::{CachedTileLoader, TileLoader},
};

use super::{Layer, LayerProperties};

/// A layer that loads and displays the map tiles.
pub struct TileLayer {
  receiver: std::sync::mpsc::Receiver<(Tile, ColorImage)>,
  sender: std::sync::mpsc::Sender<(Tile, ColorImage)>,
  tile_loader_index: usize,
  tile_loader_old_index: usize,
  all_tile_loader: Vec<Arc<CachedTileLoader>>,
  loaded_tiles: HashMap<Tile, egui::TextureHandle>,
  ctx: egui::Context,
  layer_properties: LayerProperties,
}

const NAME: &str = "Tile Layer";

impl TileLayer {
  pub fn from_config(clone: egui::Context, config: &crate::config::Config) -> TileLayer {
    let (sender, receiver) = std::sync::mpsc::channel();
    let all_tile_loader = CachedTileLoader::from_config(config)
      .map(Arc::new)
      .collect();
    TileLayer {
      receiver,
      sender,
      tile_loader_index: 0,
      tile_loader_old_index: 0,
      all_tile_loader,
      loaded_tiles: HashMap::new(),
      ctx: clone,
      layer_properties: LayerProperties::default(),
    }
  }

  fn draw_tile(&self, ui: &mut Ui, rect: Rect, tile: &Tile, transform: &Transform) -> bool {
    if let Some(image_data) = self.loaded_tiles.get(tile) {
      let (nw, se) = tile.position();
      let (nw, se) = (transform.apply(nw), transform.apply(se));
      ui.painter_at(rect).image(
        image_data.id(),
        Rect::from_min_max(nw.into(), se.into()),
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
      );
      return true;
    }
    false
  }

  fn tile_loader(&self) -> Arc<CachedTileLoader> {
    self.all_tile_loader[self.tile_loader_index].clone()
  }

  fn get_tile(&self, tile: Tile) {
    let sender = self.sender.clone();
    let tile_loader = self.tile_loader().clone();
    let ctx = self.ctx.clone();
    if !self.loaded_tiles.contains_key(&tile) {
      tokio::spawn(async move {
        let image_data = tile_loader.tile_data(&tile).await;
        if let Ok(image_data) = image_data {
          let img = image::ImageReader::new(std::io::Cursor::new(image_data)).with_guessed_format();
          if img.is_err() {
            error!("Failed to load image: {:?}", img.err());
            return;
          }
          let img = img.unwrap().decode();
          if img.is_err() {
            error!("Failed to decode image for {tile:?}: {:?}", img.err());
            return;
          }
          let img = img.unwrap();
          let size = [img.width() as _, img.height() as _];
          let image_buffer = img.to_rgba8();
          let pixel = image_buffer.as_flat_samples();
          let egui_image = egui::ColorImage::from_rgba_unmultiplied(size, pixel.as_slice());
          sender.send((tile, egui_image)).unwrap();

          // tokio sleep for 100ms
          tokio::time::sleep(std::time::Duration::from_millis(100)).await;
          ctx.request_repaint();
        }
      });
    }
  }

  fn collect_new_tile_data(&mut self, ui: &Ui) {
    for (tile, egui_image) in self.receiver.try_iter() {
      info!("Received tile from loader: {tile:?}");
      let handle = ui.ctx().load_texture(
        format!("{}-{}-{}", tile.zoom, tile.x, tile.y),
        egui_image,
        egui::TextureOptions::default(),
      );
      self.loaded_tiles.insert(tile, handle);
    }
  }
}

impl Layer for TileLayer {
  #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, rect: Rect) {
    self.collect_new_tile_data(ui);
    if self.tile_loader_index != self.tile_loader_old_index {
      self.loaded_tiles.clear();
      self.tile_loader_old_index = self.tile_loader_index;
    }

    if !self.visible() {
      return;
    }

    let (width, height) = (rect.width(), rect.height());
    let zoom = (transform.zoom * (width.max(height) / TILE_SIZE)).log2() as u8 + 1;
    let inv = transform.invert();
    let min_pos = TileCoordinate::from_pixel_position(inv.apply(rect.min.into()), zoom);
    let max_pos = TileCoordinate::from_pixel_position(inv.apply(rect.max.into()), zoom);

    for tile in tiles_in_box(min_pos, max_pos) {
      if !self.loaded_tiles.contains_key(&tile) {
        self.get_tile(tile);
      }
    }

    // Draw parent tiles if detailed tiles are not available yet. Coarser tiles are drawn first to
    // have detailed textures visible on top.
    let mut tiles_to_draw = tiles_in_box(min_pos, max_pos)
      .filter_map(|mut tile| {
        while !self.loaded_tiles.contains_key(&tile) {
          tile = tile.parent()?;
        }
        Some(tile)
      })
      .collect::<Vec<_>>();
    tiles_to_draw.sort_unstable_by_key(|tile| tile.zoom);
    tiles_to_draw.dedup();
    tiles_to_draw.reverse();

    for tile in tiles_to_draw {
      if !self.draw_tile(ui, rect, &tile, transform) {
        self.get_tile(tile);
      }
    }
  }

  fn name(&self) -> &str {
    NAME
  }

  fn visible(&self) -> bool {
    self.layer_properties.visible
  }

  fn visible_mut(&mut self) -> &mut bool {
    &mut self.layer_properties.visible
  }

  fn ui_content(&mut self, ui: &mut Ui) {
    egui::ComboBox::from_label("tile source")
      .selected_text(
        self.all_tile_loader[self.tile_loader_index]
          .name()
          .to_string(),
      )
      .show_ui(ui, |ui| {
        for (i, tile_loader) in self.all_tile_loader.iter().enumerate() {
          ui.selectable_value(
            &mut self.tile_loader_index,
            i,
            tile_loader.name().to_string(),
          );
        }
      });
    egui::Label::new(format!("{} tile loaded", self.loaded_tiles.len())).ui(ui);
  }
}
