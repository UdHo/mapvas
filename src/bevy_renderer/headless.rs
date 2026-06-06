use std::{
  any::Any,
  panic::{self, AssertUnwindSafe},
  sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
  },
  time::Duration,
};

use bevy::{
  app::ScheduleRunnerPlugin,
  camera::RenderTarget,
  log::LogPlugin,
  prelude::*,
  render::{
    render_resource::{TextureFormat, TextureUsages},
    view::screenshot::{Screenshot, ScreenshotCaptured},
  },
  window::{ExitCondition, WindowPlugin},
  winit::WinitPlugin,
};

use crate::{
  config::Config,
  map::{
    Map,
    coordinates::{PixelPosition, PixelRect},
    map_event::MapEvent,
    tile_renderer::init_style_config,
  },
  remote::RepaintSignal,
};

use super::{
  MapvasBevyRenderPlugin,
  app::build_runtime,
  geometry::BevyGeometryLayer,
  map::BevyMapViewport,
  surface::BevyRenderSurface,
  tiles::{BevyTileLayer, BevyTileRuntime},
};

const HEADLESS_STABLE_IDLE_FRAMES: u32 = 3;
const HEADLESS_MAX_FRAMES: u32 = 60 * 60;

#[derive(Debug, thiserror::Error)]
pub enum BevyHeadlessRenderError {
  #[error("headless Bevy renderer did not capture an image")]
  CaptureMissing,
  #[error("headless Bevy renderer could not initialize a GPU: {0}")]
  GpuUnavailable(String),
  #[error("failed to convert Bevy screenshot image: {0}")]
  ImageConversion(String),
  #[error("headless Bevy renderer panicked: {0}")]
  RendererPanicked(String),
}

pub struct BevyHeadlessRenderer {
  config: Config,
  wait_for_tiles: bool,
  no_map: bool,
  clear_color: Color,
}

impl BevyHeadlessRenderer {
  #[must_use]
  pub fn new() -> Self {
    Self::with_config(Config::new())
  }

  #[must_use]
  pub fn with_config(config: Config) -> Self {
    Self {
      config,
      wait_for_tiles: true,
      no_map: false,
      clear_color: Color::WHITE,
    }
  }

  #[must_use]
  pub fn without_tiles(mut self) -> Self {
    self.wait_for_tiles = false;
    self
  }

  #[must_use]
  pub fn without_map(mut self) -> Self {
    self.no_map = true;
    self.wait_for_tiles = false;
    self.clear_color = Color::BLACK;
    self
  }

  #[must_use]
  pub fn with_clear_color(mut self, color: Color) -> Self {
    self.clear_color = color;
    self
  }

  pub fn render(
    &self,
    events: &[MapEvent],
    width: u32,
    height: u32,
  ) -> Result<image::RgbaImage, BevyHeadlessRenderError> {
    init_style_config(self.config.vector_style_file.as_deref());

    let runtime = build_runtime();
    let runtime_handle = runtime.handle().clone();
    let (repaint_sender, _repaint_receiver) = std::sync::mpsc::channel();
    let (mut map, remote, _data_holder) =
      Map::new_bevy(self.config.clone(), RepaintSignal::channel(repaint_sender));
    map.set_headless();

    for event in events {
      remote.handle_map_event(event.clone());
    }

    let mut tile_layer = BevyTileLayer::new(self.config.clone());
    tile_layer.set_preload_enabled(false);
    if self.no_map {
      tile_layer.set_visible(false);
    }

    let (capture_sender, capture_receiver) = std::sync::mpsc::channel();
    let mut app = catch_bevy_headless_panic(|| {
      let mut app = App::new();
      app
        .insert_non_send_resource(BevyHeadlessState::new(
          map,
          self.config.clone(),
          width,
          height,
          self.wait_for_tiles,
          capture_sender,
        ))
        .insert_resource(ClearColor(self.clear_color))
        .insert_resource(BevyRenderSurface::new(width as f32, height as f32))
        .insert_resource(BevyTileRuntime(runtime_handle))
        .insert_resource(tile_layer)
        .add_plugins(
          DefaultPlugins
            .set(WindowPlugin {
              primary_window: None,
              exit_condition: ExitCondition::DontExit,
              ..Default::default()
            })
            .disable::<LogPlugin>()
            .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
          1.0 / 60.0,
        )))
        .add_plugins(MapvasBevyRenderPlugin::headless())
        .add_systems(Startup, setup_headless_camera)
        .add_systems(PreUpdate, drive_headless_map_frame);
      app
    })?;

    run_bevy_app(&mut app, &runtime)?;

    receive_capture(capture_receiver)
  }
}

impl Default for BevyHeadlessRenderer {
  fn default() -> Self {
    Self::new()
  }
}

#[derive(Resource, Clone)]
struct HeadlessRenderTarget(Handle<Image>);

struct BevyHeadlessState {
  map: Map,
  config: Config,
  width: u32,
  height: u32,
  wait_for_tiles: bool,
  capture_sender: Sender<Result<image::RgbaImage, BevyHeadlessRenderError>>,
  known_snapshot_version: u64,
  known_geometry_version: u64,
  idle_frames: u32,
  frames: u32,
  capture_requested: bool,
}

