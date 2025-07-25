use super::{Layer, LayerProperties, drawable::Drawable as _};
use crate::map::{
  coordinates::{BoundingBox, Coordinate, PixelCoordinate, Transform, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata, Style},
  map_event::{Layer as EventLayer, MapEvent},
};
use egui::{Color32, Pos2, Rect, Ui};
use std::{
  collections::HashMap,
  sync::{
    Arc,
    mpsc::{Receiver, Sender},
  },
};

/// A layer that draws shapes on the map.
pub struct ShapeLayer {
  shape_map: HashMap<String, Vec<Geometry<PixelCoordinate>>>,
  layer_visibility: HashMap<String, bool>,
  geometry_visibility: HashMap<(String, usize), bool>,
  recv: Arc<Receiver<MapEvent>>,
  send: Sender<MapEvent>,
  layer_properties: LayerProperties,
  highlighted_geometry: Option<(String, usize)>,
}

impl ShapeLayer {
  #[must_use]
  pub fn new() -> Self {
    let (send, recv) = std::sync::mpsc::channel();

    Self {
      shape_map: HashMap::new(),
      layer_visibility: HashMap::new(),
      geometry_visibility: HashMap::new(),
      recv: recv.into(),
      send,
      layer_properties: LayerProperties::default(),
      highlighted_geometry: None,
    }
  }

  fn handle_new_shapes(&mut self) {
    for event in self.recv.try_iter() {
      if let MapEvent::Layer(EventLayer { id, geometries }) = event {
        let l = self.shape_map.entry(id.clone()).or_default();
        let start_idx = l.len();
        l.extend(geometries.into_iter());
        self.layer_visibility.entry(id.clone()).or_insert(true);

        for i in start_idx..l.len() {
          self
            .geometry_visibility
            .entry((id.clone(), i))
            .or_insert(true);
        }
      }
    }
  }

  #[must_use]
  pub fn get_sender(&self) -> Sender<MapEvent> {
    self.send.clone()
  }

  fn show_shape_layers(&mut self, ui: &mut egui::Ui) {
    let layer_ids: Vec<String> = self.shape_map.keys().cloned().collect();

    for layer_id in layer_ids {
      let shapes_count = self.shape_map.get(&layer_id).map_or(0, Vec::len);

      let has_highlighted_geometry = self
        .highlighted_geometry
        .as_ref()
        .is_some_and(|(highlighted_layer_id, _)| highlighted_layer_id == &layer_id);

      let mut header =
        egui::CollapsingHeader::new(format!("📁 {layer_id} ({shapes_count})")).id_salt(&layer_id);

      if has_highlighted_geometry {
        header = header.open(Some(true));
      }

      let header_response = header.show(ui, |ui| {
        if let Some(shapes) = self.shape_map.get(&layer_id).cloned() {
          for (shape_idx, shape) in shapes.iter().enumerate() {
            self.show_shape_ui(ui, &layer_id, shape_idx, shape);
            if shape_idx < shapes.len() - 1 {
              ui.separator();
            }
          }
        }
      });

      header_response.header_response.context_menu(|ui| {
        let layer_visible = *self.layer_visibility.get(&layer_id).unwrap_or(&true);
        let visibility_text = if layer_visible {
          "Hide Layer"
        } else {
          "Show Layer"
        };

        if ui.button(visibility_text).clicked() {
          self
            .layer_visibility
            .insert(layer_id.clone(), !layer_visible);
          ui.close();
        }

        ui.separator();

        if ui.button("🗑 Delete Layer").clicked() {
          self.shape_map.remove(&layer_id);
          self.layer_visibility.remove(&layer_id);
          self
            .geometry_visibility
            .retain(|(lid, _), _| lid != &layer_id);
          ui.close();
        }
      });
    }
  }

