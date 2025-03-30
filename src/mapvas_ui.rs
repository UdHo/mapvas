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
}

impl eframe::App for MapApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    if self.sidebar.show() {
      egui::SidePanel::left("sidebar")
        .exact_width(self.sidebar.width)
        .resizable(true)
        .show(ctx, |ui| {
          self.sidebar.ui(ui);

          // Get the max x position (right boundary)
          let panel_rect = ui.max_rect();

          // Define a draggable area (5-pixel wide strip)
          let drag_rect = egui::Rect::from_min_max(
            panel_rect.right_top() + egui::vec2(-5.0, 0.0), // Left edge of drag area
            panel_rect.right_bottom() + egui::vec2(5.0, 0.0), // Right edge of drag area
          );

          // Make it interactive
          let response = ui.interact(
            drag_rect,
            ui.id().with("resize_handle"),
            egui::Sense::drag(),
          );

          // If dragging, update width
          if response.dragged() {
            self.sidebar.width += response.drag_delta().x;
            eprint!("Width: {}", self.sidebar.width);
            self.sidebar.width = self.sidebar.width.clamp(100.0, 400.0); // Min/Max limits
          }

          ui.painter()
            .rect_filled(drag_rect, 0.0, egui::Color32::GRAY);
        });
    }

    egui::CentralPanel::default()
      .frame(egui::Frame::NONE)
      .show(ctx, |ui| {
        (&mut self.map).ui(ui);
      });
  }
}

struct Sidebar {
  show: bool,
  width: f32,
  #[allow(dead_code)]
  remote: Remote,
  map_content: Rc<dyn MapLayerHolder>,
}

impl Sidebar {
  fn new(remote: Remote, map_content: Rc<dyn MapLayerHolder>) -> Self {
    Self {
      show: false,
      width: 200.0,
      remote,
      map_content,
    }
  }

  fn show(&self) -> bool {
    self.show
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
      ui.collapsing("Layers", |ui| {
        let mut layer_reader = self.map_content.get_reader();
        ui.vertical(|ui| {
          for layer in layer_reader.get_layers() {
            let name = layer.name().to_owned();
            ui.collapsing(name, |ui| {
              ui.checkbox(layer.visible_mut(), "visible");
            });
          }
        });
      });
    });
  }
}
