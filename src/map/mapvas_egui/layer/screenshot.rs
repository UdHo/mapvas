use std::{path::PathBuf, sync::Arc};

use egui::{
  Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions, UserData, ViewportCommand,
};

use crate::map::{map_event::MapEvent, mapvas_egui::helpers::current_time_screenshot_name};

use super::{Layer, LayerProperties};

/// A layer that takes screenshots of the map, saves the image, and displays it on the map.
pub struct ScreenshotLayer {
  last_screenshot: Option<TextureHandle>,
  last_screenshot_time: std::time::Instant,
  sender: std::sync::mpsc::Sender<MapEvent>,
  receiver: std::sync::mpsc::Receiver<MapEvent>,
  ctx: Context,
  screenshot_base_path: PathBuf,
  layer_properties: LayerProperties,
}

impl ScreenshotLayer {
  pub fn new(ctx: Context) -> Self {
    let (sender, receiver) = std::sync::mpsc::channel();
    Self {
      last_screenshot: None,
      last_screenshot_time: std::time::Instant::now(),
      sender,
      receiver,
      ctx,
      screenshot_base_path: std::env::vars()
        .find(|(k, _)| k == "MAPVAS_SCREENSHOT_PATH")
        .map_or_else(|| PathBuf::from("."), |(_, v)| PathBuf::from(v)),
      layer_properties: LayerProperties::default(),
    }
  }

  pub fn get_sender(&self) -> std::sync::mpsc::Sender<MapEvent> {
    self.sender.clone()
  }

  fn take_screenshot(&self, path: PathBuf) {
    let abs_path = if path.is_relative() {
      self.screenshot_base_path.join(&path)
    } else {
      path
    };
    self
      .ctx
      .send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(abs_path)));
  }

  #[expect(clippy::cast_possible_truncation)]
  fn handle_screenshots(&mut self) {
    let image_path = self.ctx.input(|i| {
      i.events
        .iter()
        .filter_map(|e| {
          if let egui::Event::Screenshot {
            image, user_data, ..
          } = e
          {
            Some((
              image.clone(),
              user_data
                .data
                .clone()
                .and_then(|d| d.downcast_ref::<PathBuf>().cloned())
                .unwrap_or_else(current_time_screenshot_name),
            ))
          } else {
            None
          }
        })
        .next_back()
    });

    if let Some((image, path)) = image_path {
      self.store_screenshot_texture(image.clone());
      let img = image::RgbaImage::from_raw(
        image.width() as u32,
        image.height() as u32,
        image.as_raw().to_vec(),
      )
      .map(image::DynamicImage::ImageRgba8);
      if let Some(img) = img {
        let _ = img.save(path).inspect_err(|e| {
          log::error!("Failed to save image: {e:?}");
        });
      }
    }
  }

  fn store_screenshot_texture(&mut self, image: Arc<ColorImage>) {
    self.last_screenshot_time = std::time::Instant::now();
    self.last_screenshot = Some(self.ctx.load_texture(
      "screenshot",
      image,
      TextureOptions::default(),
    ));
  }

  #[expect(clippy::cast_precision_loss)]
  fn compute_gamma(&self) -> f32 {
    1. - (self.last_screenshot_time.elapsed().as_millis() as f32) / 10000.
  }
}

const NAME: &str = "Screenshot Layer";

impl Layer for ScreenshotLayer {
  fn draw(
    &mut self,
    ui: &mut egui::Ui,
    _transform: &crate::map::coordinates::Transform,
    rect: egui::Rect,
  ) {
    for event in self.receiver.try_iter() {
      if let MapEvent::Screenshot(path) = event {
        self.take_screenshot(path);
      }
    }
    self.handle_screenshots();

    if !self.visible() {
      return;
    }

    if let Some(texture) = &self.last_screenshot {
      let screenshot_rect = rect
        .with_min_x(rect.max.x - (rect.max.x - rect.min.x) * 0.2)
        .with_min_y(rect.max.y - (rect.max.y - rect.min.y) * 0.2);

      let gamma = ui
        .ctx()
        .pointer_hover_pos()
        .and_then(|pos| screenshot_rect.contains(pos).then_some(1.0))
        .unwrap_or_else(|| self.compute_gamma());
      if gamma < 0. {
        self.last_screenshot = None;
        return;
      }

      ui.painter_at(screenshot_rect).image(
        texture.id(),
        screenshot_rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::LIGHT_GRAY.gamma_multiply(gamma),
      );
      self.ctx.request_repaint_after_secs(0.2);
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

  fn closest_geometry_with_selection(
    &mut self,
    _pos: egui::Pos2,
    _transform: &crate::map::coordinates::Transform,
  ) -> Option<f64> {
    None
  }

  fn ui_content(&mut self, _ui: &mut egui::Ui) {}
}
