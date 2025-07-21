use std::rc::Rc;

use egui::Widget as _;

use crate::{
  map::mapvas_egui::{Map, MapLayerHolder},
  remote::Remote,
};

/// Holds the UI data of mapvas.
pub struct MapApp {
  map: Map,
  sidebar: Sidebar,
}

impl MapApp {
  pub fn new(map: Map, remote: Remote, map_content: Rc<dyn MapLayerHolder>) -> Self {
    let sidebar = Sidebar::new(remote, map_content);
    Self { map, sidebar }
  }

  /// Show the resize handle for the sidebar
  fn show_resize_handle(&mut self, ui: &mut egui::Ui) {
    let panel_rect = ui.max_rect();

    // Define a draggable area (8-pixel wide strip for better usability)
    let drag_rect = egui::Rect::from_min_max(
      panel_rect.right_top() + egui::vec2(-4.0, 0.0),
      panel_rect.right_bottom() + egui::vec2(4.0, 0.0),
    );

    let response = ui.interact(
      drag_rect,
      ui.id().with("resize_handle"),
      egui::Sense::drag(),
    );

    // Change cursor when hovering
    if response.hovered() {
      ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // Handle dragging
    if response.dragged() {
      self.sidebar.width += response.drag_delta().x;
      self.sidebar.width = self.sidebar.width.clamp(200.0, 600.0);
    }

    // Draw resize handle with hover effect
    let handle_color = if response.hovered() {
      egui::Color32::from_gray(180)
    } else {
      egui::Color32::from_gray(120)
    };
    
    ui.painter().rect_filled(
      drag_rect,
      2.0,
      handle_color,
    );

    // Draw resize grip pattern
    let center_y = drag_rect.center().y;
    for i in 0..3 {
      #[allow(clippy::cast_precision_loss)]
      let y = center_y + (i as f32 - 1.0) * 4.0;
      ui.painter().circle_filled(
        egui::pos2(drag_rect.center().x, y),
        1.0,
        egui::Color32::from_gray(80),
      );
    }
  }

  /// Show the sidebar toggle button when sidebar is hidden
  fn show_sidebar_toggle_button(&mut self, ctx: &egui::Context) {
    // Use a simple area for the toggle button
    egui::Area::new(egui::Id::new("sidebar_toggle"))
      .fixed_pos(egui::pos2(12.0, 12.0))
      .show(ctx, |ui| {
        let button_response = ui.add_sized(
          [36.0, 36.0],
          egui::Button::new("")
            .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 180))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 60)))
        );

        // Draw the hamburger menu icon on top of the button
        let center = button_response.rect.center();
        let icon_color = egui::Color32::from_gray(80);
        
        // Draw three horizontal lines to represent sidebar/menu
        let line_width = 16.0;
        let line_height = 2.0;
        let line_spacing = 4.0;
        
        for i in 0..3 {
          #[allow(clippy::cast_precision_loss)]
          let y_offset = (i as f32 - 1.0) * line_spacing;
          let line_rect = egui::Rect::from_center_size(
            egui::pos2(center.x, center.y + y_offset),
            egui::vec2(line_width, line_height)
          );
          ui.painter().rect_filled(line_rect, 1.0, icon_color);
        }
        
        // Handle click
        if button_response.clicked() {
          self.sidebar.show();
        }
        
        // Set cursor and show tooltip
        if button_response.hovered() {
          ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
          button_response.on_hover_text("Show sidebar (F1 or Ctrl+B)");
        }
      });
  }
}

impl eframe::App for MapApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Handle keyboard shortcut for sidebar toggle
    ctx.input(|i| {
      if i.key_pressed(egui::Key::F1) || 
         (i.modifiers.ctrl && i.key_pressed(egui::Key::B)) {
        self.sidebar.toggle();
      }
    });

    // Update sidebar animation
    self.sidebar.update_animation(ctx);

    // Show sidebar with smooth animations
    let effective_width = self.sidebar.get_animated_width();
    
    if effective_width > 1.0 {
      egui::SidePanel::left("sidebar")
        .exact_width(effective_width)
        .resizable(self.sidebar.is_fully_visible())
        .show(ctx, |ui| {
          // Add sidebar content with fade effect
          let alpha = self.sidebar.get_content_alpha();
          ui.set_opacity(alpha);
          
          self.sidebar.ui(ui);

          // Only show resize handle when sidebar is fully visible
          if self.sidebar.is_fully_visible() {
            self.show_resize_handle(ui);
          }
        });
    }

    // Show toggle button when sidebar is hidden or partially hidden
    if !self.sidebar.is_fully_visible() {
      self.show_sidebar_toggle_button(ctx);
    }

    egui::CentralPanel::default()
      .frame(egui::Frame::NONE)
      .show(ctx, |ui| {
        (&mut self.map).ui(ui);
      });
  }
}

