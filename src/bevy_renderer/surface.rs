use bevy::{
  prelude::*,
  window::{PrimaryWindow, Window},
};

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct BevyRenderSurface {
  width: f32,
  height: f32,
}

impl BevyRenderSurface {
  #[must_use]
  pub fn new(width: f32, height: f32) -> Self {
    Self { width, height }
  }

  #[must_use]
  pub fn width(self) -> f32 {
    self.width
  }

  #[must_use]
  pub fn height(self) -> f32 {
    self.height
  }
}

impl Default for BevyRenderSurface {
  fn default() -> Self {
    Self::new(1600.0, 1200.0)
  }
}

pub struct BevyRenderSurfacePlugin;

impl Plugin for BevyRenderSurfacePlugin {
  fn build(&self, app: &mut App) {
    app
      .init_resource::<BevyRenderSurface>()
      .add_systems(Update, update_render_surface_from_window);
  }
}

fn update_render_surface_from_window(
  mut surface: ResMut<BevyRenderSurface>,
  windows: Query<&Window, With<PrimaryWindow>>,
) {
  let Ok(window) = windows.single() else {
    return;
  };
  *surface = BevyRenderSurface::new(window.width(), window.height());
}
