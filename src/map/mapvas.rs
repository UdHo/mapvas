use super::{
  coordinates::CANVAS_SIZE,
  coordinates::{
    tiles_in_box, BoundingBox, Coordinate, PixelPosition, Tile, TileCoordinate, TILE_SIZE,
  },
  map_event::FillStyle,
  map_event::{Layer, MapEvent, Style},
  tile_loader::{CachedTileLoader, TileLoader},
};

use crate::parser::{GrepParser, Parser};

use std::{cmp::max, collections::HashMap};
use std::{num::NonZeroU32, sync::Arc};

use arboard::Clipboard;
use async_std::task::block_on;
use femtovg::{renderer::OpenGl, Canvas, Path};
use femtovg::{Color, ImageFlags, ImageId, Paint};
use glutin::prelude::*;
use glutin::{
  config::ConfigTemplateBuilder,
  context::{ContextApi, ContextAttributesBuilder},
  display::GetGlDisplay,
  surface::{SurfaceAttributesBuilder, WindowSurface},
};
use glutin_winit::DisplayBuilder;
use log::{info, trace};
use raw_window_handle::HasRawWindowHandle;
use tokio::sync::mpsc::{Receiver, Sender};
use winit::{
  dpi::PhysicalPosition,
  event::{
    ElementState, Event, KeyboardInput, MouseButton, MouseScrollDelta, VirtualKeyCode, WindowEvent,
  },
  event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
  window::{Window, WindowBuilder},
};

#[derive(Debug)]
enum LayerElement {
  Polyline(Path, BoundingBox, Vec<PixelPosition>, Option<String>),
  Point(PixelPosition, Option<String>),
}

impl LayerElement {
  pub fn with_text(self, text: Option<String>) -> Self {
    match self {
      Self::Point(p, _) => Self::Point(p, text),
      Self::Polyline(a, b, c, _) => Self::Polyline(a, b, c, text),
    }
  }

  pub fn sq_distance_to_point(&self, p: PixelPosition, point_preference: f32) -> f32 {
    match self {
      Self::Polyline(_, _, coords, _) => coords
        .windows(2)
        .map(|points| p.sq_distance_line_segment(&points[0], &points[1]))
        .fold(f32::MAX, |min, d| min.min(d)),
      Self::Point(PixelPosition { x, y }, _) => {
        (p.x - x) * (p.x - x) + (p.y - y) * (p.y - y) - point_preference
      }
    }
  }

  pub fn get_text(&self) -> Option<String> {
    match self {
      Self::Polyline(_, _, _, t) => t.clone(),
      Self::Point(_, t) => t.clone(),
    }
  }

  pub fn has_text(&self) -> bool {
    match self {
      Self::Polyline(_, _, _, t) => t.is_some(),
      Self::Point(_, t) => t.is_some(),
    }
  }
}

#[allow(clippy::struct_field_names)]
struct MapEventHander {
  event_proxy: EventLoopProxy<MapEvent>,
  event_receiver: Option<Receiver<MapEvent>>,
  event_sender: Sender<MapEvent>,
}

struct MapProvider {
  loaded_images: HashMap<Tile, ImageId>,
  layers: HashMap<String, Vec<(LayerElement, Style)>>,
  tile_loader: Arc<CachedTileLoader>,
  event_sender: Sender<MapEvent>,
}

impl MapProvider {
  fn new(tile_loader: CachedTileLoader, event_sender: Sender<MapEvent>) -> Self {
    Self {
      tile_loader: Arc::new(tile_loader),
      event_sender,
      loaded_images: Default::default(),
      layers: Default::default(),
    }
  }

  fn layers_bounding_box(&self) -> Option<BoundingBox> {
    let mut bb = BoundingBox::get_invalid();
    self
      .layers
      .iter()
      .flat_map(|(_, elements)| elements.iter())
      .for_each(|e| match &e.0 {
        LayerElement::Point(p, _) => bb.add_coordinate(*p),
        LayerElement::Polyline(_, b, _, _) => bb.extend(b),
      });
    bb.is_valid().then_some(bb)
  }