struct Sidebar {
  visible: bool,
  target_visible: bool,
  width: f32,
  animation_progress: f32,
  last_frame_time: f64,
  #[allow(dead_code)]
  remote: Remote,
  map_content: Rc<dyn MapLayerHolder>,
}

impl Sidebar {
  fn new(remote: Remote, map_content: Rc<dyn MapLayerHolder>) -> Self {
    Self {
      visible: true,
      target_visible: true,
      width: 300.0,
      animation_progress: 1.0,
      last_frame_time: 0.0,
      remote,
      map_content,
    }
  }

  /// Toggle sidebar visibility with smooth animation
  fn toggle(&mut self) {
    self.target_visible = !self.target_visible;
  }

  /// Show the sidebar (with animation)
  fn show(&mut self) {
    self.target_visible = true;
  }

  /// Hide the sidebar (with animation)
  fn hide(&mut self) {
    self.target_visible = false;
  }

  /// Update the animation progress
  fn update_animation(&mut self, ctx: &egui::Context) {
    let current_time = ctx.input(|i| i.time);
    let dt = if self.last_frame_time == 0.0 {
      0.016 // First frame, assume 60fps
    } else {
      (current_time - self.last_frame_time).min(0.1) // Cap at 100ms
    };
    self.last_frame_time = current_time;

    // Animation speed (duration in seconds)
    let animation_speed = 4.0; // Complete animation in 0.25 seconds
    #[allow(clippy::cast_possible_truncation)]
    let delta_per_second = animation_speed * (dt as f32);

    if self.target_visible && self.animation_progress < 1.0 {
      self.animation_progress = (self.animation_progress + delta_per_second).min(1.0);
      ctx.request_repaint();
    } else if !self.target_visible && self.animation_progress > 0.0 {
      self.animation_progress = (self.animation_progress - delta_per_second).max(0.0);
      ctx.request_repaint();
    }

    self.visible = self.animation_progress > 0.0;
  }

  /// Get the current animated width for the sidebar
  fn get_animated_width(&self) -> f32 {
    // Use easing function for smooth animation
    let eased_progress = Self::ease_out_cubic(self.animation_progress);
    self.width * eased_progress
  }

  /// Get the content alpha for fade effect
  fn get_content_alpha(&self) -> f32 {
    // Fade content slightly faster than width animation for better UX
    let content_progress = (self.animation_progress - 0.2).max(0.0) / 0.8;
    Self::ease_out_cubic(content_progress)
  }

  /// Check if sidebar is fully visible
  fn is_fully_visible(&self) -> bool {
    self.animation_progress >= 0.99
  }

  /// Easing function for smooth animations
  fn ease_out_cubic(t: f32) -> f32 {
    let t = t - 1.0;
    t * t * t + 1.0
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    ui.vertical(|ui| {
      // Sidebar header with close button
      ui.horizontal(|ui| {
        ui.heading("Layers");
        
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          // Custom close button with better styling
          let close_button_size = egui::vec2(24.0, 24.0);
          let (close_rect, close_response) = ui.allocate_exact_size(close_button_size, egui::Sense::click());
          
          if close_response.hovered() {
            ui.painter().rect_filled(close_rect, 4.0, egui::Color32::from_gray(200));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
          }
          
          // Draw X symbol with better styling
          let center = close_rect.center();
          let size = 8.0;
          let color = egui::Color32::from_gray(100);
          let stroke_width = 1.5;
          
          // Draw the X
          ui.painter().line_segment(
            [center + egui::vec2(-size/2.0, -size/2.0), center + egui::vec2(size/2.0, size/2.0)],
            egui::Stroke::new(stroke_width, color)
          );
          ui.painter().line_segment(
            [center + egui::vec2(-size/2.0, size/2.0), center + egui::vec2(size/2.0, -size/2.0)],
            egui::Stroke::new(stroke_width, color)
          );
          
          if close_response.clicked() {
            self.hide();
          }
          
          // Add tooltip
          if close_response.hovered() {
            close_response.on_hover_text("Hide sidebar (F1 or Ctrl+B)");
          }
        });
      });

      ui.separator();

      // Layer content
      egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
          egui::CollapsingHeader::new("Map Layers")
            .default_open(true)
            .show(ui, |ui| {
              let mut layer_reader = self.map_content.get_reader();
              ui.vertical(|ui| {
                for layer in layer_reader.get_layers() {
                  layer.ui(ui);
                }
              });
            });

          ui.separator();

          // Additional sidebar content can be added here
          egui::CollapsingHeader::new("Settings")
            .default_open(false)
            .show(ui, |ui| {
              ui.label("Map settings and controls will appear here");
            });
        });
    });
  }
}
