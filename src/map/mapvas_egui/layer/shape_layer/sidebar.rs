use super::{HighlightCacheKey, HighlightTextureCache, SCROLL_AREA_MAX_HEIGHT, ShapeLayer};
use crate::map::{
  coordinates::{Coordinate, PixelCoordinate, Transform, WGS84Coordinate},
  geometry_collection::{Geometry, Metadata, Style},
};
use egui::{Color32, Rect};

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
  let galley = ui
    .ctx()
    .fonts_mut(|f| f.layout_no_wrap(label.to_string(), font_id.clone(), egui::Color32::BLACK));
  let full_width = galley.size().x;

  // Add some safety margin to prevent edge cases
  let safe_available_width = available_width - 5.0;

  if full_width <= safe_available_width {
    return (label.to_string(), false);
  }

  // Find the longest substring that fits with ellipsis
  let ellipsis_galley = ui
    .ctx()
    .fonts_mut(|f| f.layout_no_wrap(ellipsis.to_string(), font_id.clone(), egui::Color32::BLACK));
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
    let substring_galley = ui
      .ctx()
      .fonts_mut(|f| f.layout_no_wrap(substring, font_id.clone(), egui::Color32::BLACK));
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
  #[allow(clippy::too_many_lines)]
  pub(super) fn show_shape_layers(&mut self, ui: &mut egui::Ui) {
    let layer_ids: Vec<String> = self.shape_map.keys().cloned().collect();

    for layer_id in layer_ids {
      let shapes_count = self.shape_map.get(&layer_id).map_or(0, Vec::len);

      // Check if any geometry in this layer is highlighted
      let has_highlighted_geometry = self.geometry_highlighter.has_highlighted_geometry();

      let header_id = egui::Id::new(format!("shape_layer_{layer_id}"));

      let font_id = ui.style().text_styles.get(&egui::TextStyle::Body).unwrap();
      let reserved_galley = ui.ctx().fonts_mut(|f| {
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
          .default_open(has_highlighted_geometry);

      if was_truncated {
        header = header.show_background(true);
      }

      // Open sidebar on double-click (but not hover)
      if let Some((clicked_layer, _, _)) = &self.just_double_clicked
        && clicked_layer == &layer_id
      {
        header = header.open(Some(true));
      }

      let header_response = header.show(ui, |ui| {
        let shapes_count = self.shape_map.get(&layer_id).map_or(0, Vec::len);
        let row_height = ui.spacing().interact_size.y;
        let scroll_id = egui::Id::new(format!("layer_scroll_{layer_id}"));

        let mut scroll_area = egui::ScrollArea::vertical()
          .id_salt(scroll_id)
          .max_height(SCROLL_AREA_MAX_HEIGHT);

        // Jump directly to the double-clicked row. Row index = shape index (no filter).
        if let Some((clicked_layer, clicked_idx, _)) = &self.just_double_clicked
          && clicked_layer == &layer_id
        {
          #[allow(clippy::cast_precision_loss)]
          let offset = (*clicked_idx as f32 * (row_height + ui.spacing().item_spacing.y)
            - SCROLL_AREA_MAX_HEIGHT / 2.0)
            .max(0.0);
          scroll_area = scroll_area.vertical_scroll_offset(offset);
        }

        scroll_area.show_rows(ui, row_height, shapes_count, |ui, row_range| {
          for idx in row_range {
            if let Some(shape) = self.shape_map.get(&layer_id).and_then(|s| s.get(idx)) {
              let shape = shape.clone();
              self.show_shape_ui(ui, &layer_id, idx, &shape);
            }
          }
        });
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

        self.show_visibility_button(ui, layer_visible, "Layer", |this| {
          this
            .layer_visibility
            .insert(layer_id.clone(), !layer_visible);
          this.invalidate_cache();
        });

        ui.separator();

        if ui.button("🗑 Delete Layer").clicked() {
          self.shape_map.remove(&layer_id);
          self.layer_visibility.remove(&layer_id);
          self
            .geometry_visibility
            .retain(|(lid, _), _| lid != &layer_id);
          self.invalidate_cache();
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

    // Handle nested collection label popups once per frame (not per row).
    for (layer_id, shapes) in &self.shape_map {
      for (shape_idx, shape) in shapes.iter().enumerate() {
        if let Geometry::GeometryCollection(geometries, _) = shape {
          Self::check_nested_popups_recursive(ui, layer_id, shape_idx, geometries, &mut Vec::new());
        }
      }
    }

    // Show color picker windows once per frame (not per row).
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
          if let Some(shapes) = self.shape_map.get_mut(&layer_id)
            && let Some(shape) = shapes.get_mut(shape_idx)
          {
            let metadata = match shape {
              Geometry::Point(_, metadata)
              | Geometry::LineString(_, metadata)
              | Geometry::Polygon(_, metadata)
              | Geometry::GeometryCollection(_, metadata)
              | Geometry::Heatmap(_, metadata) => metadata,
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
        });

      if !is_open {
        ui.memory_mut(|mem| mem.data.remove::<bool>(popup_id));
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
    let geometry_key_for_highlight = (layer_id.to_string(), shape_idx, Vec::new());
    let is_highlighted = self.geometry_highlighter.is_highlighted(
      &geometry_key_for_highlight.0,
      geometry_key_for_highlight.1,
      &geometry_key_for_highlight.2,
    );

    let bg_color = if is_highlighted {
      Some(egui::Color32::from_rgb(100, 100, 200))
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
          let eye_response = ui.add_sized([24.0, 20.0], egui::Button::new(visibility_icon));
          if eye_response.double_clicked() {
            if let Some(shapes) = self.shape_map.get(layer_id) {
              // Check if this element is already solo (only visible one)
              let is_solo = geometry_visible
                && (0..shapes.len()).all(|i| {
                  i == shape_idx
                    || !*self
                      .geometry_visibility
                      .get(&(layer_id.to_string(), i))
                      .unwrap_or(&true)
                });
              for i in 0..shapes.len() {
                self.geometry_visibility.insert(
                  (layer_id.to_string(), i),
                  if is_solo { true } else { i == shape_idx },
                );
              }
              self.invalidate_cache();
            }
          } else if eye_response.clicked() {
            self
              .geometry_visibility
              .insert(geometry_key.clone(), !geometry_visible);
            self.invalidate_cache();
          }

          let content_response = ui
            .horizontal(|ui| {
              self.show_shape_content(ui, layer_id, shape_idx, shape);
            })
            .response;

          // Handle double-click to show popup (TODO: implement popup)
          if content_response.double_clicked() {
            println!("TODO: Show detail popup for sidebar geometry");
          }

          content_response.context_menu(|ui| {
            self.show_visibility_button(ui, geometry_visible, "Geometry", |this| {
              this
                .geometry_visibility
                .insert(geometry_key.clone(), !geometry_visible);
            });

            ui.separator();

            self.show_delete_geometry_button(ui, layer_id, shape_idx, &geometry_key);
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
          let available_width = (ui.available_width() - 40.0).max(100.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, &label.short(), available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("point_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.full()));
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
          let available_width = (ui.available_width() - 40.0).max(100.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, &label.short(), available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("line_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.full()));
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
          let available_width = (ui.available_width() - 40.0).max(100.0);
          let (truncated_label, was_truncated) =
            truncate_label_by_width(ui, &label.short(), available_width);
          let response = ui.strong(truncated_label);
          if was_truncated && response.clicked() {
            let popup_id = egui::Id::new(format!("polygon_popup_{layer_id}_{shape_idx}"));
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, label.full()));
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
            .min_by(f32::total_cmp)
            .unwrap_or(0.0);
          let max_lat = wgs84_coords
            .iter()
            .map(|c| c.lat)
            .max_by(f32::total_cmp)
            .unwrap_or(0.0);
          let min_lon = wgs84_coords
            .iter()
            .map(|c| c.lon)
            .min_by(f32::total_cmp)
            .unwrap_or(0.0);
          let max_lon = wgs84_coords
            .iter()
            .map(|c| c.lon)
            .max_by(f32::total_cmp)
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

      Geometry::Heatmap(coords, metadata) => {
        self.show_colored_icon(ui, layer_id, shape_idx, "🔥", metadata, false);

        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 40.0).max(100.0);
          let (truncated_label, _was_truncated) =
            truncate_label_by_width(ui, &label.short(), available_width);
          ui.strong(truncated_label);
        } else {
          ui.label("Heatmap");
        }

        let pts_text = format!("{} pts", coords.len());
        let available_width = (ui.available_width() - 20.0).max(30.0);
        let (truncated, _) = truncate_label_by_width(ui, &pts_text, available_width);
        ui.small(truncated);
      }
    }

    let geometry_popup_ids = [
      format!("point_popup_{layer_id}_{shape_idx}"),
      format!("line_popup_{layer_id}_{shape_idx}"),
      format!("line_coords_popup_{layer_id}_{shape_idx}"),
      format!("polygon_popup_{layer_id}_{shape_idx}"),
      format!("collection_popup_{layer_id}_{shape_idx}"),
      format!("collection_label_popup_{layer_id}_{shape_idx}"),
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
      format!("📁 {} ({} items)", label.short(), geometries.len())
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
          self.show_nested_geometry_content(
            ui,
            layer_id,
            shape_idx,
            &nested_path,
            nested_geometry,
            geometries.len(),
          );
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

    // Handle double-click to show popup (TODO: implement popup)
    if header_response.header_response.double_clicked() {
      println!("TODO: Show detail popup for collection");
    }

    // Add context menu for collection
    header_response.header_response.context_menu(|ui| {
      let geometry_key = (layer_id.to_string(), shape_idx);
      let geometry_visible = *self.geometry_visibility.get(&geometry_key).unwrap_or(&true);

      self.show_visibility_button(ui, geometry_visible, "Collection", |this| {
        this
          .geometry_visibility
          .insert(geometry_key.clone(), !geometry_visible);
      });

      ui.separator();
      ui.separator();

      if let Some(label) = &metadata.label {
        let popup_id = format!("collection_label_popup_{layer_id}_{shape_idx}");
        Self::show_label_button(ui, label, &popup_id);
      } else {
        ui.label("(No label available)");
      }

      let popup_id = format!("collection_popup_{layer_id}_{shape_idx}");
      Self::show_collection_info_button(ui, geometries, &popup_id);

      ui.separator();

      self.show_delete_collection_button(ui, layer_id, shape_idx, &geometry_key);
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
    sibling_count: usize,
  ) {
    let nested_key = (layer_id.to_string(), shape_idx, nested_path.to_vec());
    let nested_visible = *self
      .nested_geometry_visibility
      .get(&nested_key)
      .unwrap_or(&true);

    if let Geometry::GeometryCollection(nested_geometries, nested_metadata) = geometry {
      let collection_key = nested_key.clone();
      let is_expanded = *self
        .collection_expansion
        .get(&collection_key)
        .unwrap_or(&false);

      let collection_label = if let Some(label) = &nested_metadata.label {
        format!("📁 {} ({} items)", label.short(), nested_geometries.len())
      } else {
        format!("📁 Collection ({} items)", nested_geometries.len())
      };

      let header_id = egui::Id::new(format!(
        "nested_collection_{layer_id}_{shape_idx}_{nested_path:?}"
      ));
      let header_response = egui::CollapsingHeader::new(collection_label)
        .id_salt(header_id)
        .default_open(is_expanded)
        .show(ui, |ui| {
          let total_items = nested_geometries.len();
          let sibling_count = nested_geometries.len();

          let scroll_id = egui::Id::new(format!(
            "nested_scroll_{layer_id}_{shape_idx}_{nested_path:?}"
          ));
          egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .max_height(SCROLL_AREA_MAX_HEIGHT)
            .show(ui, |ui| {
              for (sub_idx, sub_geometry) in nested_geometries.iter().enumerate() {
                let mut sub_path = nested_path.to_vec();
                sub_path.push(sub_idx);
                self.show_nested_geometry_content(
                  ui,
                  layer_id,
                  shape_idx,
                  &sub_path,
                  sub_geometry,
                  sibling_count,
                );
                if sub_idx < total_items - 1 {
                  ui.separator();
                }
              }
            });
        });

      // Update expansion state
      let is_currently_open = header_response.body_response.is_some();
      self
        .collection_expansion
        .insert(collection_key, is_currently_open);

      // Add context menu for nested collection
      header_response.header_response.context_menu(|ui| {
        self.show_visibility_button(ui, nested_visible, "Collection", |this| {
          this
            .nested_geometry_visibility
            .insert(nested_key, !nested_visible);
        });

        ui.separator();

        // Show full label option for nested collections
        if let Some(label) = &nested_metadata.label {
          let popup_id_str = format!(
            "nested_label_{layer_id}_{shape_idx}_{}",
            nested_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join("_")
          );
          Self::show_label_button(ui, label, &popup_id_str);
        } else {
          ui.label("(No label available)");
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
        let eye_response = ui.add_sized([24.0, 20.0], egui::Button::new(visibility_icon));
        if eye_response.double_clicked() {
          let parent_path = &nested_path[..nested_path.len() - 1];
          let current_idx = nested_path[nested_path.len() - 1];
          // Check if this element is already solo (only visible one among siblings)
          let is_solo = nested_visible
            && (0..sibling_count).all(|i| {
              i == current_idx
                || !*self
                  .nested_geometry_visibility
                  .get(&{
                    let mut p = parent_path.to_vec();
                    p.push(i);
                    (layer_id.to_string(), shape_idx, p)
                  })
                  .unwrap_or(&true)
            });
          for i in 0..sibling_count {
            let mut sibling_path = parent_path.to_vec();
            sibling_path.push(i);
            self.nested_geometry_visibility.insert(
              (layer_id.to_string(), shape_idx, sibling_path),
              if is_solo { true } else { i == current_idx },
            );
          }
          self.invalidate_cache();
        } else if eye_response.clicked() {
          toggle_visibility = true;
        }

        // Show individual geometry content
        match geometry {
          Geometry::Point(coord, nested_metadata) => {
            let wgs84 = coord.as_wgs84();
            ui.label("📍");
            if let Some(label) = &nested_metadata.label {
              let available_width = (ui.available_width() - 40.0).max(100.0);
              let (truncated_label, _was_truncated) =
                truncate_label_by_width(ui, &label.short(), available_width);
              ui.strong(truncated_label);
            } else {
              ui.label("Point");
            }
            ui.small(format!("({:.3}, {:.3})", wgs84.lat, wgs84.lon));
          }
          Geometry::LineString(coords, nested_metadata) => {
            ui.label("📏");
            if let Some(label) = &nested_metadata.label {
              let available_width = (ui.available_width() - 40.0).max(100.0);
              let (truncated_label, _was_truncated) =
                truncate_label_by_width(ui, &label.short(), available_width);
              ui.strong(truncated_label);
            } else {
              ui.label("Line");
            }
            ui.small(format!("({} pts)", coords.len()));
          }
          Geometry::Polygon(coords, nested_metadata) => {
            ui.label("⬟");
            if let Some(label) = &nested_metadata.label {
              let available_width = (ui.available_width() - 40.0).max(100.0);
              let (truncated_label, _was_truncated) =
                truncate_label_by_width(ui, &label.short(), available_width);
              ui.strong(truncated_label);
            } else {
              ui.label("Polygon");
            }
            ui.small(format!("({} pts)", coords.len()));
          }
          Geometry::GeometryCollection(..) => {
            // This should not happen in individual geometry context
          }
          Geometry::Heatmap(coords, nested_metadata) => {
            ui.label("🔥");
            if let Some(label) = &nested_metadata.label {
              let available_width = (ui.available_width() - 40.0).max(100.0);
              let (truncated_label, _was_truncated) =
                truncate_label_by_width(ui, &label.short(), available_width);
              ui.strong(truncated_label);
            } else {
              ui.label("Heatmap");
            }
            ui.small(format!("({} pts)", coords.len()));
          }
        }
      });

      // Check if this individual nested geometry is highlighted for sidebar background
      let geometry_key_for_highlight = (layer_id.to_string(), shape_idx, nested_path.to_vec());
      let is_highlighted = self.geometry_highlighter.is_highlighted(
        &geometry_key_for_highlight.0,
        geometry_key_for_highlight.1,
        &geometry_key_for_highlight.2,
      );

      // Add background color to the horizontal response if highlighted
      if is_highlighted {
        let rect = horizontal_response.response.rect;
        ui.painter()
          .rect_filled(rect, 2.0, egui::Color32::from_rgb(100, 100, 200));

        // Scroll to this element if it was just double-clicked on the map
        if self
          .just_double_clicked
          .as_ref()
          .is_some_and(|(l, idx, path)| l == layer_id && *idx == shape_idx && path == nested_path)
        {
          horizontal_response
            .response
            .scroll_to_me(Some(egui::Align::Center));
        }
      }

      // Handle visibility toggle after the horizontal closure
      if toggle_visibility {
        self
          .nested_geometry_visibility
          .insert(nested_key.clone(), !nested_visible);
        self.invalidate_cache();
      }

      // Handle double-click to show popup (TODO: implement popup)
      if horizontal_response.response.double_clicked() {
        println!("TODO: Show detail popup for individual nested geometry");
      }

      // Add context menu to individual geometries
      horizontal_response.response.context_menu(|ui| {
        self.show_visibility_button(ui, nested_visible, "Geometry", |this| {
          this
            .nested_geometry_visibility
            .insert(nested_key, !nested_visible);
          this.invalidate_cache();
        });
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
      if metadata.style.is_none()
        && let Some(shapes) = self.shape_map.get_mut(layer_id)
        && let Some(shape) = shapes.get_mut(shape_idx)
      {
        let shape_metadata = match shape {
          Geometry::Point(_, metadata)
          | Geometry::LineString(_, metadata)
          | Geometry::Polygon(_, metadata)
          | Geometry::GeometryCollection(_, metadata)
          | Geometry::Heatmap(_, metadata) => metadata,
        };
        shape_metadata.style = Some(crate::map::geometry_collection::Style::default());
      }
      ui.memory_mut(|mem| mem.data.insert_temp(popup_id, true));
    }
  }

  fn update_shape_color(&mut self, layer_id: &str, shape_idx: usize, new_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id)
      && let Some(shape) = shapes.get_mut(shape_idx)
    {
      let metadata = match shape {
        Geometry::Point(_, metadata)
        | Geometry::LineString(_, metadata)
        | Geometry::Polygon(_, metadata)
        | Geometry::GeometryCollection(_, metadata)
        | Geometry::Heatmap(_, metadata) => metadata,
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

  fn update_shape_stroke_color(&mut self, layer_id: &str, shape_idx: usize, new_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id)
      && let Some(shape) = shapes.get_mut(shape_idx)
    {
      let metadata = match shape {
        Geometry::Point(_, metadata)
        | Geometry::LineString(_, metadata)
        | Geometry::Polygon(_, metadata)
        | Geometry::GeometryCollection(_, metadata)
        | Geometry::Heatmap(_, metadata) => metadata,
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

  fn update_shape_fill_color(&mut self, layer_id: &str, shape_idx: usize, new_fill_color: Color32) {
    if let Some(shapes) = self.shape_map.get_mut(layer_id)
      && let Some(shape) = shapes.get_mut(shape_idx)
    {
      let metadata = match shape {
        Geometry::Point(_, metadata)
        | Geometry::LineString(_, metadata)
        | Geometry::Polygon(_, metadata)
        | Geometry::GeometryCollection(_, metadata)
        | Geometry::Heatmap(_, metadata) => metadata,
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

  /// Check for nested collection popups at any depth
  fn check_nested_popups_recursive(
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    geometries: &[Geometry<PixelCoordinate>],
    current_path: &mut Vec<usize>,
  ) {
    for (nested_idx, nested_geometry) in geometries.iter().enumerate() {
      current_path.push(nested_idx);

      if let Geometry::GeometryCollection(sub_geometries, metadata) = nested_geometry {
        // Check if this collection has a label and could be a popup target
        if metadata.label.is_some() {
          let popup_id_str = format!(
            "nested_label_{layer_id}_{shape_idx}_{}",
            current_path
              .iter()
              .map(std::string::ToString::to_string)
              .collect::<Vec<_>>()
              .join("_")
          );
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

        // Recursively check deeper nesting levels
        Self::check_nested_popups_recursive(ui, layer_id, shape_idx, sub_geometries, current_path);
      }

      current_path.pop();
    }
  }
  // Context menu helpers
  fn show_visibility_button(
    &mut self,
    ui: &mut egui::Ui,
    is_visible: bool,
    item_type: &str,
    toggle_action: impl FnOnce(&mut Self),
  ) {
    let visibility_text = if is_visible { "Hide" } else { "Show" };
    if ui
      .button(format!("{visibility_text} {item_type}"))
      .clicked()
    {
      toggle_action(self);
      ui.close();
    }
  }

  fn show_label_button(
    ui: &mut egui::Ui,
    label: &crate::map::geometry_collection::Label,
    popup_id: &str,
  ) {
    if ui.button("📄 Show Full Label").clicked() {
      let id = egui::Id::new(popup_id);
      ui.memory_mut(|mem| mem.data.insert_temp(id, label.full()));
      ui.close();
    }
  }

  fn show_delete_geometry_button(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    geometry_key: &(String, usize),
  ) {
    if ui.button("🗑 Delete Geometry").clicked() {
      if let Some(shapes) = self.shape_map.get_mut(layer_id)
        && shape_idx < shapes.len()
      {
        shapes.remove(shape_idx);
        self.geometry_visibility.remove(geometry_key);

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
        self.invalidate_cache();
      }
      ui.close();
    }
  }

  fn show_collection_info_button(
    ui: &mut egui::Ui,
    geometries: &[Geometry<PixelCoordinate>],
    popup_id: &str,
  ) {
    if ui.button("📋 Collection Info").clicked() {
      let id = egui::Id::new(popup_id);
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
            Geometry::Heatmap(coords, _) => format!("Heatmap ({})", coords.len()),
          })
          .collect::<Vec<_>>()
          .join(", ")
      );
      ui.memory_mut(|mem| mem.data.insert_temp(id, collection_info));
      ui.close();
    }
  }

  /// Highlight a geometry by its path (converts to ID-based highlighting)
  pub(super) fn highlight_geometry(
    &mut self,
    layer_id: &str,
    shape_idx: usize,
    nested_path: &[usize],
  ) {
    self
      .geometry_highlighter
      .highlight_geometry(layer_id, shape_idx, nested_path);
  }

  /// Draw highlighting for a single specific geometry using the `geometry_highlighting` module
  fn draw_highlighted_geometry(
    geometry: &Geometry<PixelCoordinate>,
    painter: &egui::Painter,
    transform: &Transform,
    _highlight_all: bool, // Unused - we never highlight entire collections
  ) {
    use super::super::geometry_highlighting::draw_highlighted_geometry;
    draw_highlighted_geometry(geometry, painter, transform, false);
  }

  /// Render the hover-highlight for the currently selected geometry.
  /// Polygon fills are rasterized via tiny-skia and cached as a texture;
  /// strokes/points/lines are added as egui shapes.
  pub(super) fn draw_highlight_overlay(
    &mut self,
    ui: &mut egui::Ui,
    transform: &Transform,
    rect: Rect,
  ) {
    let Some((layer_id, shape_idx, nested_path)) =
      self.geometry_highlighter.get_highlighted_geometry()
    else {
      self.highlight_texture = None;
      return;
    };
    if !nested_path.is_empty() {
      // The render loop only handles top-level highlights; preserve that.
      self.highlight_texture = None;
      return;
    }
    if !*self.layer_visibility.get(&layer_id).unwrap_or(&true) {
      self.highlight_texture = None;
      return;
    }
    if !*self
      .geometry_visibility
      .get(&(layer_id.clone(), shape_idx))
      .unwrap_or(&true)
    {
      self.highlight_texture = None;
      return;
    }
    let Some(shape) = self
      .shape_map
      .get(&layer_id)
      .and_then(|s| s.get(shape_idx))
      .cloned()
    else {
      self.highlight_texture = None;
      return;
    };

    let key = HighlightCacheKey {
      geometry_path: (layer_id, shape_idx, nested_path),
      viewport: [
        rect.min.x.to_bits(),
        rect.min.y.to_bits(),
        rect.max.x.to_bits(),
        rect.max.y.to_bits(),
      ],
      transform: [
        transform.zoom.to_bits(),
        transform.trans.x.to_bits(),
        transform.trans.y.to_bits(),
      ],
      version: self.version,
    };

    let needs_rebuild = self.highlight_texture.as_ref().is_none_or(|c| c.key != key);
    if needs_rebuild {
      use super::super::geometry_highlighting::rasterize_highlighted_polygons;
      if let Some((image, screen_rect)) = rasterize_highlighted_polygons(&shape, transform, rect) {
        let handle =
          ui.ctx()
            .load_texture("highlight_polygon", image, egui::TextureOptions::LINEAR);
        self.highlight_texture = Some(HighlightTextureCache {
          key,
          texture: handle,
          screen_rect,
        });
      } else {
        self.highlight_texture = None;
      }
    }

    let painter = ui.painter_at(rect);
    if let Some(cache) = &self.highlight_texture {
      painter.image(
        cache.texture.id(),
        cache.screen_rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
      );
    }
    Self::draw_highlighted_geometry(&shape, &painter, transform, false);
  }

  fn show_delete_collection_button(
    &mut self,
    ui: &mut egui::Ui,
    layer_id: &str,
    shape_idx: usize,
    geometry_key: &(String, usize),
  ) {
    if ui.button("🗑 Delete Collection").clicked() {
      if let Some(shapes) = self.shape_map.get_mut(layer_id)
        && shape_idx < shapes.len()
      {
        shapes.remove(shape_idx);
        self.geometry_visibility.remove(geometry_key);

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
        self.invalidate_cache();
      }
      ui.close();
    }
  }
}
