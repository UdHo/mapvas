use std::rc::Rc;

use egui::Widget as _;

use crate::{
  config::{Config, TileProvider},
  map::mapvas_egui::{Map, MapLayerHolder},
  profile_scope,
  remote::Remote,
  search::{SearchManager, SearchProviderConfig, ui::SearchUI},
};

/// Holds the UI data of mapvas.
pub struct MapApp {
  map: Map,
  sidebar: Sidebar,
  settings_dialog: std::rc::Rc<std::cell::RefCell<SettingsDialog>>,
  previous_had_highlighted: bool,
}

impl MapApp {
  pub fn new(
    map: Map,
    remote: Remote,
    map_content: Rc<dyn MapLayerHolder>,
    config: Config,
  ) -> Self {
    let settings_dialog =
      std::rc::Rc::new(std::cell::RefCell::new(SettingsDialog::new(config.clone())));
    let sidebar = Sidebar::new(remote, map_content, config, settings_dialog.clone());
    Self {
      map,
      sidebar,
      settings_dialog,
      previous_had_highlighted: false,
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
            .stroke(egui::Stroke::new(
              1.0,
              egui::Color32::from_rgba_unmultiplied(0, 0, 0, 60),
            )),
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
            egui::vec2(line_width, line_height),
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
    profile_scope!("MapApp::update");
    // Mark frame for profiling
    crate::profiling::new_frame();
    
    // Handle keyboard shortcut for sidebar toggle
    ctx.input(|i| {
      if i.key_pressed(egui::Key::F1) || (i.modifiers.ctrl && i.key_pressed(egui::Key::B)) {
        self.sidebar.toggle();
      }
    });

    // Update sidebar animation
    self.sidebar.update_animation(ctx);

    // Show settings dialog if open
    self.settings_dialog.borrow_mut().ui(ctx);

    // Show sidebar with smooth animations
    let effective_width = self.sidebar.get_animated_width();

    if effective_width > 1.0 {
      profile_scope!("MapApp::sidebar");
      egui::SidePanel::left("sidebar")
        .default_width(self.sidebar.width)
        .width_range(200.0..=600.0)
        .resizable(true) // Use egui's built-in resize handle
        .show(ctx, |ui| {
          let alpha = self.sidebar.get_content_alpha();
          ui.set_opacity(alpha);

          self.sidebar.ui(ui);

          self.sidebar.width = ui.available_width().clamp(200.0, 600.0);
        });
    }

    if !self.sidebar.is_fully_visible() {
      self.show_sidebar_toggle_button(ctx);
    }

    // Show sidebar when geometry becomes newly highlighted (from double-click)
    let has_highlighted = self.map.has_highlighted_geometry();
    if has_highlighted && !self.previous_had_highlighted {
      self.sidebar.show();
    }
    self.previous_had_highlighted = has_highlighted;

    egui::CentralPanel::default()
      .frame(egui::Frame::NONE)
      .show(ctx, |ui| {
        profile_scope!("MapApp::central_panel");
        (&mut self.map).ui(ui);
      });
  }
}

struct Sidebar {
  target_visible: bool,
  width: f32,
  animation_progress: f32,
  last_frame_time: f64,
  #[allow(dead_code)]
  remote: Remote,
  map_content: Rc<dyn MapLayerHolder>,
  search_ui: SearchUI,
  settings_dialog: std::rc::Rc<std::cell::RefCell<SettingsDialog>>,
}

impl Sidebar {
  fn new(
    remote: Remote,
    map_content: Rc<dyn MapLayerHolder>,
    config: Config,
    settings_dialog: std::rc::Rc<std::cell::RefCell<SettingsDialog>>,
  ) -> Self {
    let search_manager = if config.search_providers.is_empty() {
      SearchManager::new()
    } else {
      SearchManager::with_config(config.search_providers).unwrap_or_else(|e| {
        log::warn!("Failed to create search manager with config: {e}, using default");
        SearchManager::new()
      })
    };

    Self {
      target_visible: true,
      width: 300.0,
      animation_progress: 1.0,
      last_frame_time: 0.0,
      remote,
      map_content,
      search_ui: SearchUI::new(search_manager),
      settings_dialog,
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
          let close_button_size = egui::vec2(24.0, 24.0);
          let (close_rect, close_response) =
            ui.allocate_exact_size(close_button_size, egui::Sense::click());

          if close_response.hovered() {
            ui.painter()
              .rect_filled(close_rect, 4.0, egui::Color32::from_gray(200));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
          }
          // Draw X symbol with better styling
          let center = close_rect.center();
          let size = 8.0;
          let color = egui::Color32::from_gray(100);
          let stroke_width = 1.5;
          // Draw the X
          ui.painter().line_segment(
            [
              center + egui::vec2(-size / 2.0, -size / 2.0),
              center + egui::vec2(size / 2.0, size / 2.0),
            ],
            egui::Stroke::new(stroke_width, color),
          );
          ui.painter().line_segment(
            [
              center + egui::vec2(-size / 2.0, size / 2.0),
              center + egui::vec2(size / 2.0, -size / 2.0),
            ],
            egui::Stroke::new(stroke_width, color),
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

      let available_height = ui.available_height();
      let settings_button_height = 32.0;

      egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(available_height - settings_button_height)
        .show(ui, |ui| {
          egui::CollapsingHeader::new("🔍Location Search")
            .default_open(true)
            .show(ui, |ui| {
              self.search_ui.ui(ui, &self.remote.sender());
            });

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
        });
    });

    ui.separator();
    if ui.button("Settings").clicked() {
      self.settings_dialog.borrow_mut().open();
    }
  }
}

#[derive(Clone)]
struct SettingsDialog {
  open: bool,
  config: Config,
  tile_providers: Vec<TileProvider>,
  selected_tab: SettingsTab,
  new_provider_name: String,
  new_provider_url: String,
  cache_directory: String,
  screenshot_path: String,
  settings_changed: bool,
  search_providers: Vec<SearchProviderConfig>,
  new_search_provider_name: String,
  new_search_provider_url: String,
  new_search_provider_headers: String,
  nominatim_base_url: String,
}

#[derive(Clone, PartialEq)]
enum SettingsTab {
  TileProviders,
  SearchProviders,
  General,
}

impl SettingsDialog {
  fn new(config: Config) -> Self {
    let cache_directory = config
      .tile_cache_dir
      .as_ref()
      .map_or_else(|| "Default".to_string(), |p| p.display().to_string());

    let screenshot_path =
      std::env::var("MAPVAS_SCREENSHOT_PATH").unwrap_or_else(|_| "Desktop".to_string());

    Self {
      open: false,
      tile_providers: config.tile_provider.clone(),
      search_providers: config.search_providers.clone(),
      config,
      selected_tab: SettingsTab::General,
      new_provider_name: String::new(),
      new_provider_url: String::new(),
      cache_directory,
      screenshot_path,
      settings_changed: false,
      new_search_provider_name: String::new(),
      new_search_provider_url: String::new(),
      new_search_provider_headers: String::new(),
      nominatim_base_url: String::new(),
    }
  }

