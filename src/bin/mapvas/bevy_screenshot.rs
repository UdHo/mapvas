use std::path::PathBuf;

use bevy::{
  prelude::*,
  render::view::screenshot::{Screenshot, save_to_disk},
};

pub struct BevyScreenshotPlugin;

impl Plugin for BevyScreenshotPlugin {
  fn build(&self, app: &mut App) {
    app
      .init_resource::<BevyScreenshotRequests>()
      .add_systems(Update, issue_bevy_screenshots);
  }
}

#[derive(Resource)]
pub struct BevyScreenshotRequests {
  pending_paths: Vec<PathBuf>,
  screenshot_base_path: PathBuf,
}

impl Default for BevyScreenshotRequests {
  fn default() -> Self {
    Self {
      pending_paths: Vec::new(),
      screenshot_base_path: std::env::vars()
        .find(|(key, _)| key == "MAPVAS_SCREENSHOT_PATH")
        .map_or_else(|| PathBuf::from("."), |(_, value)| PathBuf::from(value)),
    }
  }
}

impl BevyScreenshotRequests {
  pub fn request_path(&mut self, path: PathBuf) {
    self.pending_paths.push(path);
  }

  fn take_pending_absolute_paths(&mut self) -> Vec<PathBuf> {
    let base_path = self.screenshot_base_path.clone();
    self
      .pending_paths
      .drain(..)
      .map(|path| {
        if path.is_relative() {
          base_path.join(path)
        } else {
          path
        }
      })
      .collect()
  }
}

fn issue_bevy_screenshots(mut commands: Commands, mut requests: ResMut<BevyScreenshotRequests>) {
  for path in requests.take_pending_absolute_paths() {
    commands
      .spawn(Screenshot::primary_window())
      .observe(save_to_disk(path));
  }
}
