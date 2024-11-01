use std::{collections::HashMap, sync::Arc};

use egui::{Color32, ColorImage, Rect, Ui};
use log::{debug, info};

use crate::map::{
  coordinates::{
    tiles_in_box, BoundingBox, Coordinate, Tile, TileCoordinate, Transform, TILE_SIZE,
  },
  tile_loader::{CachedTileLoader, TileLoader},
};

use super::Layer;

pub struct TileLayer {
  receiver: std::sync::mpsc::Receiver<(Tile, ColorImage)>,
  sender: std::sync::mpsc::Sender<(Tile, ColorImage)>,
  tile_loader: Arc<CachedTileLoader>,
  loaded_tiles: HashMap<Tile, egui::TextureHandle>,
  ctx: egui::Context,
}

impl TileLayer {
  pub fn new(ctx: egui::Context) -> Self {
    let (sender, receiver) = std::sync::mpsc::channel();
    let tile_loader = Arc::new(CachedTileLoader::default());
    Self {
      receiver,
      sender,
      tile_loader,
      loaded_tiles: HashMap::new(),
      ctx,
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
        if (tile.x + tile.y) % 2 == 0 {
          Color32::WHITE
        } else {
          Color32::from_rgb(230, 230, 230)
        },
      );
      return true;
    }
    false
  }

  fn get_tile(&self, tile: Tile) {
    let sender = self.sender.clone();
    let tile_loader = self.tile_loader.clone();
    let ctx = self.ctx.clone();
    if !self.loaded_tiles.contains_key(&tile) {
      tokio::spawn(async move {
        let image_data = tile_loader.tile_data(&tile).await;
        if let Ok(image_data) = image_data {
          let img = image::io::Reader::new(std::io::Cursor::new(image_data)).with_guessed_format();
          if img.is_err() {
            debug!("Failed to load image: {:?}", img.err());
            return;
          }
          let img = img.unwrap().decode();
          if img.is_err() {
            debug!("Failed to decode image: {:?}", img.err());
            return;
          }
          let img = img.unwrap();
          let size = [img.width() as _, img.height() as _];
          let image_buffer = img.to_rgba8();
          let pixel = image_buffer.as_flat_samples();
          let egui_image = egui::ColorImage::from_rgba_unmultiplied(size, pixel.as_slice());
          sender.send((tile, egui_image)).unwrap();
          debug!("Sent tile to UI: {:?}", tile);
          ctx.request_repaint();
        }
      });
    }
  }

  fn collect_new_tile_data(&mut self, ui: &Ui) {
    for (tile, egui_image) in self.receiver.try_iter() {
      info!("Received tile from loader: {:?}", tile);
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

    let (width, height) = (rect.width(), rect.height());
    let zoom = (transform.zoom * (width.min(height) / TILE_SIZE)).log2() as i32;
    let inv = transform.invert();
    let min_pos = TileCoordinate::from_pixel_position(inv.apply(rect.min.into()), zoom as u8);
    let max_pos = TileCoordinate::from_pixel_position(inv.apply(rect.max.into()), zoom as u8);

    for tile in tiles_in_box(min_pos, max_pos) {
      if !self.draw_tile(ui, rect, &tile, transform) {
        self.get_tile(tile);
      }
    }
  }

  fn bounding_box(&self) -> Option<crate::map::coordinates::BoundingBox> {
    Some(BoundingBox::from_iterator([
      Coordinate {
        lat: 52.5,
        lon: 13.,
      }
      .into(),
      Coordinate {
        lat: 52.6,
        lon: 13.1,
      }
      .into(),
    ]))
  }
}
