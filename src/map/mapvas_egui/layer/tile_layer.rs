use std::{collections::HashMap, sync::Arc};

use egui::{Color32, ColorImage, Pos2, Rect, Ui, Widget};
use log::error;

use crate::{
  map::{
    coordinates::{
      TILE_SIZE, Tile, TileCoordinate, TilePriority, Transform, generate_preload_tiles,
      tiles_in_box,
    },
    tile_loader::{CachedTileLoader, TileLoader, TileSource},
  },
  profile_scope,
};

use super::{Layer, LayerProperties};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoordinateDisplayMode {
  Off,
  Overlay,
  GridOnly,
}

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
  tile_source: TileSource,
  coordinate_display_mode: CoordinateDisplayMode,
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
      tile_source: TileSource::All,
      coordinate_display_mode: CoordinateDisplayMode::Off,
    }
  }

  fn draw_tile(&self, ui: &mut Ui, rect: Rect, tile: &Tile, transform: &Transform) -> bool {
    if let Some(image_data) = self.loaded_tiles.get(tile) {
      let (nw, se) = tile.position();
      let (nw, se) = (transform.apply(nw), transform.apply(se));
      let tile_rect = Rect::from_min_max(nw.into(), se.into());

      let tint_color = if self.coordinate_display_mode == CoordinateDisplayMode::Overlay {
        let is_even_tile = (tile.x + tile.y).is_multiple_of(2);
        if is_even_tile {
          Color32::from_rgba_unmultiplied(255, 240, 240, 255)
        } else {
          Color32::from_rgba_unmultiplied(240, 240, 255, 255)
        }
      } else {
        Color32::WHITE
      };

      ui.painter_at(rect).image(
        image_data.id(),
        tile_rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint_color,
      );

      if self.coordinate_display_mode == CoordinateDisplayMode::Overlay {
        draw_coordinate_text_overlay(ui, rect, tile, &tile_rect);
      }

      return true;
    }
    false
  }

  fn draw_coordinate_grid(
    &self,
    ui: &mut Ui,
    clip_rect: Rect,
    transform: &Transform,
    min_pos: TileCoordinate,
    max_pos: TileCoordinate,
  ) {
    use crate::map::coordinates::tiles_in_box;

    let painter = ui.painter_at(clip_rect);

    for tile in tiles_in_box(min_pos, max_pos) {
      let (nw, se) = tile.position();
      let (nw, se) = (transform.apply(nw), transform.apply(se));
      let tile_rect = Rect::from_min_max(nw.into(), se.into());

      if !tile_rect.intersects(clip_rect) {
        continue;
      }

      if self.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
        let is_even_tile = (tile.x + tile.y).is_multiple_of(2);
        let bg_color = if is_even_tile {
          Color32::from_rgba_unmultiplied(255, 240, 240, 120)
        } else {
          Color32::from_rgba_unmultiplied(240, 240, 255, 120)
        };
        painter.rect_filled(tile_rect, egui::CornerRadius::ZERO, bg_color);
      }

      self.draw_tile_info(ui, clip_rect, &tile, &tile_rect);
    }
  }

  fn draw_tile_info(&self, ui: &mut Ui, clip_rect: Rect, tile: &Tile, tile_rect: &Rect) {
    let painter = ui.painter_at(clip_rect);

    let bg_width = 100.0;
    let bg_height = 60.0;
    let bg_rect = Rect::from_center_size(tile_rect.center(), egui::vec2(bg_width, bg_height));

    if tile_rect.width() > bg_width && tile_rect.height() > bg_height {
      painter.rect_filled(
        bg_rect,
        egui::CornerRadius::same(5),
        Color32::from_rgba_unmultiplied(0, 0, 0, 180),
      );

      let font_id = egui::FontId::monospace(11.0);
      let text_color = Color32::WHITE;

      let lines = [
        format!("Z:{}", tile.zoom),
        format!("X:{}", tile.x),
        format!("Y:{}", tile.y),
      ];

      for (i, line) in lines.iter().enumerate() {
        #[expect(clippy::cast_precision_loss)]
        let text_pos = bg_rect.min + egui::vec2(8.0, 8.0 + i as f32 * 14.0);
        painter.text(
          text_pos,
          egui::Align2::LEFT_TOP,
          line,
          font_id.clone(),
          text_color,
        );
      }
    }

    let border_color = if self.coordinate_display_mode == CoordinateDisplayMode::GridOnly {
      Color32::from_rgb(80, 80, 80)
    } else {
      Color32::from_rgb(100, 100, 100)
    };

    painter.rect_stroke(
      *tile_rect,
      egui::CornerRadius::ZERO,
      egui::Stroke::new(1.0, border_color),
      egui::epaint::StrokeKind::Outside,
    );
  }

  fn tile_loader(&self) -> Arc<CachedTileLoader> {
    self.all_tile_loader[self.tile_loader_index].clone()
  }

  fn get_tile(&self, tile: Tile) {
    self.get_tile_with_priority(tile, TilePriority::Current);
  }

  fn get_tile_with_priority(&self, tile: Tile, priority: TilePriority) {
    let sender = self.sender.clone();
    let tile_loader = self.tile_loader().clone();
    let ctx = self.ctx.clone();
    let tile_source = self.tile_source;
    if !self.loaded_tiles.contains_key(&tile) {
      tokio::spawn(async move {
        let image_data = tile_loader
          .tile_data_with_priority(&tile, tile_source, priority)
          .await;
        if let Ok(image_data) = image_data {
          let img_reader =
            match image::ImageReader::new(std::io::Cursor::new(image_data)).with_guessed_format() {
              Ok(reader) => reader,
              Err(e) => {
                error!("Failed to create image reader for {tile:?}: {e}");
                return;
              }
            };
          let img = match img_reader.decode() {
            Ok(image) => image,
            Err(e) => {
              error!("Failed to decode image for {tile:?}: {e}");
              return;
            }
          };
          let size = [img.width() as _, img.height() as _];
          let image_buffer = img.to_rgba8();
          let pixel = image_buffer.as_flat_samples();
          let egui_image = egui::ColorImage::from_rgba_unmultiplied(size, pixel.as_slice());
          let _ = sender
            .send((tile, egui_image))
            .inspect_err(|e| error!("Failed to send tile: {e}"));
          // Shorter delay for higher priority tiles
          let delay = match priority {
            TilePriority::Current => 100,
            TilePriority::Adjacent => 200,
            TilePriority::ZoomLevel => 300,
          };
          tokio::time::sleep(std::time::Duration::from_millis(delay)).await;

          ctx.request_repaint();
        }
      });
    }
  }

  fn preload_tiles(&self, visible_tiles: &[Tile]) {
    // Generate preload candidates
    let preload_candidates = generate_preload_tiles(visible_tiles);

    // Limit preloading to avoid overwhelming the system
    let max_preload = 20;
    for (tile, priority) in preload_candidates.into_iter().take(max_preload) {
      // Only preload if not already loaded or loading
      if !self.loaded_tiles.contains_key(&tile) {
        self.get_tile_with_priority(tile, priority);
      }
    }
  }
  fn collect_new_tile_data(&mut self, ui: &Ui) {
    for (tile, egui_image) in self.receiver.try_iter() {
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
    profile_scope!("TileLayer::draw");
    self.collect_new_tile_data(ui);
    if self.tile_loader_index != self.tile_loader_old_index {
      self.loaded_tiles.clear();
      self.tile_loader_old_index = self.tile_loader_index;
    }

    if !self.visible() {
      return;
    }

    let (width, height) = (rect.width(), rect.height());
    let zoom = (transform.zoom * (width.max(height) / TILE_SIZE)).log2() as u8 + 2;
    let inv = transform.invert();
    let min_pos = TileCoordinate::from_pixel_position(inv.apply(rect.min.into()), zoom);
    let max_pos = TileCoordinate::from_pixel_position(inv.apply(rect.max.into()), zoom);

    let visible_tiles: Vec<Tile> = tiles_in_box(min_pos, max_pos).collect();

    // Load current visible tiles with highest priority
    for tile in &visible_tiles {
      if !self.loaded_tiles.contains_key(tile) {
        self.get_tile(*tile);
      }
    }

    // Start preloading adjacent and zoom level tiles
    self.preload_tiles(&visible_tiles);

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

    if self.coordinate_display_mode != CoordinateDisplayMode::GridOnly {
      for tile in tiles_to_draw {
        if !self.draw_tile(ui, rect, &tile, transform) {
          self.get_tile(tile);
        }
      }
    }

    if self.coordinate_display_mode != CoordinateDisplayMode::Off {
      self.draw_coordinate_grid(ui, rect, transform, min_pos, max_pos);
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
    egui::ComboBox::from_label("tile provider")
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
    egui::ComboBox::from_label("tile source")
      .selected_text(self.tile_source.to_string())
      .show_ui(ui, |ui| {
        for s in [TileSource::All, TileSource::Cache, TileSource::Download] {
          ui.selectable_value(&mut self.tile_source, s, s.to_string());
        }
      });

    egui::Label::new(format!("{} tiles loaded", self.loaded_tiles.len())).ui(ui);

    ui.separator();
    ui.label("Tile Coordinate Display:");

    ui.horizontal(|ui| {
      ui.radio_value(
        &mut self.coordinate_display_mode,
        CoordinateDisplayMode::Off,
        "Off",
      );
      ui.radio_value(
        &mut self.coordinate_display_mode,
        CoordinateDisplayMode::Overlay,
        "Overlay",
      );
      ui.radio_value(
        &mut self.coordinate_display_mode,
        CoordinateDisplayMode::GridOnly,
        "Grid Only",
      );
    });
  }

  fn closest_geometry_with_selection(&mut self, _pos: Pos2, _transform: &Transform) -> Option<f64> {
    None
  }

  fn handle_double_click(&mut self, _pos: Pos2, _transform: &Transform) -> bool {
    false
  }
}

fn draw_coordinate_text_overlay(ui: &mut Ui, clip_rect: Rect, tile: &Tile, tile_rect: &Rect) {
  let painter = ui.painter_at(clip_rect);

  let bg_width = 100.0;
  let bg_height = 60.0;
  let bg_rect = Rect::from_center_size(tile_rect.center(), egui::vec2(bg_width, bg_height));

  painter.rect_filled(
    bg_rect,
    egui::CornerRadius::same(5),
    Color32::from_rgba_unmultiplied(0, 0, 0, 180),
  );

  let font_id = egui::FontId::monospace(11.0);
  let text_color = Color32::WHITE;

  let lines = [
    format!("Z:{}", tile.zoom),
    format!("X:{}", tile.x),
    format!("Y:{}", tile.y),
  ];

  for (i, line) in lines.iter().enumerate() {
    #[expect(clippy::cast_precision_loss)]
    let text_pos = bg_rect.min + egui::vec2(8.0, 8.0 + i as f32 * 14.0);
    painter.text(
      text_pos,
      egui::Align2::LEFT_TOP,
      line,
      font_id.clone(),
      text_color,
    );
  }

  painter.rect_stroke(
    *tile_rect,
    egui::CornerRadius::ZERO,
    egui::Stroke::new(2.0, Color32::from_rgb(100, 100, 100)),
    egui::epaint::StrokeKind::Outside,
  );
}
