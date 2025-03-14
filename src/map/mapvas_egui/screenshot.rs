use std::path::PathBuf;

use egui::{Context, UserData, ViewportCommand};

use crate::map::map_event::MapEvent;

use super::Layer;

pub struct ScreenshotLayer {
  sender: std::sync::mpsc::Sender<MapEvent>,
  receiver: std::sync::mpsc::Receiver<MapEvent>,
  ctx: Context,
}

impl ScreenshotLayer {
  pub fn new(ctx: Context) -> Self {
    let (sender, receiver) = std::sync::mpsc::channel();
    Self {
      sender,
      receiver,
      ctx,
    }
  }

  pub fn get_sender(&self) -> std::sync::mpsc::Sender<MapEvent> {
    self.sender.clone()
  }

  fn take_screenshot(&self, path: PathBuf) {
    self
      .ctx
      .send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(path)));
  }

  #[expect(clippy::cast_possible_truncation)]
  fn handle_screenshots(&self) {
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
                .unwrap_or_else(super::helpers::current_time_screenshot_name),
            ))
          } else {
            None
          }
        })
        .last()
    });

    if let Some((image, path)) = image_path {
      let img = image::RgbaImage::from_raw(
        image.width() as u32,
        image.height() as u32,
        image.as_raw().to_vec(),
      )
      .map(image::DynamicImage::ImageRgba8);
      if let Some(img) = img {
        let _ = img.save(path).inspect_err(|e| {
          log::error!("Failed to save image: {:?}", e);
        });
      }
    }
  }
}

impl Layer for ScreenshotLayer {
  fn draw(
    &mut self,
    _ui: &mut egui::Ui,
    _transform: &crate::map::coordinates::Transform,
    _rect: egui::Rect,
  ) {
    for event in self.receiver.try_iter() {
      if let MapEvent::Screenshot(path) = event {
        self.take_screenshot(path);
      }
    }
    self.handle_screenshots();
  }
}
