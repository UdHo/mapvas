use crate::coordinates::{tiles_in_box, PixelPosition, Tile, TileCoordinate, TILE_SIZE};
use crate::tile_loader::{CachedTileLoader, TileGetter};
use femtovg::{renderer::OpenGl, Canvas, Path};
use femtovg::{Color, ImageFlags, ImageId, Paint};
use glutin::prelude::*;
use glutin::{
  config::ConfigTemplateBuilder,
  context::{ContextApi, ContextAttributesBuilder},
  display::GetGlDisplay,
  surface::{SurfaceAttributesBuilder, WindowSurface},
};
use log::{debug, trace};

use glutin_winit::DisplayBuilder;
use raw_window_handle::HasRawWindowHandle;
use std::collections::HashMap;
use std::num::NonZeroU32;
use winit::event::{ElementState, KeyboardInput, MouseButton, VirtualKeyCode};
use winit::event_loop::EventLoopBuilder;
use winit::window::WindowBuilder;
use winit::{
  event::{Event, WindowEvent},
  event_loop::{ControlFlow, EventLoop, EventLoopProxy},
  window::Window,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapEvent {
  TileDataArrived { tile: Tile, data: Vec<u8> },
}

pub struct MapVas {
  canvas: Canvas<OpenGl>,
  context: glutin::context::PossiblyCurrentContext,
  surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
  window: Window,
  dragging: bool,
  mousex: f32,
  mousey: f32,
  tile_loader: CachedTileLoader,
  event_proxy: EventLoopProxy<MapEvent>,
  loaded_images: HashMap<Tile, ImageId>,
}

impl MapVas {
  async fn run(mut self, event_loop: EventLoop<MapEvent>) {
    event_loop.run(move |event, _, control_flow| {
      *control_flow = ControlFlow::Wait;

      match event {
        Event::LoopDestroyed => *control_flow = ControlFlow::Exit,
        Event::WindowEvent { ref event, .. } => match event {
          WindowEvent::Resized(physical_size) => {
            self.surface.resize(
              &self.context,
              physical_size.width.try_into().unwrap(),
              physical_size.height.try_into().unwrap(),
            );
          }
          WindowEvent::MouseInput {
            button: MouseButton::Left,
            state,
            ..
          } => match state {
            ElementState::Pressed => self.dragging = true,
            ElementState::Released => self.dragging = false,
          },
          WindowEvent::CursorMoved {
            device_id: _,
            position,
            ..
          } => {
            if self.dragging {
              let p0 = self
                .canvas
                .transform()
                .inversed()
                .transform_point(self.mousex, self.mousey);
              let p1 = self
                .canvas
                .transform()
                .inversed()
                .transform_point(position.x as f32, position.y as f32);

              self.canvas.translate(p1.0 - p0.0, p1.1 - p0.1);
            }

            self.mousex = position.x as f32;
            self.mousey = position.y as f32;
          }
          WindowEvent::MouseWheel {
            device_id: _,
            delta: winit::event::MouseScrollDelta::LineDelta(_, y),
            ..
          } => {
            let pt = self
              .canvas
              .transform()
              .inversed()
              .transform_point(self.mousex, self.mousey);

            self.canvas.translate(pt.0, pt.1);
            self.canvas.scale(1.0 + (y / 10.0), 1.0 + (y / 10.0));
            self.canvas.translate(-pt.0, -pt.1);
            trace!("Mouse wheel {}", y);
          }
          WindowEvent::KeyboardInput {
            input:
              KeyboardInput {
                virtual_keycode: Some(key),
                state: ElementState::Pressed,
                ..
              },
            ..
          } => self.handle_key(key),

          WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
          _ => trace!("Unhandled window event: {:?}", event),
        },

        Event::RedrawRequested(_) => self.redraw(),
        Event::MainEventsCleared => self.window.request_redraw(),
        Event::UserEvent(MapEvent::TileDataArrived { tile, data }) => {
          self.add_tile_image(tile, data)
        }
        _ => trace!("Unhandled event: {:?}", event),
      }
    });
  }

  pub async fn new() {
    let event_loop = EventLoopBuilder::<MapEvent>::with_user_event().build();
    let (canvas, window, context, surface) = {
      let window_builder = WindowBuilder::new()
        .with_inner_size(winit::dpi::PhysicalSize::new(1000, 1000))
        .with_resizable(true)
        .with_title("MapVas");
      let template = ConfigTemplateBuilder::new().with_alpha_size(8);

      let display_builder = DisplayBuilder::new().with_window_builder(Some(window_builder));
      let (window, gl_config) = display_builder
        .build(&event_loop, template, |configs| {
          configs
            .reduce(|accum, config| {
              let transparency_check = config.supports_transparency().unwrap_or(false)
                & !accum.supports_transparency().unwrap_or(false);

              if transparency_check || config.num_samples() < accum.num_samples() {
                config
              } else {
                accum
              }
            })
            .unwrap()
        })
        .unwrap();

      let window = window.unwrap();

      let raw_window_handle = Some(window.raw_window_handle());

      let gl_display = gl_config.display();

      let context_attributes = ContextAttributesBuilder::new().build(raw_window_handle);
      let fallback_context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(None))
        .build(raw_window_handle);
      let mut not_current_gl_context = Some(unsafe {
        gl_display
          .create_context(&gl_config, &context_attributes)
          .unwrap_or_else(|_| {
            gl_display
              .create_context(&gl_config, &fallback_context_attributes)
              .expect("failed to create context")
          })
      });

      let (width, height): (u32, u32) = window.inner_size().into();
      let raw_window_handle = window.raw_window_handle();
      let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        NonZeroU32::new(width).unwrap(),
        NonZeroU32::new(height).unwrap(),
      );

      let surface = unsafe {
        gl_config
          .display()
          .create_window_surface(&gl_config, &attrs)
          .unwrap()
      };

      let gl_context = not_current_gl_context
        .take()
        .unwrap()
        .make_current(&surface)
        .unwrap();

      let renderer =
        unsafe { OpenGl::new_from_function_cstr(|s| gl_display.get_proc_address(s) as *const _) }
          .expect("Cannot create renderer");

      let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
      canvas.set_size(width, height, window.scale_factor() as f32);

      (canvas, window, gl_context, surface)
    };

    Self {
      canvas,
      context,
      surface,
      window,
      dragging: false,
      mousey: 0.0,
      mousex: 0.0,
      tile_loader: CachedTileLoader::default(),
      event_proxy: event_loop.create_proxy(),
      loaded_images: HashMap::default(),
    }
    .run(event_loop)
    .await;
  }

  fn handle_key(&mut self, key: &VirtualKeyCode) {
    debug!("Key {:?}", key);
  }

  fn get_current_canvas_section(&self) -> (PixelPosition, PixelPosition, f32) {
    let mut trans = self.canvas.transform();
    let zoom = trans[0];
    trans.inverse();

    let nw = trans.transform_point(0., 0.);
    let size = self.window.inner_size();
    let se = trans.transform_point(size.width as f32, size.height as f32);
    (
      PixelPosition { x: nw.0, y: nw.1 },
      PixelPosition { x: se.0, y: se.1 },
      zoom,
    )
  }

  fn get_tiles_to_draw(&mut self) -> Vec<Tile> {
    let (nw, se, zoom) = self.get_current_canvas_section();

    let size = self.window.inner_size();
    let vertical_tile_number = (size.height as f32 / TILE_SIZE).round();

    let zoom_level = ((zoom * vertical_tile_number).log2() as i32).clamp(2, 19);
    let nw_tile = TileCoordinate::from_pixel_position(nw.clamp(), zoom_level as u8);
    let se_tile = TileCoordinate::from_pixel_position(se.clamp(), zoom_level as u8);
    tiles_in_box(nw_tile, se_tile)
  }

  fn draw_map(&mut self) {
    let tiles = &self.get_tiles_to_draw();
    debug!("Drawing {} tiles: {:?}", tiles.len(), tiles);
    for tile in tiles {
      let found_tile_image = self.find_image_or_download(*tile);
      if found_tile_image.is_none() {
        continue;
      }
      let (nw, se) = found_tile_image.unwrap().0.position();
      let fill_paint = Paint::image(
        *found_tile_image.unwrap().1,
        nw.x,
        nw.y,
        se.x - nw.x,
        se.y - nw.y,
        0.0,
        1.,
      );
      let mut path = Path::new();
      path.rect(nw.x, nw.y, se.x, se.y);
      self.canvas.fill_path(&path, &fill_paint);
    }
  }

  fn redraw(&mut self) {
    let dpi_factor = self.window.scale_factor();
    let size = self.window.inner_size();

    self
      .canvas
      .set_size(size.width, size.height, dpi_factor as f32);
    self
      .canvas
      .clear_rect(0, 0, size.width, size.height, Color::rgbf(0.3, 0.3, 0.32));

    self.draw_map();

    let mut path = Path::new();
    path.move_to(10., 10.);
    path.line_to(100., 200.);

    let stroke = Paint::color(Color::rgb(200, 0, 0));
    self.canvas.stroke_path(&path, &stroke);

    self.canvas.save();
    self.canvas.flush();
    self.surface.swap_buffers(&self.context).unwrap();
  }

  fn find_image_or_download(&self, tile: Tile) -> Option<(Tile, &ImageId)> {
    let image_id = self.loaded_images.get(&tile);
    match image_id {
      Some(id) => Some((tile, id)),
      None => {
        let tile_loader = self.tile_loader.clone();
        let event_proxy = self.event_proxy.clone();
        tokio::spawn(async move {
          match tile_loader.tile_data(&tile).await {
            Ok(data) => event_proxy
              .send_event(MapEvent::TileDataArrived { tile, data })
              .unwrap_or_default(),
            Err(_) => (),
          }
        });
        // Load parent tile instead
        let mut parent = tile.parent();
        while let Some(current_tile) = parent {
          let id = self.loaded_images.get(&current_tile);
          match id {
            Some(i) => return Some((current_tile, i)),
            _ => parent = current_tile.parent(),
          }
        }
        None
      }
    }
  }

  fn add_tile_image(&mut self, tile: Tile, data: Vec<u8>) {
    let image_id = self
      .canvas
      .load_image_mem(&data, ImageFlags::empty())
      .expect("Something went wrong when adding image.");
    self.loaded_images.insert(tile, image_id);
  }
}
