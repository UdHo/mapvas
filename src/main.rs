use femtovg::{renderer::OpenGl, Canvas, Path};
use femtovg::{Color, FillRule, ImageFlags, Paint};
use glutin::prelude::*;
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder},
    display::GetGlDisplay,
    surface::{SurfaceAttributesBuilder, WindowSurface},
};
use log::{debug, info, trace};

use glutin_winit::DisplayBuilder;
use raw_window_handle::HasRawWindowHandle;
use std::num::NonZeroU32;
use winit::event::{ElementState, KeyboardInput, MouseButton, VirtualKeyCode};
use winit::window::WindowBuilder;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

struct MapVas {}

impl MapVas {
    fn run(
        &mut self,
        mut canvas: Canvas<OpenGl>,
        el: EventLoop<()>,
        #[cfg(not(target_arch = "wasm32"))] context: glutin::context::PossiblyCurrentContext,
        #[cfg(not(target_arch = "wasm32"))] surface: glutin::surface::Surface<
            glutin::surface::WindowSurface,
        >,
        window: Window,
    ) {
        let mut dragging = false;
        let mut mousex = 0.;
        let mut mousey = 0.;
        let mut screenshot_image_id = None;

        el.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            match event {
                Event::LoopDestroyed => *control_flow = ControlFlow::Exit,
                Event::WindowEvent { ref event, .. } => match event {
                    WindowEvent::Resized(physical_size) => {
                        surface.resize(
                            &context,
                            physical_size.width.try_into().unwrap(),
                            physical_size.height.try_into().unwrap(),
                        );
                    }
                    WindowEvent::MouseInput {
                        button: MouseButton::Left,
                        state,
                        ..
                    } => match state {
                        ElementState::Pressed => dragging = true,
                        ElementState::Released => dragging = false,
                    },
                    WindowEvent::CursorMoved {
                        device_id: _,
                        position,
                        ..
                    } => {
                        if dragging {
                            let p0 = canvas
                                .transform()
                                .inversed()
                                .transform_point(mousex, mousey);
                            let p1 = canvas
                                .transform()
                                .inversed()
                                .transform_point(position.x as f32, position.y as f32);

                            canvas.translate(p1.0 - p0.0, p1.1 - p0.1);
                        }

                        mousex = position.x as f32;
                        mousey = position.y as f32;
                    }
                    WindowEvent::MouseWheel {
                        device_id: _,
                        delta: winit::event::MouseScrollDelta::LineDelta(_, y),
                        ..
                    } => {
                        let pt = canvas
                            .transform()
                            .inversed()
                            .transform_point(mousex, mousey);

                        canvas.translate(pt.0, pt.1);
                        canvas.scale(1.0 + (y / 10.0), 1.0 + (y / 10.0));
                        canvas.translate(-pt.0, -pt.1);
                        debug!("Mouse wheel {}", y);
                    }
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                virtual_keycode: Some(VirtualKeyCode::S),
                                state: ElementState::Pressed,
                                ..
                            },
                        ..
                    } => {
                        info!("Screenshot taken.");
                        if let Some(screenshot_image_id) = screenshot_image_id {
                            canvas.delete_image(screenshot_image_id);
                        }

                        if let Ok(image) = canvas.screenshot() {
                            screenshot_image_id = Some(
                                canvas
                                    .create_image(image.as_ref(), ImageFlags::empty())
                                    .unwrap(),
                            );
                        }
                    }
                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    _ => (),
                },

                Event::RedrawRequested(_) => {
                    let dpi_factor = window.scale_factor();
                    debug!("Scale factor: {}", dpi_factor);
                    let size = window.inner_size();

                    canvas.set_size(size.width, size.height, dpi_factor as f32);
                    canvas.clear_rect(0, 0, size.width, size.height, Color::rgbf(0.3, 0.3, 0.32));

                    let mut path = Path::new();
                    path.move_to(10., 10.);
                    path.line_to(100., 200.);
                    let contains = canvas.contains_point(&path, mousex, mousey, FillRule::NonZero);
                    let stroke = Paint::color(match contains {
                        true => Color::rgb(200, 0, 0),
                        false => Color::rgb(0, 200, 0),
                    });
                    canvas.stroke_path(&path, &stroke);

                    canvas.save();
                    canvas.flush();
                    surface.swap_buffers(&context).unwrap();
                }

                Event::MainEventsCleared => window.request_redraw(),
                _ => trace!("Unhandled event: {:?}", event),
            }
        });
    }

    pub fn start(&mut self) {
        let event_loop = EventLoop::new();
        let (canvas, window, context, surface) = {
            let window_builder = WindowBuilder::new()
                .with_inner_size(winit::dpi::PhysicalSize::new(400, 400))
                .with_resizable(true)
                .with_title("canvas");

            let template = ConfigTemplateBuilder::new().with_alpha_size(8);

            let display_builder = DisplayBuilder::new().with_window_builder(Some(window_builder));
            let (window, gl_config) = display_builder
                .build(&event_loop, template, |configs| {
                    // Find the config with the maximum number of samples, so our triangle will
                    // be smooth.
                    configs
                        .reduce(|accum, config| {
                            let transparency_check =
                                config.supports_transparency().unwrap_or(false)
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

            let renderer = unsafe {
                OpenGl::new_from_function_cstr(|s| gl_display.get_proc_address(s) as *const _)
            }
            .expect("Cannot create renderer");

            let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
            canvas.set_size(width, height, window.scale_factor() as f32);

            (canvas, window, gl_context, surface)
        };

        self.run(canvas, event_loop, context, surface, window);
    }
}
#[tokio::main]
async fn main() {
    env_logger::init();

    MapVas {}.start();
}