  fn open(&mut self) {
    self.open = true;
  }

  fn ui(&mut self, ctx: &egui::Context) {
    if !self.open {
      return;
    }

    let mut open = self.open;
    egui::Window::new("Settings")
      .collapsible(false)
      .resizable(true)
      .default_size([600.0, 400.0])
      .open(&mut open)
      .show(ctx, |ui| {
        ui.horizontal(|ui| {
          // Tab buttons
          ui.selectable_value(&mut self.selected_tab, SettingsTab::General, "General");
          ui.selectable_value(
            &mut self.selected_tab,
            SettingsTab::TileProviders,
            "Tile Providers",
          );
          ui.selectable_value(
            &mut self.selected_tab,
            SettingsTab::SearchProviders,
            "Search Providers",
          );
        });

        ui.separator();

        egui::ScrollArea::vertical()
          .auto_shrink([false; 2])
          .show(ui, |ui| match self.selected_tab {
            SettingsTab::General => self.general_settings_ui(ui),
            SettingsTab::TileProviders => self.tile_providers_ui(ui),
            SettingsTab::SearchProviders => self.search_providers_ui(ui),
          });
      });
    self.open = open;
  }

  fn general_settings_ui(&mut self, ui: &mut egui::Ui) {
    ui.heading("General Settings");
    ui.separator();

    ui.group(|ui| {
      ui.label("Cache Settings:");
      ui.horizontal(|ui| {
        ui.label("Tile cache directory:");
        if ui.text_edit_singleline(&mut self.cache_directory).changed() {
          self.settings_changed = true;
        }
      });
      ui.small("Leave as 'Default' to use the default cache location");
    });

    ui.group(|ui| {
      ui.label("Screenshot Settings:");
      ui.horizontal(|ui| {
        ui.label("Default screenshot path:");
        if ui.text_edit_singleline(&mut self.screenshot_path).changed() {
          self.settings_changed = true;
        }
      });
      ui.small("Path where screenshots will be saved (use 'Desktop' for default)");
    });

    ui.group(|ui| {
      ui.label("Config Location:");
      if let Some(config_path) = &self.config.config_path {
        ui.label(format!("Config directory: {}", config_path.display()));
      } else {
        ui.label("Config directory: Using default");
      }
      ui.small("Config file location (read-only)");
    });

    ui.separator();

    // Save button
    ui.horizontal(|ui| {
      if self.settings_changed {
        if ui.button("Save Settings").clicked() {
          self.save_settings();
        }
        ui.label("Settings have been modified");
      } else {
        ui.label("No changes to save");
      }
    });
  }

