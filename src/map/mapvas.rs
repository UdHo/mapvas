use crate::map::{coordinates::CANVAS_SIZE, map_event::FillStyle};

use super::{
  coordinates::{tiles_in_box, Coordinate, PixelPosition, Tile, TileCoordinate, TILE_SIZE},
  map_event::{Layer, MapEvent, Style},
  tile_loader::{CachedTileLoader, TileLoader},
};
use femtovg::{renderer::OpenGl, Canvas, Path};
use femtovg::{Color, ImageFlags, ImageId, Paint};
use glutin::prelude::*;
use glutin::{
  config::ConfigTemplateBuilder,
  context::{ContextApi, ContextAttributesBuilder},
  display::GetGlDisplay,
  surface::{SurfaceAttributesBuilder, WindowSurface},
};
use log::{error, trace};

use glutin_winit::DisplayBuilder;
use raw_window_handle::HasRawWindowHandle;
use std::num::NonZeroU32;
use std::{cmp::max, collections::HashMap};
use tokio::sync::mpsc::{Receiver, Sender};
use winit::event_loop::EventLoopBuilder;
use winit::window::WindowBuilder;
use winit::{
  dpi::PhysicalPosition,
  event::{ElementState, KeyboardInput, MouseButton, MouseScrollDelta, VirtualKeyCode},
};
use winit::{
  event::{Event, WindowEvent},
  event_loop::{ControlFlow, EventLoop, EventLoopProxy},
  window::Window,
};

enum LayerElement {
  Polyline(Path),
  Point(PixelPosition),
}

struct MapEventHander {
  event_proxy: EventLoopProxy<MapEvent>,
  event_receiver: Option<Receiver<MapEvent>>,
  event_sender: Sender<MapEvent>,
}

/// Keeps data for map and layer drawing.
pub struct MapVas {
  event_loop: Option<EventLoop<MapEvent>>,
  canvas: Canvas<OpenGl>,
  context: glutin::context::PossiblyCurrentContext,
  surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
  window: Window,
  dragging: bool,
  mousex: f32,
  mousey: f32,
  event_handler: MapEventHander,
  tile_loader: CachedTileLoader,
  loaded_images: HashMap<Tile, ImageId>,
  layers: HashMap<String, Vec<(LayerElement, Style)>>,
}