impl BevyHeadlessState {
  fn new(
    map: Map,
    config: Config,
    width: u32,
    height: u32,
    wait_for_tiles: bool,
    capture_sender: Sender<Result<image::RgbaImage, BevyHeadlessRenderError>>,
  ) -> Self {
    Self {
      map,
      config,
      width,
      height,
      wait_for_tiles,
      capture_sender,
      known_snapshot_version: u64::MAX,
      known_geometry_version: u64::MAX,
      idle_frames: 0,
      frames: 0,
      capture_requested: false,
    }
  }

  fn rect(&self) -> PixelRect {
    PixelRect::from_min_size(
      PixelPosition { x: 0.0, y: 0.0 },
      PixelPosition {
        x: self.width as f32,
        y: self.height as f32,
      },
    )
  }
}

fn setup_headless_camera(
  mut commands: Commands,
  mut images: ResMut<Assets<Image>>,
  state: NonSend<BevyHeadlessState>,
) {
  let mut image = Image::new_target_texture(
    state.width,
    state.height,
    TextureFormat::bevy_default(),
    None,
  );
  image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
  let target = images.add(image);
  commands.insert_resource(HeadlessRenderTarget(target.clone()));
  commands.spawn((Camera2d, RenderTarget::Image(target.into())));
}

fn drive_headless_map_frame(
  mut commands: Commands,
  mut state: NonSendMut<BevyHeadlessState>,
  mut viewport: ResMut<BevyMapViewport>,
  mut geometry: ResMut<BevyGeometryLayer>,
  mut tiles: ResMut<BevyTileLayer>,
  target: Option<Res<HeadlessRenderTarget>>,
) {
  if state.capture_requested {
    return;
  }

  state.frames = state.frames.saturating_add(1);
  let rect = state.rect();
  state.map.prepare_headless_bevy_frame(rect);
  viewport.set(state.map.viewport());
  geometry.update_config(&state.config);
  tiles.update_config(&state.config);
  tiles.refresh_style_version();

  let snapshot_version = state.map.geometry_snapshot_version();
  if snapshot_version != state.known_snapshot_version {
    let snapshot = state
      .map
      .geometry_snapshot_since(state.known_geometry_version);
    state.known_snapshot_version = snapshot.version;
    state.known_geometry_version = snapshot.geometry_version;
    geometry.update_snapshot(snapshot);
  }

  let pending = state.map.has_pending_work() || (state.wait_for_tiles && tiles.has_pending_work());
  if pending && state.frames < HEADLESS_MAX_FRAMES {
    state.idle_frames = 0;
    return;
  }

  if pending {
    log::warn!("Headless Bevy render captured after timeout with pending tile or map work");
  } else {
    state.idle_frames = state.idle_frames.saturating_add(1);
    if state.idle_frames < HEADLESS_STABLE_IDLE_FRAMES {
      return;
    }
  }

  let Some(target) = target else {
    return;
  };
  let capture_sender = state.capture_sender.clone();
  state.capture_requested = true;
  commands.spawn(Screenshot::image(target.0.clone())).observe(
    move |capture: On<ScreenshotCaptured>, mut app_exit: MessageWriter<AppExit>| {
      let image = capture
        .image
        .clone()
        .try_into_dynamic()
        .map(|image| image.to_rgba8())
        .map_err(|error| BevyHeadlessRenderError::ImageConversion(format!("{error:?}")));
      let _ = capture_sender.send(image);
      app_exit.write(AppExit::Success);
    },
  );
}

fn receive_capture(
  receiver: Receiver<Result<image::RgbaImage, BevyHeadlessRenderError>>,
) -> Result<image::RgbaImage, BevyHeadlessRenderError> {
  receiver
    .recv()
    .map_err(|_| BevyHeadlessRenderError::CaptureMissing)?
}

fn run_bevy_app(
  app: &mut App,
  runtime: &tokio::runtime::Runtime,
) -> Result<(), BevyHeadlessRenderError> {
  catch_bevy_headless_panic(|| {
    let _runtime_guard = runtime.handle().enter();
    app.run();
  })
}

fn catch_bevy_headless_panic<T>(
  operation: impl FnOnce() -> T,
) -> Result<T, BevyHeadlessRenderError> {
  let captured_panic_message = Arc::new(Mutex::new(None));
  let hook_message = Arc::clone(&captured_panic_message);
  let previous_hook = panic::take_hook();
  panic::set_hook(Box::new(move |info| {
    let payload_message = panic_payload_message(info.payload());
    let message = if payload_message == "unknown panic payload" {
      info.to_string()
    } else {
      payload_message
    };
    if let Ok(mut captured_message) = hook_message.lock() {
      *captured_message = Some(message);
    }
  }));

  let run_result = panic::catch_unwind(AssertUnwindSafe(operation));
  panic::set_hook(previous_hook);

  match run_result {
    Ok(value) => Ok(value),
    Err(payload) => {
      let message = captured_panic_message
        .lock()
        .ok()
        .and_then(|captured_message| captured_message.clone())
        .unwrap_or_else(|| panic_payload_message(payload.as_ref()));
      if is_gpu_unavailable_panic(&message) {
        return Err(BevyHeadlessRenderError::GpuUnavailable(message));
      }
      Err(BevyHeadlessRenderError::RendererPanicked(message))
    }
  }
}

fn is_gpu_unavailable_panic(message: &str) -> bool {
  message.contains("Unable to find a GPU") || message.contains("No adapter found")
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
  if let Some(message) = payload.downcast_ref::<&str>() {
    (*message).to_owned()
  } else if let Some(message) = payload.downcast_ref::<String>() {
    message.clone()
  } else {
    "unknown panic payload".to_owned()
  }
}