  fn find_image_or_download(&self, tile: Tile) -> Option<(Tile, &ImageId)> {
    let image_id = self.loaded_images.get(&tile);
    if let Some(id) = image_id {
      Some((tile, id))
    } else {
      let tile_loader = self.tile_loader.clone();
      let sender = self.event_sender.clone();
      tokio::spawn(async move {
        if let Ok(data) = tile_loader.tile_data(&tile).await {
          let _ = sender.send(MapEvent::TileDataArrived { tile, data }).await;
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

  fn add_tile_image(&mut self, tile: Tile, image_id: ImageId) {
    self.loaded_images.insert(tile, image_id);
  }

  fn clear_layers(&mut self) {
    self.layers.clear();
  }
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
  map_provider: MapProvider,
  closest_text: String,
}

impl Default for MapVas {
  fn default() -> Self {
    MapVas::new()
  }
}

impl MapVas {
  /// Creates a non-running map widget.
  /// Showing the widget requires the main event loop to be started with ``Self::run()``.
  ///
  /// # Panics
  /// if something goes terribly wrong.
  #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
  #[must_use]
  pub fn new() -> MapVas {
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
        unsafe { OpenGl::new_from_function_cstr(|s| gl_display.get_proc_address(s).cast()) }
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
      event_handler: MapEventHander {
        event_proxy,
        event_receiver: Some(rx),
        event_sender: tx.clone(),
      },
      map_provider: MapProvider::new(CachedTileLoader::default(), tx),
      closest_text: Default::default(),
    }
  }

  /// Starts running the event loop.
  ///
  /// # Panics
  /// If it cannot start up.
  #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
  pub fn run(mut self) {
    let _ = self.canvas.add_font_mem(ttf_noto_sans::REGULAR);

    self.spawn_event_handler();
    self
      .event_loop
      .take()
      .expect("Main event loop started twice.")
      .run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
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
            WindowEvent::MouseInput {
              button: MouseButton::Right,
              ..
            } => self.update_closest(),

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
            } => self.handle_key(*key),

            WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
            WindowEvent::ScaleFactorChanged {
              scale_factor: _,
              new_inner_size,
            } => self.surface.resize(
              &self.context,
              new_inner_size.width.try_into().unwrap(),
              new_inner_size.height.try_into().unwrap(),
            ),

            _ => trace!("Unhandled window event: {:?}", event),
          },
          Event::RedrawRequested(_) => self.redraw(),
          Event::MainEventsCleared => self.window.request_redraw(),
          Event::UserEvent(MapEvent::TileDataArrived { tile, data }) => {
            self.add_tile_image(tile, &data);
          }
          Event::UserEvent(MapEvent::Layer(layer)) => self.handle_layer_event(layer),
          Event::UserEvent(MapEvent::Clear) => {
            self.map_provider.clear_layers();
          }
          Event::LoopDestroyed | Event::UserEvent(MapEvent::Shutdown) => {
            *control_flow = ControlFlow::Exit;
          }
          Event::UserEvent(MapEvent::Focus) => self.handle_focus_event(),
          _ => trace!("Unhandled event: {:?}", event),
        }
      });
  }

  /// Allows to send events from the outside the event loop.
  pub fn get_event_sender(&self) -> Sender<MapEvent> {
    self.event_handler.event_sender.clone()
  }

  fn draw_text(&mut self) {
    if self.closest_text.is_empty() {
      return;
    }
    let w = self.window.inner_size().width as f32;
    let h = 25.;
    let mut path = Path::new();
    path.rect(0., 0., w, h);
    self
      .canvas
      .fill_path(&path, &Paint::color(Color::rgba(128, 128, 128, 128)));
    let mut text_paint = Paint::color(Color::rgba(240, 240, 240, 255));
    text_paint.set_font_size(14.);
    let _ = self
      .canvas
      .fill_text(10., 15., &self.closest_text, &text_paint);
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

  fn handle_key(&mut self, key: VirtualKeyCode) {
    const SCROLL_SPEED: f32 = 20.;
    const ZOOM_SPEED: f32 = 1.1;
    match key {
      VirtualKeyCode::Left => self.translate(0., 0., SCROLL_SPEED, 0.),
      VirtualKeyCode::Right => self.translate(SCROLL_SPEED, 0., 0., 0.),
      VirtualKeyCode::Up => self.translate(0., 0., 0., SCROLL_SPEED),
      VirtualKeyCode::Down => self.translate(0., SCROLL_SPEED, 0., 0.),
      // Plus and equals to zoom in to avoid holding shift.
      VirtualKeyCode::Equals | VirtualKeyCode::Plus => self.zoom_canvas_center(ZOOM_SPEED),
      VirtualKeyCode::Minus => self.zoom_canvas_center(1. / ZOOM_SPEED),
      VirtualKeyCode::V => self.paste(),
      VirtualKeyCode::C => self.copy(),
      VirtualKeyCode::F => self.handle_focus_event(),
      VirtualKeyCode::L => self.update_closest(),
      _ => (),
    };
  }

  fn paste(&self) {
    let sender = self.get_event_sender();
    rayon::spawn(move || {
      if let Ok(text) = Clipboard::new().expect("clipboard").get_text() {
        if let Some(map_event) = GrepParser::new(false).parse_line(&text) {
          let _ = block_on(sender.send(map_event));
        }
      }
    });
  }

  fn copy(&self) {
    if self.closest_text.is_empty() {
      return;
    }
    let mut clipboard = Clipboard::new().unwrap();
    clipboard.set_text(&self.closest_text).unwrap();
  }

  #[allow(clippy::cast_precision_loss)]
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

  #[allow(unused)]
  fn print_coordinate(&self) {
    let (nw, _, zoom) = self.get_current_canvas_section();
    eprintln!(
      "{:?}",
      PixelPosition {
        x: nw.x + self.mousex / zoom,
        y: nw.y + self.mousey / zoom
      },
    );
  }

  #[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
  )]

  fn get_tiles_to_draw(&mut self) -> impl Iterator<Item = Tile> {
    let (nw, se, zoom) = self.get_current_canvas_section();

    let size = self.window.inner_size();
    let vertical_tile_number = (size.height as f32 / TILE_SIZE).round();

    let zoom_level = ((zoom * vertical_tile_number).log2() as i32).clamp(2, 19);
    let nw_tile = TileCoordinate::from_pixel_position(nw.clamp(), zoom_level as u8);
    let se_tile = TileCoordinate::from_pixel_position(se.clamp(), zoom_level as u8);
    tiles_in_box(nw_tile, se_tile)
  }

  fn draw_map(&mut self) {
    for tile in self.get_tiles_to_draw() {
      let found_tile_image = self.map_provider.find_image_or_download(tile);
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

  #[allow(clippy::cast_possible_truncation)]
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
    self.draw_layers();

    self.canvas.save();
    self.canvas.reset();
    self.draw_text();
    self.canvas.restore();

    self.canvas.flush();
    self.surface.swap_buffers(&self.context).unwrap();
  }

  fn draw_layers(&mut self) {
    let line_width = 3. / self.get_zoom_factor();
    for layer in &self.map_provider.layers {
      for (path, style) in layer.1 {
        let mut stroke = Paint::color(style.color.to_rgb());
        stroke.set_line_width(line_width);
        let fill = match style.fill {
          FillStyle::Transparent => Some(Paint::color(style.color.to_rgba(50))),
          FillStyle::Solid => Some(Paint::color(style.color.to_rgb())),
          FillStyle::NoFill => None,
        };

        match path {
          LayerElement::Polyline(poly, _, _, _) => {
            self.canvas.stroke_path(poly, &stroke);
            if let Some(style) = fill.as_ref() {
              self.canvas.fill_path(poly, style);
            };
          }
          LayerElement::Point(point, _) => {
            let mut circle = Path::new();
            circle.circle(
              point.x,
              point.y,
              (3. / self.get_zoom_factor()).max(0.000_05),
            );
            self.canvas.stroke_path(&circle, &stroke);
            if let Some(style) = fill.as_ref() {
              self.canvas.fill_path(&circle, style);
            };
          }
        };
      }
    }
  }

  fn add_tile_image(&mut self, tile: Tile, data: &[u8]) {
    let image_id = self.canvas.load_image_mem(data, ImageFlags::empty());
    if let Ok(id) = image_id {
      self.map_provider.add_tile_image(tile, id);
    } else {
      info!("Tile {tile:?} image decoding problem");
    }
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

  #[allow(clippy::cast_precision_loss)]
  fn set_center(&mut self, center: PixelPosition) {
    let new = self.canvas.transform().transform_point(center.x, center.y);
    let window_size = self.window.inner_size();
    self.translate(
      new.0,
      new.1,
      window_size.width as f32 / 2.,
      window_size.height as f32 / 2.,
    );
  }

  #[allow(clippy::cast_precision_loss)]
  fn fit_to_window(&mut self) {
    let window_size = self.window.inner_size();
    let ratio =
      max(window_size.width, window_size.height) as f32 / (self.get_zoom_factor() * CANVAS_SIZE);
    if ratio > 1. {
      self.zoom_canvas_center(ratio);
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
    let corrected_factor = factor.clamp(1. / current_factor, 2.0f32.powi(19) / current_factor);
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

  #[allow(clippy::cast_precision_loss)]
  fn zoom_canvas_center(&mut self, factor: f32) {
    let size = self.window.inner_size();
    self.zoom_canvas(factor, size.width as f32 / 2., size.height as f32 / 2.);
  }

  fn coords_to_element(coords: &[Coordinate], close_path: bool) -> LayerElement {
    let points = coords
      .iter()
      .skip(1)
      .copied()
      .map(Into::<PixelPosition>::into);
    if coords.len() == 1 {
      LayerElement::Point(coords[0].into(), None)
    } else {
      let mut path = Path::new();

      let start: PixelPosition = coords[0].into();
      path.move_to(start.x, start.y);
      points.clone().for_each(|to| {
        path.line_to(to.x, to.y);
      });
      if close_path {
        path.line_to(start.x, start.y);
      }
      LayerElement::Polyline(
        path,
        BoundingBox::from_iterator(coords.iter().copied().map(Into::into)),
        coords.iter().copied().map(Into::into).collect(),
        None,
      )
    }
  }

  #[allow(clippy::cast_precision_loss)]
  fn handle_focus_event(&mut self) {
    let bb = self.map_provider.layers_bounding_box().unwrap_or_default();
    if !bb.is_valid() {
      return;
    }

    let window_size = self.window.inner_size();
    let empty_space_width = 30;
    let requested_zoom_x = (window_size.width - empty_space_width) as f32 / (bb.width() + 0.00001);
    let requested_zoom_y =
      (window_size.height - empty_space_width) as f32 / (bb.height() + 0.00001);
    self.zoom_canvas_center(requested_zoom_x.min(requested_zoom_y) / self.get_zoom_factor());
    self.fit_to_window();
    self.set_center(bb.center());
  }

  fn handle_layer_event(&mut self, layer: Layer) {
    let mut paths: Vec<(LayerElement, Style)> = layer
      .shapes
      .iter()
      .map(|shape| {
        (
          Self::coords_to_element(&shape.coordinates, shape.style.fill != FillStyle::NoFill)
            .with_text(shape.label.clone()),
          shape.style,
        )
      })
      .collect();

    self
      .map_provider
      .layers
      .entry(layer.id)
      .and_modify(|l| l.append(&mut paths))
      .or_insert(paths);
  }

  fn update_closest(&mut self) {
    let mut trans = self.canvas.transform();
    trans.inverse();
    let pos = trans.transform_point(self.mousex, self.mousey);
    let mouse = PixelPosition { x: pos.0, y: pos.1 };

    let (a, b, _) = self.get_current_canvas_section();
    let dist_treshold = (b.x - a.x) / 80.;
    let point_preference_weight = dist_treshold / 4.;

    let (closest, dist) = self
      .map_provider
      .layers
      .iter()
      .flat_map(|l| l.1.iter().map(|e| &e.0))
      .fold((None, f32::MAX), |(el, dist), next| {
        let next_dist = next.sq_distance_to_point(mouse, point_preference_weight);
        if next_dist < dist && next.has_text() {
          (Some(next), next_dist)
        } else {
          (el, dist)
        }
      });
    self.closest_text =
      if let (Some(closest), true) = (closest, dist < dist_treshold * dist_treshold) {
        closest.get_text().unwrap_or("".into())
      } else {
        "".into()
      };
  }
}

#[cfg(test)]
mod test {
  #[test]
  fn image_loading_test() {}
}