impl MapVas {
  /// Creates a non-running map widget.
  /// Showing the widget requires the main event loop to be started with [Self::run()].
  pub fn new(window: u16) -> MapVas {
    let event_loop = EventLoopBuilder::<MapEvent>::with_user_event().build();
    let window_name = if window == 0 {
      format!("")
    } else {
      format!(": {}", window)
    };
    let title = format!("MapVas{}", window_name);
    let (canvas, window, context, surface) = {
      let window_builder = WindowBuilder::new()
        .with_inner_size(winit::dpi::PhysicalSize::new(1000, 1000))
        .with_resizable(true)
        .with_title(title);
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

    let event_proxy = event_loop.create_proxy();
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    Self {
      event_loop: Some(event_loop),
      canvas,
      context,
      surface,
      window,
      dragging: false,
      mousey: 0.0,
      mousex: 0.0,
      tile_loader: CachedTileLoader::default(),
      event_handler: MapEventHander {
        event_proxy,
        event_receiver: Some(rx),
        event_sender: tx,
      },
      loaded_images: HashMap::default(),
      layers: HashMap::default(),
    }
  }

  /// Starts running the event loop.
  pub async fn run(mut self) {
    self.spawn_event_handler();

    self
      .event_loop
      .take()
      .expect("Main event loop started twice.")
      .run(move |event, _, control_flow| {
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
                self.translate(
                  self.mousex,
                  self.mousey,
                  position.x as f32,
                  position.y as f32,
                );
              }
              self.mousex = position.x as f32;
              self.mousey = position.y as f32;
            }
            WindowEvent::MouseWheel {
              device_id: _,
              delta,
              ..
            } => {
              let change = match delta {
                MouseScrollDelta::LineDelta(_, y) => *y,
                MouseScrollDelta::PixelDelta(PhysicalPosition { x, y }) => {
                  let max_abs = {
                    if x.abs() > y.abs() {
                      *x as f32
                    } else {
                      *y as f32
                    }
                  };
                  max_abs / 10.
                }
              };
              self.zoom_canvas(1.0 + (change / 10.0), self.mousex, self.mousey);
            }
            WindowEvent::TouchpadMagnify {
              device_id: _,
              delta,
              ..
            } => {
              self.zoom_canvas(1.0 + *delta as f32, self.mousex, self.mousey);
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
          Event::UserEvent(MapEvent::Layer(layer)) => self.handle_layer_event(layer),
          Event::UserEvent(MapEvent::Clear) => {
            error!("Clear event received");
            self.layers.clear();
          }
          Event::UserEvent(MapEvent::Shutdown) => *control_flow = ControlFlow::Exit,
          _ => trace!("Unhandled event: {:?}", event),
        }
      });
  }

  /// Allows to send events from the outside the event loop.
  pub fn get_event_sender(&self) -> Sender<MapEvent> {
    self.event_handler.event_sender.clone()
  }

  fn spawn_event_handler(&mut self) {
    let proxy = self.event_handler.event_proxy.clone();
    let mut receiver = self
      .event_handler
      .event_receiver
      .take()
      .expect("Receiver taken?");
    tokio::spawn(async move {
      while let Some(event) = receiver.recv().await {
        let _ = proxy.send_event(event);
      }
    });
  }

  fn handle_key(&mut self, key: &VirtualKeyCode) {
    const SCROLL_SPEED: f32 = 20.;
    const ZOOM_SPEED: f32 = 1.1;
    match key {
      VirtualKeyCode::Left => self.translate(0., 0., SCROLL_SPEED, 0.),
      VirtualKeyCode::Right => self.translate(SCROLL_SPEED, 0., 0., 0.),
      VirtualKeyCode::Up => self.translate(0., 0., 0., SCROLL_SPEED),
      VirtualKeyCode::Down => self.translate(0., SCROLL_SPEED, 0., 0.),
      // Plus and equals to zoom in to avoid holding shift.
      VirtualKeyCode::Equals => self.zoom_canvas_center(ZOOM_SPEED),
      VirtualKeyCode::Plus => self.zoom_canvas_center(ZOOM_SPEED),
      VirtualKeyCode::Minus => self.zoom_canvas_center(1. / ZOOM_SPEED),
      _ => (),
    };
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
    trace!("Drawing {} tiles: {:?}", tiles.len(), tiles);
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
    self.fit_to_window();
    let dpi_factor = self.window.scale_factor();
    let size = self.window.inner_size();

    self
      .canvas
      .set_size(size.width, size.height, dpi_factor as f32);
    self
      .canvas
      .clear_rect(0, 0, size.width, size.height, Color::rgbf(0.3, 0.3, 0.32));

    self.draw_map();

    let line_width = 3. / self.get_zoom_factor();
    for layer in &self.layers {
      for (path, style) in layer.1 {
        let mut stroke = Paint::color(style.color.to_rgb());
        stroke.set_line_width(line_width);
        let fill = match style.fill {
          FillStyle::Transparent => Some(Paint::color(style.color.to_rgba(50))),
          FillStyle::Solid => Some(Paint::color(style.color.to_rgb())),
          FillStyle::NoFill => None,
        };

        match path {
          LayerElement::Polyline(poly) => {
            self.canvas.stroke_path(poly, &stroke);
            fill
              .as_ref()
              .map(|style| self.canvas.fill_path(&poly, style));
          }
          LayerElement::Point(point) => {
            let mut circle = Path::new();
            circle.circle(point.x, point.y, 3. / self.get_zoom_factor());
            self.canvas.stroke_path(&circle, &stroke);
            fill
              .as_ref()
              .map(|style| self.canvas.fill_path(&circle, style));
          }
        };
      }
    }
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
        let sender = self.get_event_sender();
        tokio::spawn(async move {
          match tile_loader.tile_data(&tile).await {
            Ok(data) => {
              let _ = sender.send(MapEvent::TileDataArrived { tile, data }).await;
            }
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
    let image_id = self.canvas.load_image_mem(&data, ImageFlags::empty());
    match image_id {
      Ok(id) => {
        let _ = self.loaded_images.insert(tile, id);
      }
      Err(e) => {
        error!(
          "Error {:?} loading image for {:?} with {:?}.",
          e, tile, data
        );
      }
    };
  }

  fn translate(&mut self, to_x: f32, to_y: f32, from_x: f32, from_y: f32) {
    let p0 = self
      .canvas
      .transform()
      .inversed()
      .transform_point(to_x, to_y);
    let p1 = self
      .canvas
      .transform()
      .inversed()
      .transform_point(from_x, from_y);
    self.canvas.translate(p1.0 - p0.0, p1.1 - p0.1);
  }

  fn fit_to_window(&mut self) {
    let window_size = self.window.inner_size();
    let ratio =
      max(window_size.width, window_size.height) as f32 / (self.get_zoom_factor() * CANVAS_SIZE);
    if ratio > 1. {
      self.zoom_canvas_center(ratio)
    }
    let section = self.get_current_canvas_section();
    self.translate(0., 0., section.0.x.min(0.), section.0.y.min(0.));
    self.translate(
      CANVAS_SIZE,
      CANVAS_SIZE,
      section.1.x.max(CANVAS_SIZE),
      section.1.y.max(CANVAS_SIZE),
    );
  }

  fn zoom_canvas(&mut self, factor: f32, center_x: f32, center_y: f32) {
    let current_factor = self.get_zoom_factor();
    let corrected_factor = factor.clamp(1. / current_factor, 2.0f32.powi(15) / current_factor);
    let pt = self
      .canvas
      .transform()
      .inversed()
      .transform_point(center_x, center_y);

    self.canvas.translate(pt.0, pt.1);
    self.canvas.scale(corrected_factor, corrected_factor);
    self.canvas.translate(-pt.0, -pt.1);
  }

  fn get_zoom_factor(&self) -> f32 {
    self.get_current_canvas_section().2
  }

  fn zoom_canvas_center(&mut self, factor: f32) {
    let size = self.window.inner_size();
    self.zoom_canvas(factor, size.width as f32 / 2., size.height as f32 / 2.);
  }

  fn coords_to_element(coords: &Vec<Coordinate>, close_path: bool) -> LayerElement {
    let points: Vec<PixelPosition> = coords.iter().map(|c| PixelPosition::from(*c)).collect();
    match points.len() {
      1 => LayerElement::Point(points[0]),
      _ => {
        let mut path = Path::new();
        let start = points[0];
        path.move_to(start.x, start.y);
        for to in points.iter().skip(1) {
          path.line_to(to.x, to.y);
        }
        if close_path {
          path.line_to(start.x, start.y);
        }
        LayerElement::Polyline(path)
      }
    }
  }

  fn handle_layer_event(&mut self, layer: Layer) {
    let mut paths: Vec<(LayerElement, Style)> = layer
      .shapes
      .iter()
      .map(|shape| {
        (
          Self::coords_to_element(&shape.coordinates, shape.style.fill != FillStyle::NoFill),
          shape.style,
        )
      })
      .collect();

    self
      .layers
      .entry(layer.id)
      .and_modify(|l| l.append(&mut paths))
      .or_insert(paths);
  }
}