  fn show_shape_ui(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    shape: &Geometry<PixelCoordinate>,
  ) {
    let geometry_key = (layer_id.to_string(), shape_idx);
    let geometry_visible = *self.geometry_visibility.get(&geometry_key).unwrap_or(&true);
    let is_highlighted = self.highlighted_geometry.as_ref() == Some(&geometry_key);

    let bg_color = if is_highlighted {
      Some(egui::Color32::from_rgb(100, 149, 237))
    } else {
      None
    };

    let frame = if let Some(color) = bg_color {
      egui::Frame::default()
        .fill(color)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(4))
    } else {
      egui::Frame::default()
    };

    frame.show(ui, |ui| {
      ui.horizontal(|ui| {
        let visibility_icon = if geometry_visible { "👁" } else { "🚫" };
        if ui
          .add_sized([24.0, 20.0], egui::Button::new(visibility_icon))
          .clicked()
        {
          self
            .geometry_visibility
            .insert(geometry_key.clone(), !geometry_visible);
        }

        let content_response = ui
          .horizontal(|ui| {
            self.show_shape_content(ui, layer_id, shape_idx, shape);
          })
          .response;

        content_response.context_menu(|ui| {
          let visibility_text = if geometry_visible { "Hide" } else { "Show" };

          if ui.button(format!("{visibility_text} Geometry")).clicked() {
            self
              .geometry_visibility
              .insert(geometry_key.clone(), !geometry_visible);
            ui.close();
          }

          ui.separator();

          if ui.button("🗑 Delete Geometry").clicked() {
            if let Some(shapes) = self.shape_map.get_mut(layer_id) {
              if shape_idx < shapes.len() {
                shapes.remove(shape_idx);
                self.geometry_visibility.remove(&geometry_key);
                let keys_to_update: Vec<_> = self
                  .geometry_visibility
                  .keys()
                  .filter(|(lid, idx)| lid == layer_id && *idx > shape_idx)
                  .cloned()
                  .collect();

                for (lid, idx) in keys_to_update {
                  if let Some(visible) = self.geometry_visibility.remove(&(lid.clone(), idx)) {
                    self.geometry_visibility.insert((lid, idx - 1), visible);
                  }
                }
              }
            }
            ui.close();
          }
        });
      });
    });
  }

  fn show_shape_content(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    shape: &Geometry<PixelCoordinate>,
  ) {
    match shape {
      Geometry::Point(coord, metadata) => {
        let wgs84 = coord.as_wgs84();
        self.show_colored_icon(ui, layer_id, shape_idx, "📍", metadata, false);

        if let Some(label) = &metadata.label {
          ui.strong(label);
          ui.small(format!("({:.4}, {:.4})", wgs84.lat, wgs84.lon));
        } else {
          ui.label(format!("{:.4}, {:.4}", wgs84.lat, wgs84.lon));
        }
      }

      Geometry::LineString(coords, metadata) => {
        self.show_colored_icon(ui, layer_id, shape_idx, "📏", metadata, false);

        if let Some(label) = &metadata.label {
          ui.strong(label);
        } else {
          ui.label("Line");
        }

        ui.small(format!("({} pts)", coords.len()));

        if let (Some(first), Some(last)) = (coords.first(), coords.last()) {
          let first_wgs84 = first.as_wgs84();
          let last_wgs84 = last.as_wgs84();
          ui.small(format!(
            "{:.2},{:.2}→{:.2},{:.2}",
            first_wgs84.lat, first_wgs84.lon, last_wgs84.lat, last_wgs84.lon
          ));
        }
      }

      Geometry::Polygon(coords, metadata) => {
        self.show_colored_icon(ui, layer_id, shape_idx, "⬟", metadata, true);

        if let Some(label) = &metadata.label {
          ui.strong(label);
        } else {
          ui.label("Polygon");
        }

        ui.small(format!("({} pts)", coords.len()));

        if !coords.is_empty() {
          let wgs84_coords: Vec<WGS84Coordinate> = coords
            .iter()
            .map(crate::map::coordinates::Coordinate::as_wgs84)
            .collect();
          let min_lat = wgs84_coords
            .iter()
            .map(|c| c.lat)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);
          let max_lat = wgs84_coords
            .iter()
            .map(|c| c.lat)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);
          let min_lon = wgs84_coords
            .iter()
            .map(|c| c.lon)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);
          let max_lon = wgs84_coords
            .iter()
            .map(|c| c.lon)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);

          ui.small(format!(
            "{min_lat:.1},{min_lon:.1}→{max_lat:.1},{max_lon:.1}"
          ));
        }
      }

      Geometry::GeometryCollection(geometries, metadata) => {
        ui.label(format!("📦 Collection ({} items)", geometries.len()));
        if let Some(label) = &metadata.label {
          ui.small(format!("- {label}"));
        }
      }
    }
  }

  fn show_colored_icon(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    icon: &str,
    metadata: &Metadata,
    is_polygon: bool,
  ) {
    if let Some(style) = &metadata.style {
      let mut stroke_color = style.color();
      let mut fill_color = style.fill_color();
      let colored_text = egui::RichText::new(icon).color(stroke_color);

      let hover_text = if is_polygon {
        "Click to change stroke & fill colors"
      } else {
        "Click to change color"
      };
      let icon_response = ui.button(colored_text).on_hover_text(hover_text);

      let popup_id = egui::Id::new(format!("color_picker_{layer_id}_{shape_idx}"));

      if icon_response.clicked() {
        #[allow(deprecated)]
        ui.memory_mut(|mem| mem.toggle_popup(popup_id));
      }

      #[allow(deprecated)]
      egui::popup_below_widget(
        ui,
        popup_id,
        &icon_response,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
          if is_polygon {
            ui.heading("Choose Colors");
            ui.separator();
            ui.label("Stroke Color:");
            if ui.color_edit_button_srgba(&mut stroke_color).changed() {
              self.update_shape_stroke_color(layer_id, shape_idx, stroke_color);
            }

            let mut stroke_hsva = egui::ecolor::Hsva::from(stroke_color);
            egui::widgets::color_picker::color_picker_hsva_2d(
              ui,
              &mut stroke_hsva,
              egui::widgets::color_picker::Alpha::Opaque,
            );
            let new_stroke_color = egui::Color32::from(stroke_hsva);
            if new_stroke_color != stroke_color {
              self.update_shape_stroke_color(layer_id, shape_idx, new_stroke_color);
            }

            ui.separator();
            ui.label("Fill Color:");
            if ui.color_edit_button_srgba(&mut fill_color).changed() {
              self.update_shape_fill_color(layer_id, shape_idx, fill_color);
            }

            let mut fill_hsva = egui::ecolor::Hsva::from(fill_color);
            egui::widgets::color_picker::color_picker_hsva_2d(
              ui,
              &mut fill_hsva,
              egui::widgets::color_picker::Alpha::BlendOrAdditive,
            );
            let new_fill_color = egui::Color32::from(fill_hsva);
            if new_fill_color != fill_color {
              self.update_shape_fill_color(layer_id, shape_idx, new_fill_color);
            }
          } else {
            ui.heading("Choose Color");
            ui.separator();
            if ui.color_edit_button_srgba(&mut stroke_color).changed() {
              self.update_shape_color(layer_id, shape_idx, stroke_color);
            }

            ui.separator();
            let mut hsva = egui::ecolor::Hsva::from(stroke_color);
            egui::widgets::color_picker::color_picker_hsva_2d(
              ui,
              &mut hsva,
              egui::widgets::color_picker::Alpha::Opaque,
            );
            let new_color = egui::Color32::from(hsva);
            if new_color != stroke_color {
              self.update_shape_color(layer_id, shape_idx, new_color);
            }
          }
        },
      );
    } else {
      ui.label(icon);
    }
  }

  fn update_shape_color(&mut self, layer_id: &str, shape_idx: usize, new_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id) {
      if let Some(shape) = shapes.get_mut(shape_idx) {
        let metadata = match shape {
          Geometry::Point(_, metadata)
          | Geometry::LineString(_, metadata)
          | Geometry::Polygon(_, metadata)
          | Geometry::GeometryCollection(_, metadata) => metadata,
        };

        let new_style = if let Some(existing_style) = &metadata.style {
          Style::default()
            .with_color(new_color)
            .with_fill_color(existing_style.fill_color())
            .with_visible(true)
        } else {
          Style::default().with_color(new_color)
        };
        metadata.style = Some(new_style);
      }
    }
  }

  fn update_shape_stroke_color(&mut self, layer_id: &str, shape_idx: usize, new_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id) {
      if let Some(shape) = shapes.get_mut(shape_idx) {
        let metadata = match shape {
          Geometry::Point(_, metadata)
          | Geometry::LineString(_, metadata)
          | Geometry::Polygon(_, metadata)
          | Geometry::GeometryCollection(_, metadata) => metadata,
        };

        let new_style = if let Some(existing_style) = &metadata.style {
          Style::default()
            .with_color(new_color)
            .with_fill_color(existing_style.fill_color())
            .with_visible(true)
        } else {
          Style::default().with_color(new_color)
        };
        metadata.style = Some(new_style);
      }
    }
  }

  fn update_shape_fill_color(&mut self, layer_id: &str, shape_idx: usize, new_fill_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id) {
      if let Some(shape) = shapes.get_mut(shape_idx) {
        let metadata = match shape {
          Geometry::Point(_, metadata)
          | Geometry::LineString(_, metadata)
          | Geometry::Polygon(_, metadata)
          | Geometry::GeometryCollection(_, metadata) => metadata,
        };

        let new_style = if let Some(existing_style) = &metadata.style {
          Style::default()
            .with_color(existing_style.color())
            .with_fill_color(new_fill_color)
            .with_visible(true)
        } else {
          Style::default()
            .with_color(Color32::BLUE)
            .with_fill_color(new_fill_color)
        };
        metadata.style = Some(new_style);
      }
    }
  }
}

