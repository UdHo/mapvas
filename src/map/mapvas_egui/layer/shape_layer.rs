use super::{Layer, LayerProperties, drawable::Drawable as _};
use crate::{
  config::Config,
  map::{
    coordinates::{BoundingBox, Coordinate, PixelCoordinate, Transform, WGS84Coordinate},
    distance,
    geometry_collection::{Geometry, Metadata, Style},
    map_event::{Layer as EventLayer, MapEvent},
  },
  profile_scope,
};
use egui::{Color32, Pos2, Rect, Ui};
use std::{
  collections::HashMap,
  sync::{
    Arc,
    mpsc::{Receiver, Sender},
  },
};

const MAX_ITEMS_PER_COLLECTION: usize = 100;
const ITEMS_PER_PAGE: usize = 50;

/// A layer that draws shapes on the map.
pub struct ShapeLayer {
  shape_map: HashMap<String, Vec<Geometry<PixelCoordinate>>>,
  layer_visibility: HashMap<String, bool>,
  geometry_visibility: HashMap<(String, usize), bool>,
  collection_expansion: HashMap<(String, usize, Vec<usize>), bool>,
  nested_geometry_visibility: HashMap<(String, usize, Vec<usize>), bool>,
  collection_pagination: HashMap<(String, usize, Vec<usize>), usize>,
  layer_pagination: HashMap<String, usize>,
  recv: Arc<Receiver<MapEvent>>,
  send: Sender<MapEvent>,
  layer_properties: LayerProperties,
  highlighted_geometry: Option<(String, usize)>,
  just_highlighted: bool,
  config: Config,
}

fn truncate_label_by_width(ui: &egui::Ui, label: &str, available_width: f32) -> (String, bool) {
  // Ensure minimum available width
  if available_width < 20.0 {
    return ("...".to_string(), true);
  }

  let chars: Vec<char> = label.chars().collect();

  // Fast fallback for very long strings to prevent hanging
  if chars.len() > 200 {
    let truncated: String = chars[..50].iter().collect();
    return (format!("{truncated}..."), true);
  }

  let font_id = ui.style().text_styles.get(&egui::TextStyle::Body).unwrap();
  let ellipsis = "...";

  // Measure using egui's text measurement utilities
  let galley =
    ui.fonts(|f| f.layout_no_wrap(label.to_string(), font_id.clone(), egui::Color32::BLACK));
  let full_width = galley.size().x;

  // Add some safety margin to prevent edge cases
  let safe_available_width = available_width - 5.0;

  if full_width <= safe_available_width {
    return (label.to_string(), false);
  }

  // Find the longest substring that fits with ellipsis
  let ellipsis_galley =
    ui.fonts(|f| f.layout_no_wrap(ellipsis.to_string(), font_id.clone(), egui::Color32::BLACK));
  let ellipsis_width = ellipsis_galley.size().x;

  // If even ellipsis doesn't fit, return just dots
  if ellipsis_width > safe_available_width {
    return ("...".to_string(), true);
  }

  let mut best_len = 0;

  // Use binary search for efficiency with long strings
  let mut left = 0;
  let mut right = chars.len().min(100); // Cap to prevent excessive measurements

  while left <= right {
    let mid = usize::midpoint(left, right);
    if mid == 0 {
      break;
    }

    let substring: String = chars[..mid].iter().collect();
    let substring_galley =
      ui.fonts(|f| f.layout_no_wrap(substring, font_id.clone(), egui::Color32::BLACK));
    let test_width = substring_galley.size().x + ellipsis_width;

    if test_width <= safe_available_width {
      best_len = mid;
      left = mid + 1;
    } else {
      right = mid - 1;
    }
  }

  if best_len == 0 {
    (ellipsis.to_string(), true)
  } else {
    let truncated: String = chars[..best_len].iter().collect();
    (format!("{truncated}{ellipsis}"), true)
  }
}

