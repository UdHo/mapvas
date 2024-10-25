use std::{collections::HashMap, sync::Arc};

use egui::{Color32, ColorImage, Rect, Response, Sense, Ui};
use log::info;

use crate::map::coordinates::PixelPosition;

use super::{
  coordinates::{tiles_in_box, Tile, TileCoordinate, Transform, TILE_SIZE},
  tile_loader::{CachedTileLoader, TileLoader},
};

trait Layer {
  fn draw(&mut self, ui: &mut Ui, transform: &Transform, rect: Rect);
}

struct TileLayer {
  receiver: std::sync::mpsc::Receiver<(Tile, ColorImage)>,
  sender: std::sync::mpsc::Sender<(Tile, ColorImage)>,
  tile_loader: Arc<CachedTileLoader>,
  loaded_tiles: HashMap<Tile, egui::TextureHandle>,
}

fn set_coordinate_to_pixel(coord: PixelPosition, cursor: PixelPosition, transform: &mut Transform) {
  let current_pos_in_gui = transform.apply(coord);
  transform.translate(current_pos_in_gui * (-1.) + cursor);
}

impl TileLayer {
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

  fn get_tile(&self, tile: Tile, ctx: &egui::Context) {
    let sender = self.sender.clone();
    let tile_loader = self.tile_loader.clone();
    let ctx = ctx.clone();
    if !self.loaded_tiles.contains_key(&tile) {
      tokio::spawn(async move {
        let image_data = tile_loader.tile_data(&tile).await;
        if let Ok(image_data) = image_data {
          let img = image::io::Reader::new(std::io::Cursor::new(image_data)).with_guessed_format();
          if img.is_err() {
            info!("Failed to load image: {:?}", img.err());
            return;
          }
          let img = img.unwrap().decode();
          if img.is_err() {
            info!("Failed to decode image: {:?}", img.err());
            return;
          }
          let img = img.unwrap();
          let size = [img.width() as _, img.height() as _];
          let image_buffer = img.to_rgba8();
          let pixel = image_buffer.as_flat_samples();
          let egui_image = egui::ColorImage::from_rgba_unmultiplied(size, pixel.as_slice());
          sender.send((tile, egui_image)).unwrap();
          info!("Sent tile to UI: {:?}", tile);
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
        self.get_tile(tile, ui.ctx());
      }
    }
  }
}

fn fit_to_screen(transform: &mut Transform, rect: &Rect) {
  const MAX_ZOOM: f32 = 524_288.;
  let min_zoom = (rect.width().max(rect.height()) / TILE_SIZE - 1.)
    .log2()
    .floor();
  transform.zoom = transform.zoom.clamp(min_zoom, MAX_ZOOM);

  let inv = transform.invert();
  let PixelPosition { x, y } = inv.apply(PixelPosition { x: 0., y: 0. });
  if x < 0. || y < 0. {
    transform.translate(
      PixelPosition {
        x: (x.min(0.)),
        y: (y.min(0.)),
      } * transform.zoom,
    );
  }

  let PixelPosition { x, y } = dbg!(inv.apply(PixelPosition {
    x: rect.max.x,
    y: rect.max.y,
  }));
  if x > 1000. || y > 1000. {
    transform.translate(
      PixelPosition {
        x: (x - 1000.).max(0.),
        y: (y - 1000.).max(0.),
      } * transform.zoom,
    );
  }
}

struct ShapeLayer {}

#[derive(Default)]
pub struct MapData {
  transform: Transform,
  layers: Vec<Box<dyn Layer>>,
}

impl MapData {
  #[must_use]
  pub fn new() -> Self {
    let (sender, receiver) = std::sync::mpsc::channel();
    let tile_loader = Arc::new(CachedTileLoader::default());
    let tile_layer = TileLayer {
      receiver,
      sender,
      tile_loader,
      loaded_tiles: HashMap::new(),
    };
    Self {
      transform: Transform::invalid(),
      layers: vec![Box::new(tile_layer)],
    }
  }
}

pub fn map_ui(ui: &mut Ui, map_data: &mut MapData) -> Response {
  let size = ui.available_size();
  let (rect, /*mut*/ response) = ui.allocate_exact_size(size, Sense::click_and_drag());

  if map_data.transform.is_invalid() {
    fit_to_screen(&mut map_data.transform, &rect);
    set_coordinate_to_pixel(
      PixelPosition { x: 500., y: 500. },
      rect.center().into(),
      &mut map_data.transform,
    );
  }

  if response.hovered() {
    // check mouse wheel movvement
    let delta = ui
      .input(|i| {
        i.events
          .iter()
          .find_map(move |e| match e {
            egui::Event::MouseWheel {
              unit: _,
              delta,
              modifiers: _,
            } => Some(delta),
            _ => None,
          })
          .copied()
      })
      .map(|d| (d.y / 1. + 1.).clamp(0.5, 2.));
    if let Some(delta) = delta {
      let cursor = response.hover_pos().unwrap_or_default().into();
      let hover_pos: PixelPosition = map_data.transform.invert().apply(cursor);
      map_data.transform.zoom(delta);
      set_coordinate_to_pixel(hover_pos, cursor, &mut map_data.transform);
      fit_to_screen(&mut map_data.transform, &rect);
    }
  }

  if response.clicked() {
    eprintln!("Clicked {:?} {:?}", response, response.hover_pos());
  }
  if response.dragged() {
    eprintln!("Dragged {:?}", response.drag_delta());
    map_data.transform.translate(PixelPosition {
      x: response.drag_delta().x,
      y: response.drag_delta().y,
    });
    fit_to_screen(&mut map_data.transform, &rect);
  }

  if ui.is_rect_visible(rect) {
    for layer in &mut map_data.layers {
      layer.draw(ui, &map_data.transform, rect);
    }
  }

  response
}