  fn tile_providers_ui(&mut self, ui: &mut egui::Ui) {
    ui.heading("Tile Providers");
    ui.separator();

    ui.label("Configure tile servers for map rendering:");

    // List existing providers
    ui.group(|ui| {
      ui.label("Current Providers:");
      let mut to_remove = None;
      for (i, provider) in self.tile_providers.iter().enumerate() {
        ui.horizontal(|ui| {
          ui.label(&provider.name);
          ui.label("-");
          ui.small(&provider.url);
          if ui.small_button("🗑").clicked() && self.tile_providers.len() > 1 {
            to_remove = Some(i);
          }
        });
      }
      if let Some(i) = to_remove {
        self.tile_providers.remove(i);
        self.settings_changed = true;
      }
    });

    ui.separator();

    // Add new provider
    ui.group(|ui| {
      ui.label("Add New Provider:");
      ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut self.new_provider_name);
      });
      ui.horizontal(|ui| {
        ui.label("URL:");
        ui.text_edit_singleline(&mut self.new_provider_url);
      });
      ui.small("Use {x}, {y}, {zoom} as placeholders (e.g., https://tile.openstreetmap.org/{zoom}/{x}/{y}.png)");
      if ui.button("Add Provider").clicked() && !self.new_provider_name.is_empty() && !self.new_provider_url.is_empty() {
        self.tile_providers.push(TileProvider {
          name: self.new_provider_name.clone(),
          url: self.new_provider_url.clone(),
        });
        self.new_provider_name.clear();
        self.new_provider_url.clear();
        self.settings_changed = true;
      }
    });

    ui.separator();

    // Save button for tile providers
    ui.horizontal(|ui| {
      if self.settings_changed {
        if ui.button("Save Tile Providers").clicked() {
          self.save_settings();
        }
        ui.label("Tile provider changes need to be saved");
      }
    });
  }

  #[allow(clippy::too_many_lines)]
  fn search_providers_ui(&mut self, ui: &mut egui::Ui) {
    ui.heading("Search Providers");
    ui.separator();

    ui.label("Configure location search services:");

    // List current providers with ability to remove
    ui.group(|ui| {
      ui.label("Current Providers:");
      let mut to_remove = None;
      for (i, provider) in self.search_providers.iter().enumerate() {
        ui.horizontal(|ui| {
          match provider {
            SearchProviderConfig::Coordinate => {
              ui.label("🧭 Coordinate Parser");
              ui.label("(built-in - parses lat/lng from text)");
            }
            SearchProviderConfig::Nominatim { base_url } => {
              ui.label("🌍 Nominatim");
              if let Some(url) = base_url {
                ui.small(url);
              } else {
                ui.small("(default OpenStreetMap geocoding)");
              }
              // Don't allow removing if it's the only non-coordinate provider
              if self.search_providers.len() > 2 && ui.small_button("🗑").clicked() {
                to_remove = Some(i);
              }
            }
            SearchProviderConfig::Custom {
              name, url_template, ..
            } => {
              ui.label(format!("🔧 {name}"));
              ui.small(url_template);
              if ui.small_button("🗑").clicked() {
                to_remove = Some(i);
              }
            }
          }
        });
      }
      if let Some(i) = to_remove {
        self.search_providers.remove(i);
        self.settings_changed = true;
      }
    });

    ui.separator();

    // Nominatim configuration
    ui.group(|ui| {
      ui.label("Nominatim Configuration:");
      ui.horizontal(|ui| {
        ui.label("Custom base URL (optional):");
        if ui
          .text_edit_singleline(&mut self.nominatim_base_url)
          .changed()
        {
          self.settings_changed = true;
        }
      });
      ui.small("Leave empty for default OpenStreetMap Nominatim");

      if ui.button("Update Nominatim").clicked() {
        // Update existing Nominatim provider or add new one
        let base_url = if self.nominatim_base_url.trim().is_empty() {
          None
        } else {
          Some(self.nominatim_base_url.trim().to_string())
        };

        // Find and update existing Nominatim provider
        let mut found = false;
        for provider in &mut self.search_providers {
          if matches!(provider, SearchProviderConfig::Nominatim { .. }) {
            *provider = SearchProviderConfig::Nominatim {
              base_url: base_url.clone(),
            };
            found = true;
            break;
          }
        }

        // If no Nominatim provider exists, add one
        if !found {
          self
            .search_providers
            .push(SearchProviderConfig::Nominatim { base_url });
        }

        self.settings_changed = true;
      }
    });

    ui.separator();

    // Add custom provider
    ui.group(|ui| {
      ui.label("Add Custom Search Provider:");
      ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut self.new_search_provider_name);
      });
      ui.horizontal(|ui| {
        ui.label("URL Template:");
        ui.text_edit_singleline(&mut self.new_search_provider_url);
      });
      ui.horizontal(|ui| {
        ui.label("Headers (JSON, optional):");
        ui.text_edit_singleline(&mut self.new_search_provider_headers);
      });
      ui.small(
        "URL should use {query} placeholder (e.g., https://api.example.com/search?q={query})",
      );
      ui.small("Headers example: {\"Authorization\": \"Bearer YOUR_API_KEY\"}");

      if ui.button("Add Custom Provider").clicked()
        && !self.new_search_provider_name.is_empty()
        && !self.new_search_provider_url.is_empty()
      {
        // Parse headers if provided
        let headers = if self.new_search_provider_headers.trim().is_empty() {
          None
        } else if let Ok(h) = serde_json::from_str(&self.new_search_provider_headers) {
          Some(h)
        } else {
          log::warn!("Invalid JSON headers, ignoring");
          None
        };

        self.search_providers.push(SearchProviderConfig::Custom {
          name: self.new_search_provider_name.clone(),
          url_template: self.new_search_provider_url.clone(),
          headers,
        });

        self.new_search_provider_name.clear();
        self.new_search_provider_url.clear();
        self.new_search_provider_headers.clear();
        self.settings_changed = true;
      }
    });

    ui.separator();

    // Save button for search providers
    ui.horizontal(|ui| {
      if self.settings_changed {
        if ui.button("Save Search Providers").clicked() {
          self.save_settings();
        }
        ui.label("Search provider changes need to be saved");
      }
    });
  }

  fn save_settings(&mut self) {
    use std::path::PathBuf;

    // Update config with current settings
    self.config.tile_provider = self.tile_providers.clone();
    self.config.search_providers = self.search_providers.clone();

    // Update cache directory if changed
    if self.cache_directory != "Default" {
      self.config.tile_cache_dir = Some(PathBuf::from(&self.cache_directory));
    }

    // Note: Screenshot path is handled by the application at runtime
    // We'll store it in the config for future reference
    if self.screenshot_path != "Desktop" {
      // In a real application, you'd want to handle screenshot path differently
      log::info!("Screenshot path set to: {}", self.screenshot_path);
    }

    // Save to config file
    if let Some(config_path) = &self.config.config_path {
      let config_file = config_path.join("config.json");
      match serde_json::to_string_pretty(&self.config) {
        Ok(config_json) => {
          if let Err(e) = std::fs::write(&config_file, config_json) {
            log::error!("Failed to save config file: {e}");
          } else {
            log::info!("Settings saved to {}", config_file.display());
            self.settings_changed = false;
          }
        }
        Err(e) => {
          log::error!("Failed to serialize config: {e}");
        }
      }
    } else {
      log::warn!("No config path available, settings not saved");
    }
  }
}