impl ShapeLayer {
  #[must_use]
  pub fn new(config: Config) -> Self {
    let (send, recv) = std::sync::mpsc::channel();

    Self {
      shape_map: HashMap::new(),
      layer_visibility: HashMap::new(),
      geometry_visibility: HashMap::new(),
      collection_expansion: HashMap::new(),
      nested_geometry_visibility: HashMap::new(),
      collection_pagination: HashMap::new(),
      layer_pagination: HashMap::new(),
      recv: recv.into(),
      send,
      layer_properties: LayerProperties::default(),
      highlighted_geometry: None,
      just_highlighted: false,
      config,
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

  #[allow(clippy::too_many_lines)]
  fn show_shape_layers(&mut self, ui: &mut egui::Ui) {
    // Update pagination to show highlighted geometry if needed
    self.update_pagination_for_highlight();

    let layer_ids: Vec<String> = self.shape_map.keys().cloned().collect();

    for layer_id in layer_ids {
      let shapes_count = self.shape_map.get(&layer_id).map_or(0, Vec::len);

      let has_highlighted_geometry = self
        .highlighted_geometry
        .as_ref()
        .is_some_and(|(highlighted_layer_id, _)| highlighted_layer_id == &layer_id);

      let header_id = egui::Id::new(format!("shape_layer_{layer_id}"));

      let font_id = ui.style().text_styles.get(&egui::TextStyle::Body).unwrap();
      let reserved_galley = ui.fonts(|f| {
        f.layout_no_wrap(
          "📁  (9999) ".to_string(),
          font_id.clone(),
          egui::Color32::BLACK,
        )
      });
      let reserved_width = reserved_galley.size().x + 60.0;
      let available_width = (ui.available_width() - reserved_width).max(30.0);
      let (truncated_layer_id, was_truncated) =
        truncate_label_by_width(ui, &layer_id, available_width);
      let mut header =
        egui::CollapsingHeader::new(format!("📁 {truncated_layer_id} ({shapes_count})"))
          .id_salt(header_id)
          .default_open(has_highlighted_geometry && shapes_count <= MAX_ITEMS_PER_COLLECTION);

      if was_truncated {
        header = header.show_background(true);
      }

      if self.just_highlighted && has_highlighted_geometry {
        header = header.open(Some(true));
      }

      let header_response = header.show(ui, |ui| {
        if let Some(shapes) = self.shape_map.get(&layer_id).cloned() {
          let total_shapes = shapes.len();

          if total_shapes > MAX_ITEMS_PER_COLLECTION {
            ui.label(format!(
              "⚠️ Large layer with {total_shapes} geometries - showing paginated view"
            ));
            ui.separator();

            let current_page = *self.layer_pagination.get(&layer_id).unwrap_or(&0);
            let total_pages = total_shapes.div_ceil(ITEMS_PER_PAGE);
            let start_idx = current_page * ITEMS_PER_PAGE;
            let end_idx = (start_idx + ITEMS_PER_PAGE).min(total_shapes);

            ui.horizontal(|ui| {
              ui.label(format!(
                "Page {} of {} (showing {}-{} of {})",
                current_page + 1,
                total_pages,
                start_idx + 1,
                end_idx,
                total_shapes
              ));
            });

            ui.horizontal(|ui| {
              if ui.button("◀ Previous").clicked() && current_page > 0 {
                self
                  .layer_pagination
                  .insert(layer_id.clone(), current_page - 1);
              }

              if ui.button("Next ▶").clicked() && current_page < total_pages - 1 {
                self
                  .layer_pagination
                  .insert(layer_id.clone(), current_page + 1);
              }

              if ui.button("🎯 Show All on Map").clicked() {
                for shape_idx in start_idx..end_idx {
                  self
                    .geometry_visibility
                    .insert((layer_id.clone(), shape_idx), true);
                }
              }
            });

            ui.separator();

            for (idx, shape) in shapes
              .iter()
              .enumerate()
              .skip(start_idx)
              .take(ITEMS_PER_PAGE)
            {
              self.show_shape_ui(ui, &layer_id, idx, shape);
              if idx < end_idx - 1 {
                ui.separator();
              }
            }
          } else {
            for (shape_idx, shape) in shapes.iter().enumerate() {
              self.show_shape_ui(ui, &layer_id, shape_idx, shape);
              if shape_idx < shapes.len() - 1 {
                ui.separator();
              }
            }
          }
        }
      });

      let header_resp = header_response.header_response;
      if was_truncated && header_resp.clicked() {
        ui.memory_mut(|mem| {
          mem.data.insert_temp(
            egui::Id::new(format!("layer_popup_{layer_id}")),
            layer_id.clone(),
          );
        });
      }

      header_resp.context_menu(|ui| {
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

      let popup_id = egui::Id::new(format!("layer_popup_{layer_id}"));
      if let Some(full_text) = ui.memory(|mem| mem.data.get_temp::<String>(popup_id)) {
        let mut is_open = true;
        egui::Window::new("Full Layer Name")
          .id(popup_id)
          .open(&mut is_open)
          .collapsible(false)
          .resizable(true)
          .movable(true)
          .default_width(500.0)
          .min_width(400.0)
          .max_width(800.0)
          .max_height(400.0)
          .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical()
              .max_height(300.0)
              .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                  ui.add(egui::Label::new(&full_text).wrap());
                });
              });
          });

        if !is_open {
          ui.memory_mut(|mem| mem.data.remove::<String>(popup_id));
        }
      }
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
      Some(egui::Color32::from_rgb(60, 80, 110))
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
      // Handle collections differently - they get their own CollapsingHeader without eye icon
      if let Geometry::GeometryCollection(geometries, metadata) = shape {
        self.show_geometry_collection_inline(ui, layer_id, shape_idx, geometries, metadata);
      } else {
        // Non-collections get the traditional eye icon + content layout
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
      }
    });
  }

  #[allow(clippy::too_many_lines)]
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
          let available_width = (ui.available_width() - 100.0).max(30.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, label, available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("point_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.clone()));
          }
          let coord_text = format!("({:.3}, {:.3})", wgs84.lat, wgs84.lon);
          let available_width = (ui.available_width() - 20.0).max(30.0);
          let (truncated_coord, _) = truncate_label_by_width(ui, &coord_text, available_width);
          ui.small(truncated_coord);
        } else {
          let coord_text = format!("{:.3}, {:.3}", wgs84.lat, wgs84.lon);
          let available_width = (ui.available_width() - 20.0).max(30.0);
          let (truncated_coord, _) = truncate_label_by_width(ui, &coord_text, available_width);
          ui.label(truncated_coord);
        }
      }

      Geometry::LineString(coords, metadata) => {
        self.show_colored_icon(ui, layer_id, shape_idx, "📏", metadata, false);

        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 100.0).max(30.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, label, available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("line_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.clone()));
          }
        } else {
          let response = ui.strong("Line");
          if response.clicked() {
            let popup_id = egui::Id::new(format!("line_popup_{layer_id}_{shape_idx}"));
            let line_info = format!(
              "📏 LineString\nPoints: {}\nStart: {:.4}, {:.4}\nEnd: {:.4}, {:.4}",
              coords.len(),
              coords.first().map_or(0.0, |c| c.as_wgs84().lat),
              coords.first().map_or(0.0, |c| c.as_wgs84().lon),
              coords.last().map_or(0.0, |c| c.as_wgs84().lat),
              coords.last().map_or(0.0, |c| c.as_wgs84().lon)
            );
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, line_info));
          }
        }

        ui.small(format!("({} pts)", coords.len()));

        if let (Some(first), Some(last)) = (coords.first(), coords.last()) {
          let first_wgs84 = first.as_wgs84();
          let last_wgs84 = last.as_wgs84();
          let coord_text = format!(
            "{:.2},{:.2}→{:.2},{:.2}",
            first_wgs84.lat, first_wgs84.lon, last_wgs84.lat, last_wgs84.lon
          );
          let available_width = (ui.available_width() - 20.0).max(30.0);
          let (truncated_coord, _) = truncate_label_by_width(ui, &coord_text, available_width);
          let response = ui.small(truncated_coord);
          if response.clicked() {
            let popup_id = egui::Id::new(format!("line_coords_popup_{layer_id}_{shape_idx}"));
            let all_coords = coords
              .iter()
              .enumerate()
              .map(|(i, coord)| {
                let wgs84 = coord.as_wgs84();
                format!("{:2}: {:.6}, {:.6}", i + 1, wgs84.lat, wgs84.lon)
              })
              .collect::<Vec<_>>()
              .join("\n");
            let coords_info = format!(
              "📏 LineString Coordinates\nTotal Points: {}\n\nAll Coordinates:\n{}",
              coords.len(),
              all_coords
            );
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, coords_info));
          }
        }
      }

      Geometry::Polygon(coords, metadata) => {
        self.show_colored_icon(ui, layer_id, shape_idx, "⬟", metadata, true);

        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 100.0).max(30.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, label, available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("polygon_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.clone()));
          }
        } else {
          ui.label("Polygon");
        }

        ui.small(format!("({} pts)", coords.len()));

        if !coords.is_empty() {
          let wgs84_coords: Vec<WGS84Coordinate> =
            coords.iter().map(Coordinate::as_wgs84).collect();
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

          let bounds_text = format!("{min_lat:.1},{min_lon:.1}→{max_lat:.1},{max_lon:.1}");
          let available_width = (ui.available_width() - 20.0).max(30.0);
          let (truncated_bounds, _) = truncate_label_by_width(ui, &bounds_text, available_width);
          ui.small(truncated_bounds);
        }
      }

      Geometry::GeometryCollection(geometries, metadata) => {
        // Collections should use CollapsingHeader, not the eye icon UI
        // This handles the case where a top-level geometry is a collection
        self.show_geometry_collection_inline(ui, layer_id, shape_idx, geometries, metadata);
      }
    }

    let geometry_popup_ids = [
      format!("point_popup_{layer_id}_{shape_idx}"),
      format!("line_popup_{layer_id}_{shape_idx}"),
      format!("line_coords_popup_{layer_id}_{shape_idx}"),
      format!("polygon_popup_{layer_id}_{shape_idx}"),
      format!("collection_popup_{layer_id}_{shape_idx}"),
    ];

    for popup_id_str in geometry_popup_ids {
      let popup_id = egui::Id::new(&popup_id_str);
      if let Some(full_text) = ui.memory(|mem| mem.data.get_temp::<String>(popup_id)) {
        let mut is_open = true;
        egui::Window::new("Full Label")
          .id(popup_id)
          .open(&mut is_open)
          .collapsible(false)
          .resizable(true)
          .movable(true)
          .default_width(500.0)
          .min_width(400.0)
          .max_width(800.0)
          .max_height(400.0)
          .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical()
              .max_height(300.0)
              .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                  ui.add(egui::Label::new(&full_text).wrap());
                });
              });
          });

        if !is_open {
          ui.memory_mut(|mem| mem.data.remove::<String>(popup_id));
        }
      }
    }

    let mut color_picker_requests = Vec::new();
    for (layer_id, shapes) in &self.shape_map {
      for (shape_idx, shape) in shapes.iter().enumerate() {
        let popup_id = egui::Id::new(format!("color_picker_{layer_id}_{shape_idx}"));
        if ui
          .memory(|mem| mem.data.get_temp::<bool>(popup_id))
          .unwrap_or(false)
        {
          let window_title = match shape {
            Geometry::Polygon(_, _) => "Choose Colors",
            _ => "Choose Color",
          };
          color_picker_requests.push((layer_id.clone(), shape_idx, window_title, popup_id));
        }
      }
    }

    for (layer_id, shape_idx, window_title, popup_id) in color_picker_requests {
      let mut is_open = true;
      egui::Window::new(window_title)
        .id(popup_id)
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .movable(true)
        .default_width(250.0)
        .show(ui.ctx(), |ui| {
          if let Some(shapes) = self.shape_map.get_mut(&layer_id) {
            if let Some(shape) = shapes.get_mut(shape_idx) {
              let metadata = match shape {
                Geometry::Point(_, metadata)
                | Geometry::LineString(_, metadata)
                | Geometry::Polygon(_, metadata)
                | Geometry::GeometryCollection(_, metadata) => metadata,
              };

              if metadata.style.is_none() {
                metadata.style = Some(crate::map::geometry_collection::Style::default());
              }

              if let Some(style) = &metadata.style {
                let mut stroke_color = style.color();
                let mut fill_color = style.fill_color();
                let is_polygon = matches!(shape, Geometry::Polygon(_, _));

                if is_polygon {
                  ui.label("Stroke Color:");
                  if ui.color_edit_button_srgba(&mut stroke_color).changed() {
                    self.update_shape_stroke_color(&layer_id, shape_idx, stroke_color);
                  }

                  let mut stroke_hsva = egui::ecolor::Hsva::from(stroke_color);
                  egui::widgets::color_picker::color_picker_hsva_2d(
                    ui,
                    &mut stroke_hsva,
                    egui::widgets::color_picker::Alpha::Opaque,
                  );
                  let new_stroke_color = egui::Color32::from(stroke_hsva);
                  if new_stroke_color != stroke_color {
                    self.update_shape_stroke_color(&layer_id, shape_idx, new_stroke_color);
                  }

                  ui.separator();
                  ui.label("Fill Color:");
                  if ui.color_edit_button_srgba(&mut fill_color).changed() {
                    self.update_shape_fill_color(&layer_id, shape_idx, fill_color);
                  }

                  let mut fill_hsva = egui::ecolor::Hsva::from(fill_color);
                  egui::widgets::color_picker::color_picker_hsva_2d(
                    ui,
                    &mut fill_hsva,
                    egui::widgets::color_picker::Alpha::BlendOrAdditive,
                  );
                  let new_fill_color = egui::Color32::from(fill_hsva);
                  if new_fill_color != fill_color {
                    self.update_shape_fill_color(&layer_id, shape_idx, new_fill_color);
                  }
                } else {
                  if ui.color_edit_button_srgba(&mut stroke_color).changed() {
                    self.update_shape_color(&layer_id, shape_idx, stroke_color);
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
                    self.update_shape_color(&layer_id, shape_idx, new_color);
                  }
                }
              }
            }
          }
        });

      if !is_open {
        ui.memory_mut(|mem| mem.data.remove::<bool>(popup_id));
      }
    }
  }

  fn show_geometry_collection_inline(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    geometries: &[Geometry<PixelCoordinate>],
    metadata: &Metadata,
  ) {
    let collection_key = (layer_id.to_string(), shape_idx, vec![]);
    let is_expanded = self
      .collection_expansion
      .get(&collection_key)
      .unwrap_or(&false);

    let collection_label = if let Some(label) = &metadata.label {
      format!("📁 {} ({} items)", label, geometries.len())
    } else {
      format!("📁 Collection ({} items)", geometries.len())
    };

    let header_id = egui::Id::new(format!("collection_{layer_id}_{shape_idx}"));
    let header_response = egui::CollapsingHeader::new(collection_label)
      .id_salt(header_id)
      .default_open(*is_expanded)
      .show(ui, |ui| {
        for (nested_idx, nested_geometry) in geometries.iter().enumerate() {
          let nested_path = vec![nested_idx];
          self.show_nested_geometry_content(ui, layer_id, shape_idx, &nested_path, nested_geometry);
          if nested_idx < geometries.len() - 1 {
            ui.separator();
          }
        }
      });

    // Update expansion state based on the body response (if body was shown, header was open)
    let is_currently_open = header_response.body_response.is_some();
    self
      .collection_expansion
      .insert(collection_key, is_currently_open);

    // Add context menu for collection
    header_response.header_response.context_menu(|ui| {
      // Collection visibility control
      let geometry_key = (layer_id.to_string(), shape_idx);
      let geometry_visible = *self.geometry_visibility.get(&geometry_key).unwrap_or(&true);
      let visibility_text = if geometry_visible {
        "Hide Collection"
      } else {
        "Show Collection"
      };

      if ui.button(visibility_text).clicked() {
        self
          .geometry_visibility
          .insert(geometry_key.clone(), !geometry_visible);
        ui.close();
      }

      ui.separator();

      if ui.button("📋 Collection Info").clicked() {
        let popup_id = egui::Id::new(format!("collection_popup_{layer_id}_{shape_idx}"));
        let collection_info = format!(
          "📁 Geometry Collection\nItems: {}\nNested geometries: {}",
          geometries.len(),
          geometries
            .iter()
            .map(|g| match g {
              Geometry::Point(_, _) => "Point".to_string(),
              Geometry::LineString(_, _) => "LineString".to_string(),
              Geometry::Polygon(_, _) => "Polygon".to_string(),
              Geometry::GeometryCollection(nested, _) => format!("Collection ({})", nested.len()),
            })
            .collect::<Vec<_>>()
            .join(", ")
        );
        ui.memory_mut(|mem| mem.data.insert_temp(popup_id, collection_info));
        ui.close();
      }

      ui.separator();

      if ui.button("🗑 Delete Collection").clicked() {
        if let Some(shapes) = self.shape_map.get_mut(layer_id) {
          if shape_idx < shapes.len() {
            shapes.remove(shape_idx);
            self.geometry_visibility.remove(&geometry_key);
            // Clean up any nested visibility state for this collection
            self
              .nested_geometry_visibility
              .retain(|(lid, idx, _), _| !(lid == layer_id && *idx == shape_idx));
            self
              .collection_expansion
              .retain(|(lid, idx, _), _| !(lid == layer_id && *idx == shape_idx));

            // Update indices for remaining geometries
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
  }

  #[allow(clippy::too_many_lines)]
  fn show_nested_geometry_content(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
    geometry: &Geometry<PixelCoordinate>,
  ) {
    let nested_key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
    let nested_visible = *self
      .nested_geometry_visibility
      .get(&nested_key)
      .unwrap_or(&true);

    if let Geometry::GeometryCollection(nested_geometries, nested_metadata) = geometry {
      // Collections get CollapsingHeader without extra spacing - egui handles the indentation
      let collection_key = nested_key.clone();
      let is_expanded = *self
        .collection_expansion
        .get(&collection_key)
        .unwrap_or(&false);

      let collection_label = if let Some(label) = &nested_metadata.label {
        format!("📁 {} ({} items)", label, nested_geometries.len())
      } else {
        format!("📁 Collection ({} items)", nested_geometries.len())
      };

      let header_id = egui::Id::new(format!(
        "nested_collection_{layer_id}_{shape_idx}_{nested_path:?}"
      ));
      let header_response = egui::CollapsingHeader::new(collection_label)
        .id_salt(header_id)
        .default_open(is_expanded && nested_geometries.len() <= MAX_ITEMS_PER_COLLECTION)
        .show(ui, |ui| {
          let total_items = nested_geometries.len();

          if total_items > MAX_ITEMS_PER_COLLECTION {
            ui.label(format!(
              "⚠️ Large collection with {total_items} items - showing paginated view"
            ));
            ui.separator();

            let current_page = *self
              .collection_pagination
              .get(&collection_key)
              .unwrap_or(&0);
            let total_pages = total_items.div_ceil(ITEMS_PER_PAGE);
            let start_idx = current_page * ITEMS_PER_PAGE;
            let end_idx = (start_idx + ITEMS_PER_PAGE).min(total_items);

            ui.horizontal(|ui| {
              ui.label(format!(
                "Page {} of {} (showing {}-{} of {})",
                current_page + 1,
                total_pages,
                start_idx + 1,
                end_idx,
                total_items
              ));
            });

            ui.horizontal(|ui| {
              if ui.button("◀ Previous").clicked() && current_page > 0 {
                self
                  .collection_pagination
                  .insert(collection_key.clone(), current_page - 1);
              }

              if ui.button("Next ▶").clicked() && current_page < total_pages - 1 {
                self
                  .collection_pagination
                  .insert(collection_key.clone(), current_page + 1);
              }
            });

            ui.separator();

            for (idx, sub_geometry) in nested_geometries
              .iter()
              .enumerate()
              .skip(start_idx)
              .take(ITEMS_PER_PAGE)
            {
              let mut sub_path = nested_path.to_vec();
              sub_path.push(idx);
              self.show_nested_geometry_content(ui, layer_id, shape_idx, &sub_path, sub_geometry);
              if idx < end_idx - 1 {
                ui.separator();
              }
            }
          } else {
            for (sub_idx, sub_geometry) in nested_geometries.iter().enumerate() {
              let mut sub_path = nested_path.to_vec();
              sub_path.push(sub_idx);
              self.show_nested_geometry_content(ui, layer_id, shape_idx, &sub_path, sub_geometry);
              if sub_idx < nested_geometries.len() - 1 {
                ui.separator();
              }
            }
          }
        });

      // Update expansion state
      let is_currently_open = header_response.body_response.is_some();
      self
        .collection_expansion
        .insert(collection_key, is_currently_open);

      // Add context menu for nested collection
      header_response.header_response.context_menu(|ui| {
        let visibility_text = if nested_visible {
          "Hide Collection"
        } else {
          "Show Collection"
        };
        if ui.button(visibility_text).clicked() {
          self
            .nested_geometry_visibility
            .insert(nested_key, !nested_visible);
          ui.close();
        }
      });
    } else {
      // Individual geometries get eye icon + content with indentation
      let mut toggle_visibility = false;

      let horizontal_response = ui.horizontal(|ui| {
        // Add minimal indentation based on nesting level (only for individual geometries)
        let indent_level = nested_path.len();
        #[allow(clippy::cast_precision_loss)]
        ui.add_space(4.0 * (indent_level as f32));

        // Visibility toggle button for individual geometries
        let visibility_icon = if nested_visible { "👁" } else { "🚫" };
        if ui
          .add_sized([24.0, 20.0], egui::Button::new(visibility_icon))
          .clicked()
        {
          toggle_visibility = true;
        }

        // Show individual geometry content
        match geometry {
          Geometry::Point(coord, nested_metadata) => {
            let wgs84 = coord.as_wgs84();
            ui.label("📍");
            if let Some(label) = &nested_metadata.label {
              ui.strong(label);
            } else {
              ui.label("Point");
            }
            ui.small(format!("({:.3}, {:.3})", wgs84.lat, wgs84.lon));
          }
          Geometry::LineString(coords, nested_metadata) => {
            ui.label("📏");
            if let Some(label) = &nested_metadata.label {
              ui.strong(label);
            } else {
              ui.label("Line");
            }
            ui.small(format!("({} pts)", coords.len()));
          }
          Geometry::Polygon(coords, nested_metadata) => {
            ui.label("⬟");
            if let Some(label) = &nested_metadata.label {
              ui.strong(label);
            } else {
              ui.label("Polygon");
            }
            ui.small(format!("({} pts)", coords.len()));
          }
          Geometry::GeometryCollection(..) => {
            // This should not happen in individual geometry context
          }
        }
      });

      // Handle visibility toggle after the horizontal closure
      if toggle_visibility {
        self
          .nested_geometry_visibility
          .insert(nested_key.clone(), !nested_visible);
      }

      // Add context menu to individual geometries
      horizontal_response.response.context_menu(|ui| {
        let visibility_text = if nested_visible { "Hide" } else { "Show" };
        if ui.button(format!("{visibility_text} Geometry")).clicked() {
          self
            .nested_geometry_visibility
            .insert(nested_key, !nested_visible);
          ui.close();
        }
      });
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
    let stroke_color = if let Some(style) = &metadata.style {
      style.color()
    } else {
      egui::Color32::BLUE
    };

    let colored_text = egui::RichText::new(icon).color(stroke_color);

    let hover_text = if is_polygon {
      "Click to change stroke & fill colors"
    } else {
      "Click to change color"
    };
    let icon_response = ui.button(colored_text).on_hover_text(hover_text);

    let popup_id = egui::Id::new(format!("color_picker_{layer_id}_{shape_idx}"));

    if icon_response.clicked() {
      if metadata.style.is_none() {
        if let Some(shapes) = self.shape_map.get_mut(layer_id) {
          if let Some(shape) = shapes.get_mut(shape_idx) {
            let shape_metadata = match shape {
              Geometry::Point(_, metadata)
              | Geometry::LineString(_, metadata)
              | Geometry::Polygon(_, metadata)
              | Geometry::GeometryCollection(_, metadata) => metadata,
            };
            shape_metadata.style = Some(crate::map::geometry_collection::Style::default());
          }
        }
      }
      ui.memory_mut(|mem| mem.data.insert_temp(popup_id, true));
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

  fn draw_geometry_with_visibility(
    &self,
    painter: &egui::Painter,
    transform: &Transform,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
    geometry: &Geometry<PixelCoordinate>,
  ) {
    let nested_key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
    let is_visible = self
      .nested_geometry_visibility
      .get(&nested_key)
      .unwrap_or(&true);

    if !is_visible {
      return;
    }

    match geometry {
      Geometry::GeometryCollection(geometries, _) => {
        for (nested_idx, nested_geometry) in geometries.iter().enumerate() {
          let mut new_path = nested_path.to_vec();
          new_path.push(nested_idx);
          self.draw_geometry_with_visibility(
            painter,
            transform,
            layer_id,
            shape_idx,
            &new_path,
            nested_geometry,
          );
        }
      }
      _ => {
        geometry.draw_with_style(painter, transform, self.config.heading_style);
      }
    }
  }

  /// Calculate which page contains the given index
  fn calculate_page_for_index(index: usize) -> usize {
    index / ITEMS_PER_PAGE
  }

  /// Update pagination to show the highlighted geometry if just highlighted
  fn update_pagination_for_highlight(&mut self) {
    if self.just_highlighted {
      if let Some((layer_id, shape_idx)) = &self.highlighted_geometry {
        // Update layer pagination to show the highlighted geometry
        let target_page = Self::calculate_page_for_index(*shape_idx);
        self.layer_pagination.insert(layer_id.clone(), target_page);
      }
    }
  }
}

const NAME: &str = "Shape Layer";

impl Layer for ShapeLayer {
  fn process_pending_events(&mut self) {
    self.handle_new_shapes();
  }

  fn discard_pending_events(&mut self) {
    for _event in self.recv.try_iter() {}
  }

  fn draw(&mut self, ui: &mut Ui, transform: &Transform, _rect: Rect) {
    profile_scope!("ShapeLayer::draw");
    self.handle_new_shapes();

    if !self.visible() {
      return;
    }

    for (layer_id, shapes) in &self.shape_map {
      if *self.layer_visibility.get(layer_id).unwrap_or(&true) {
        profile_scope!("ShapeLayer::draw_layer", layer_id);
        for (shape_idx, shape) in shapes.iter().enumerate() {
          let geometry_key = (layer_id.clone(), shape_idx);
          if *self.geometry_visibility.get(&geometry_key).unwrap_or(&true) {
            self.draw_geometry_with_visibility(
              ui.painter(),
              transform,
              layer_id,
              shape_idx,
              &[],
              shape,
            );
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
    self.collection_expansion.clear();
    self.nested_geometry_visibility.clear();
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
    let layer_id = egui::Id::new("shape_layer_header");

    let mut layer_header = egui::CollapsingHeader::new(self.name().to_owned())
      .id_salt(layer_id)
      .default_open(has_highlighted_geometry);

    if self.just_highlighted {
      layer_header = layer_header.open(Some(true));
    }

    layer_header.show(ui, |ui| {
      ui.checkbox(self.visible_mut(), "visible");
      self.ui_content(ui);
    });
  }

  fn ui_content(&mut self, ui: &mut Ui) {
    profile_scope!("ShapeLayer::ui_content");
    let has_highlighted_geometry = self.highlighted_geometry.is_some();
    let shapes_header_id = egui::Id::new("shapes_header");

    let mut shapes_header = egui::CollapsingHeader::new("Shapes")
      .id_salt(shapes_header_id)
      .default_open(has_highlighted_geometry);

    if self.just_highlighted && has_highlighted_geometry {
      shapes_header = shapes_header.open(Some(true));
    }

    shapes_header.show(ui, |ui| {
      self.show_shape_layers(ui);
    });

    self.just_highlighted = false;
  }

  fn has_highlighted_geometry(&self) -> bool {
    self.highlighted_geometry.is_some()
  }

  fn closest_geometry_with_selection(&mut self, pos: Pos2, transform: &Transform) -> Option<f64> {
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

        if let Some(distance) = distance::distance_to_geometry(shape, click_coord) {
          if distance < closest_distance && distance < tolerance_map_coords {
            closest_distance = distance;
            found_geometry = Some((layer_id.clone(), shape_idx));
          }
        }
      }
    }

    if let Some((layer_id, shape_idx)) = found_geometry {
      let was_different = self.highlighted_geometry != Some((layer_id.clone(), shape_idx));
      self.highlighted_geometry = Some((layer_id, shape_idx));
      self.just_highlighted = was_different;
      return Some(closest_distance);
    }

    self.highlighted_geometry = None;
    self.just_highlighted = false;
    None
  }

  fn update_config(&mut self, config: &crate::config::Config) {
    self.config = config.clone();
  }
}