const NAME: &str = "Shape Layer";

impl Layer for ShapeLayer {
  fn process_pending_events(&mut self) {
    // Process any pending layer data immediately
    self.handle_new_shapes();
  }

  fn discard_pending_events(&mut self) {
    for _event in self.recv.try_iter() {
    }
  }

  fn draw(&mut self, ui: &mut Ui, transform: &Transform, _rect: Rect) {
    self.handle_new_shapes();

    if !self.visible() {
      return;
    }

    for (layer_id, shapes) in &self.shape_map {
      if *self.layer_visibility.get(layer_id).unwrap_or(&true) {
        for (shape_idx, shape) in shapes.iter().enumerate() {
          let geometry_key = (layer_id.clone(), shape_idx);
          if *self.geometry_visibility.get(&geometry_key).unwrap_or(&true) {
            shape.draw(ui.painter(), transform);
          }
        }
      }
    }
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    let bb = self
      .shape_map
      .iter()
      .filter(|(layer_id, _)| *self.layer_visibility.get(*layer_id).unwrap_or(&true))
      .flat_map(|(layer_id, shapes)| {
        shapes.iter().enumerate().filter_map(|(shape_idx, shape)| {
          let geometry_key = (layer_id.clone(), shape_idx);
          if *self.geometry_visibility.get(&geometry_key).unwrap_or(&true) {
            Some(shape.bounding_box())
          } else {
            None
          }
        })
      })
      .fold(BoundingBox::default(), |acc, b| acc.extend(&b));

    bb.is_valid().then_some(bb)
  }

  fn clear(&mut self) {
    self.shape_map.clear();
    self.layer_visibility.clear();
    self.geometry_visibility.clear();
  }

  fn name(&self) -> &str {
    NAME
  }

  fn visible(&self) -> bool {
    self.layer_properties.visible
  }

  fn visible_mut(&mut self) -> &mut bool {
    &mut self.layer_properties.visible
  }

  fn ui(&mut self, ui: &mut Ui) {
    let has_highlighted_geometry = self.highlighted_geometry.is_some();

    let mut layer_header = egui::CollapsingHeader::new(self.name().to_owned());
    if has_highlighted_geometry {
      layer_header = layer_header.open(Some(true));
    }

    layer_header.show(ui, |ui| {
      ui.checkbox(self.visible_mut(), "visible");
      self.ui_content(ui);
    });
  }

  fn ui_content(&mut self, ui: &mut Ui) {
    let has_highlighted_geometry = self.highlighted_geometry.is_some();

    let mut shapes_header = egui::CollapsingHeader::new("Shapes");
    if has_highlighted_geometry {
      shapes_header = shapes_header.open(Some(true));
    }

    shapes_header.show(ui, |ui| {
      self.show_shape_layers(ui);
    });
  }

  fn has_highlighted_geometry(&self) -> bool {
    self.highlighted_geometry.is_some()
  }

  fn find_closest_geometry(&mut self, pos: Pos2, transform: &Transform) -> (f64, bool) {
    let click_coord = transform.invert().apply(pos.into());
    let tolerance_map_coords = f64::from(20.0 / transform.zoom);
    let mut closest_distance = f64::INFINITY;

    for (layer_id, shapes) in &self.shape_map {
      if !self.layer_visibility.get(layer_id).unwrap_or(&true) {
        continue;
      }

      for (shape_idx, shape) in shapes.iter().enumerate() {
        let geometry_key = (layer_id.clone(), shape_idx);
        if !self.geometry_visibility.get(&geometry_key).unwrap_or(&true) {
          continue;
        }

        let distance = self.calculate_distance_to_geometry(shape, click_coord);
        if distance < closest_distance && distance < tolerance_map_coords {
          closest_distance = distance;
        }
      }
    }

    (closest_distance, closest_distance < tolerance_map_coords)
  }

  fn handle_double_click(&mut self, pos: Pos2, transform: &Transform) -> bool {
    let click_coord = transform.invert().apply(pos.into());
    let tolerance_map_coords = f64::from(20.0 / transform.zoom);
    let mut closest_distance = f64::INFINITY;
    let mut found_geometry: Option<(String, usize)> = None;

    for (layer_id, shapes) in &self.shape_map {
      if !self.layer_visibility.get(layer_id).unwrap_or(&true) {
        continue;
      }

      for (shape_idx, shape) in shapes.iter().enumerate() {
        let geometry_key = (layer_id.clone(), shape_idx);
        if !self.geometry_visibility.get(&geometry_key).unwrap_or(&true) {
          continue;
        }

        let distance = self.calculate_distance_to_geometry(shape, click_coord);
        if distance < closest_distance && distance < tolerance_map_coords {
          closest_distance = distance;
          found_geometry = Some((layer_id.clone(), shape_idx));
        }
      }
    }

    if let Some((layer_id, shape_idx)) = found_geometry {
      self.highlighted_geometry = Some((layer_id, shape_idx));
      true
    } else {
      self.highlighted_geometry = None;
      false
    }
  }
}

impl ShapeLayer {
  #[allow(clippy::only_used_in_recursion)]
  fn calculate_distance_to_geometry(
    &self,
    geometry: &Geometry<PixelCoordinate>,
    click_coord: PixelCoordinate,
  ) -> f64 {
    match geometry {
      Geometry::Point(coord, _) => {
        let dx = coord.x - click_coord.x;
        let dy = coord.y - click_coord.y;
        f64::from(dx * dx + dy * dy).sqrt()
      }
      Geometry::LineString(coords, _) => coords
        .iter()
        .map(|coord| {
          let dx = coord.x - click_coord.x;
          let dy = coord.y - click_coord.y;
          f64::from(dx * dx + dy * dy).sqrt()
        })
        .fold(f64::INFINITY, f64::min),
      Geometry::Polygon(coords, _) => coords
        .iter()
        .map(|coord| {
          let dx = coord.x - click_coord.x;
          let dy = coord.y - click_coord.y;
          f64::from(dx * dx + dy * dy).sqrt()
        })
        .fold(f64::INFINITY, f64::min),
      Geometry::GeometryCollection(geometries, _) => geometries
        .iter()
        .map(|geom| self.calculate_distance_to_geometry(geom, click_coord))
        .fold(f64::INFINITY, f64::min),
    }
  }
}
