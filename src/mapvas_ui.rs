use std::rc::Rc;

use egui::Widget as _;

use crate::{
  command_line::{Command, CommandLine, handle_command_line_input, show_command_line_ui},
  config::{Config, HeadingStyle, TileProvider, TileType},
  map::{
    mapvas_egui::{Map, MapLayerHolder, timeline_widget::IntervalLock},
    tile_renderer::{
      init_style_config, save_style_config, set_style_config, style_config, Rgb, RoadStyle,
      StyleConfig,
    },
  },
  profile_scope,
  remote::Remote,
  search::{SearchManager, SearchProviderConfig, ui::SearchUI},
  task_tracker::{task_tracker, TaskCategory},
};
use chrono::{DateTime, Utc};
use tokio_metrics::RuntimeMonitor;

/// Holds the UI data of mapvas.
pub struct MapApp {
  map: Map,
  sidebar: Sidebar,
  settings_dialog: std::rc::Rc<std::cell::RefCell<SettingsDialog>>,
  previous_had_highlighted: bool,
  last_heading_style: HeadingStyle,
  command_line: CommandLine,
  runtime_monitor: Option<RuntimeMonitor>,
}

impl MapApp {
  #[allow(clippy::needless_pass_by_value)]
  pub fn new(
    map: Map,
    remote: Remote,
    map_content: Rc<dyn MapLayerHolder>,
    config: Config,
    runtime_monitor: Option<RuntimeMonitor>,
  ) -> Self {
    let settings_dialog =
      std::rc::Rc::new(std::cell::RefCell::new(SettingsDialog::new(config.clone())));
    let sidebar = Sidebar::new(remote, map_content, config.clone(), settings_dialog.clone());
    Self {
      map,
      sidebar,
      settings_dialog,
      previous_had_highlighted: false,
      last_heading_style: config.heading_style,
      command_line: CommandLine::new(),
      runtime_monitor,
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
  #[allow(clippy::too_many_lines)]
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    profile_scope!("MapApp::update");
    // Mark frame for profiling
    crate::profiling::new_frame();

    // Handle keyboard shortcuts
    ctx.input(|i| {
      // Sidebar toggle
      if i.key_pressed(egui::Key::F1) || (i.modifiers.ctrl && i.key_pressed(egui::Key::B)) {
        self.sidebar.toggle();
      }
      // Timeline layer toggle
      if i.modifiers.ctrl && i.key_pressed(egui::Key::T) {
        let current_visible = self.map.is_timeline_visible();
        self.map.set_timeline_visible(!current_visible);
      }

      // Temporal filter toggle
      if i.modifiers.ctrl && i.key_pressed(egui::Key::F) {
        let current_enabled = self.sidebar.temporal_controls.is_filter_enabled();
        self
          .sidebar
          .temporal_controls
          .set_filter_enabled(!current_enabled);

        let status = if current_enabled {
          "disabled"
        } else {
          "enabled"
        };
        self
          .command_line
          .set_message(format!("Temporal filtering {status}"), false);
      }

      // Timeline interval lock toggle
      if i.modifiers.ctrl && i.key_pressed(egui::Key::L) {
        self.map.toggle_timeline_interval_lock();

        let lock_state = self.map.get_timeline_interval_lock();
        let status = match lock_state {
          IntervalLock::None => "unlocked",
          IntervalLock::Start => "start locked",
          IntervalLock::End => "end locked",
        };
        self
          .command_line
          .set_message(format!("Timeline interval {status}"), false);
      }
    });

    // Update sidebar animation
    self.sidebar.update_animation(ctx);

    // Show settings dialog if open and check for config changes
    self.settings_dialog.borrow_mut().ui(ctx);

    // Update map config if heading style has changed (for real-time updates)
    let current_config = self.settings_dialog.borrow().get_current_config();
    if current_config.heading_style != self.last_heading_style {
      self.map.update_config(&current_config);
      self.last_heading_style = current_config.heading_style;
    }

    // Always refresh timeline data to check for temporal events
    self
      .sidebar
      .temporal_controls
      .init_from_layers(&*self.sidebar.map_content);

    // Check if we have temporal data, and auto-activate timeline if we do
    let has_temporal_data = self.sidebar.temporal_controls.time_start.is_some()
      && self.sidebar.temporal_controls.time_end.is_some();

    if !has_temporal_data || !self.sidebar.temporal_controls.filter_enabled {
      // No temporal data OR user has disabled filtering - disable temporal filtering (but leave timeline visibility as-is for user control)
      self.map.set_temporal_filter(None, None);
    } else {
      // We have temporal data AND filtering is enabled - update timeline and filtering

      // Update the timeline layer with current settings (always, so it maintains state)
      let time_range = (
        self.sidebar.temporal_controls.time_start,
        self.sidebar.temporal_controls.time_end,
      );

      if self.map.is_timeline_visible() {
        // Timeline is visible - sync with its state

        // Sync playback state from timeline layer back to sidebar controls
        let (timeline_playing, timeline_speed) = self.map.get_timeline_playback_state();
        self.sidebar.temporal_controls.is_playing = timeline_playing;
        self.sidebar.temporal_controls.playback_speed = timeline_speed;

        // Get the current interval from the timeline layer
        let (interval_start, interval_end) = self.map.get_timeline_interval();

        // If we don't have an interval yet from the timeline, initialize with full range
        let current_interval = if interval_start.is_none() || interval_end.is_none() {
          if let (Some(start), Some(end)) = time_range {
            // Start with the full range visible
            (Some(start), Some(end))
          } else {
            (None, None)
          }
        } else {
          (interval_start, interval_end)
        };

        self.map.update_timeline(
          time_range,
          current_interval,
          self.sidebar.temporal_controls.is_playing,
          self.sidebar.temporal_controls.playback_speed,
        );

        // Use the midpoint of the interval as current_time and the interval size as time_window
        let (current_time, time_window) =
          if let (Some(start), Some(end)) = (current_interval.0, current_interval.1) {
            let midpoint = start + (end.signed_duration_since(start) / 2);
            let window_size = end.signed_duration_since(start);
            (Some(midpoint), Some(window_size))
          } else {
            // Fallback to sidebar controls if timeline doesn't have an interval yet
            (
              self.sidebar.temporal_controls.current_time,
              self.sidebar.temporal_controls.time_window,
            )
          };

        self.map.set_temporal_filter(current_time, time_window);
      } else {
        // Timeline is hidden - disable temporal filtering completely to show all elements
        self.map.set_temporal_filter(None, None);

        // Still update timeline layer with time range so it's ready when made visible
        self.map.update_timeline(
          time_range,
          (None, None), // No active interval when hidden
          false,        // Not playing when hidden
          self.sidebar.temporal_controls.playback_speed,
        );
      }
    }

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

          self.sidebar.ui(ui, self.runtime_monitor.as_ref());

          self.sidebar.width = ui.available_width().clamp(200.0, 600.0);
        });
    }

    if !self.sidebar.is_fully_visible() {
      self.show_sidebar_toggle_button(ctx);
    }

    // Show sidebar when double-click action occurs (but not on hover highlighting)
    let has_double_click = self.map.has_double_click_action();
    if has_double_click && !self.previous_had_highlighted {
      self.sidebar.show();
    }
    self.previous_had_highlighted = has_double_click;

    // Handle command line input and execute commands
    if let Some(command) = handle_command_line_input(&mut self.command_line, ctx) {
      self.execute_command(command, ctx);
    }

    egui::CentralPanel::default()
      .frame(egui::Frame::NONE)
      .show(ctx, |ui| {
        profile_scope!("MapApp::central_panel");
        (&mut self.map).ui(ui);
      });

    // Show command line UI (must be after CentralPanel to appear on top)
    show_command_line_ui(&mut self.command_line, ctx);
  }
}

impl MapApp {
  /// Execute a command from the command line
  #[allow(clippy::too_many_lines)]
  fn execute_command(&mut self, command: Command, ctx: &egui::Context) {
    match command {
      Command::Quit => {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        self.command_line.set_message("Goodbye!".to_string(), false);
      }
      Command::Write => {
        // TODO: Implement save functionality
        self
          .command_line
          .set_message("Write command not implemented yet".to_string(), true);
      }
      Command::WriteQuit => {
        // TODO: Implement save functionality, then quit
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        self
          .command_line
          .set_message("Write and quit".to_string(), false);
      }
      Command::Search(query) => {
        // Use the existing search functionality
        self.map.search_geometries(&query);
        let results_count = self.map.get_search_results_count();
        if results_count > 0 {
          self.command_line.set_message(
            format!("Found {results_count} results for '{query}'"),
            false,
          );
          // Show sidebar to display search results
          self.sidebar.show();
        } else {
          self
            .command_line
            .set_message(format!("No results found for '{query}'"), true);
        }
      }
      Command::SearchNext => {
        if self.map.next_search_result() {
          let results_count = self.map.get_search_results_count();
          self
            .command_line
            .set_message(format!("Next search result ({results_count} total)"), false);
        } else {
          self
            .command_line
            .set_message("No search results available".to_string(), true);
        }
      }
      Command::SearchPrev => {
        if self.map.previous_search_result() {
          let results_count = self.map.get_search_results_count();
          self.command_line.set_message(
            format!("Previous search result ({results_count} total)"),
            false,
          );
        } else {
          self
            .command_line
            .set_message("No search results available".to_string(), true);
        }
      }
      Command::Filter(query) => {
        self.map.filter_geometries(&query);
        self
          .command_line
          .set_message(format!("Applied filter: '{query}'"), false);
      }
      Command::ClearFilter => {
        self.map.clear_filter();
        self
          .command_line
          .set_message("Filter cleared - showing all geometries".to_string(), false);
      }
      Command::GoTo(location) => {
        // TODO: Implement go to location (could use location search)
        self.command_line.set_message(
          format!("Go to location '{location}' not implemented yet"),
          true,
        );
      }
      Command::Focus(target) => {
        // TODO: Implement focus on specific layer or geometry
        self
          .command_line
          .set_message(format!("Focus on '{target}' not implemented yet"), true);
      }
      Command::ShowLayer(layer) => {
        if self.map.show_layer(&layer) {
          self
            .command_line
            .set_message(format!("Showed layer '{layer}'"), false);
        } else {
          self
            .command_line
            .set_message(format!("Layer '{layer}' not found"), true);
        }
      }
      Command::HideLayer(layer) => {
        if self.map.hide_layer(&layer) {
          self
            .command_line
            .set_message(format!("Hid layer '{layer}'"), false);
        } else {
          self
            .command_line
            .set_message(format!("Layer '{layer}' not found"), true);
        }
      }
      Command::ToggleLayer(layer) => {
        if self.map.toggle_layer(&layer) {
          self
            .command_line
            .set_message(format!("Toggled layer '{layer}'"), false);
        } else {
          self
            .command_line
            .set_message(format!("Layer '{layer}' not found"), true);
        }
      }
      Command::ZoomIn => {
        self.map.zoom_in();
        self
          .command_line
          .set_message("Zoomed in".to_string(), false);
      }
      Command::ZoomOut => {
        self.map.zoom_out();
        self
          .command_line
          .set_message("Zoomed out".to_string(), false);
      }
      Command::ZoomFit => {
        self.map.zoom_fit();
        self
          .command_line
          .set_message("Fit to view".to_string(), false);
      }
      Command::ToggleTemporalFilter => {
        let current_visible = self.map.is_timeline_visible();
        self.map.set_timeline_visible(!current_visible);
        let status = if current_visible {
          "disabled"
        } else {
          "enabled"
        };
        self
          .command_line
          .set_message(format!("Timeline {status}"), false);
      }
      Command::Unknown(cmd) => {
        self
          .command_line
          .set_message(format!("Unknown command: '{cmd}'"), true);
      }
    }
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
  temporal_controls: TemporalControls,
}

/// Controls for temporal visualization
#[derive(Default)]
struct TemporalControls {
  /// Whether timeline is currently playing
  is_playing: bool,
  /// Current time position in the timeline
  current_time: Option<DateTime<Utc>>,
  /// Start of the time range
  time_start: Option<DateTime<Utc>>,
  /// End of the time range
  time_end: Option<DateTime<Utc>>,
  /// Playback speed multiplier (1.0 = real time)
  playback_speed: f32,
  /// Time window duration (None = point in time, Some = duration window)
  time_window: Option<chrono::Duration>,
  /// Last update time for animation
  last_update: f64,
  /// Whether temporal filtering is enabled (can be disabled by user even when temporal data exists)
  filter_enabled: bool,
}

impl TemporalControls {
  /// Get whether temporal filtering is enabled
  pub fn is_filter_enabled(&self) -> bool {
    self.filter_enabled
  }

  /// Set whether temporal filtering is enabled
  pub fn set_filter_enabled(&mut self, enabled: bool) {
    self.filter_enabled = enabled;

    // If disabling filter, clear current filter values
    if !enabled {
      self.current_time = None;
      self.time_window = None;
    } else if let (Some(start), Some(end)) = (self.time_start, self.time_end) {
      // If enabling filter and we have time data, restore default values
      if self.current_time.is_none() {
        self.current_time = Some(start);
      }
      if self.time_window.is_none() {
        let total_duration = end.signed_duration_since(start);
        self.time_window = Some(total_duration / 10);
      }
    }
  }

  /// Update the timeline during playback
  fn update_timeline(&mut self, current_ui_time: f64) {
    if !self.is_playing {
      return;
    }

    if let (Some(start), Some(end), Some(current)) =
      (self.time_start, self.time_end, self.current_time)
    {
      let dt = if self.last_update == 0.0 {
        0.016 // First frame, assume 60fps
      } else {
        (current_ui_time - self.last_update).min(0.1) // Cap at 100ms
      };

      let speed_factor = self.playback_speed * 60.0; // 60x faster than real time by default
      #[allow(clippy::cast_possible_truncation)]
      let time_advance =
        chrono::Duration::milliseconds((dt * 1000.0 * f64::from(speed_factor)) as i64);

      let new_time = current + time_advance;

      if new_time > end {
        // Loop back to start
        self.current_time = Some(start);
      } else {
        self.current_time = Some(new_time);
      }
    }

    self.last_update = current_ui_time;
  }

  /// Initialize time range from layer data, with demo fallback
  fn init_from_layers(&mut self, map_content: &dyn MapLayerHolder) {
    let mut earliest: Option<DateTime<Utc>> = None;
    let mut latest: Option<DateTime<Utc>> = None;

    // We need to access the actual geometries to extract temporal data
    // Since the layer system doesn't directly expose geometries, we'll use a different approach
    // For now, we'll extract temporal range through the shapelayer if possible

    // Scan for temporal data by accessing the layers
    let mut layer_reader = map_content.get_reader();
    for layer in layer_reader.get_layers() {
      // Get temporal range directly from the layer trait method
      let (layer_earliest, layer_latest) = layer.get_temporal_range();

      if let Some(layer_earliest) = layer_earliest {
        earliest = Some(earliest.map_or(layer_earliest, |e| e.min(layer_earliest)));
      }

      if let Some(layer_latest) = layer_latest {
        latest = Some(latest.map_or(layer_latest, |l| l.max(layer_latest)));
      }
    }

    // Only enable temporal filtering if we found actual temporal data
    if let (Some(start), Some(end)) = (earliest, latest) {
      self.time_start = Some(start);
      self.time_end = Some(end);

      // Only set default filter values if filtering is enabled
      if self.filter_enabled {
        if self.current_time.is_none() {
          self.current_time = Some(start);
        }

        // Initialize with a reasonable time window (10% of total range)
        if self.time_window.is_none() {
          let total_duration = end.signed_duration_since(start);
          self.time_window = Some(total_duration / 10);
        }
      } else {
        // Filter is disabled, clear filter values
        self.current_time = None;
        self.time_window = None;
      }
    } else {
      // No temporal data found - clear any existing temporal settings
      self.time_start = None;
      self.time_end = None;
      self.current_time = None;
      self.time_window = None;
    }
  }
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
      temporal_controls: TemporalControls {
        playback_speed: 1.0,
        filter_enabled: true, // Default to enabled when temporal data is available
        ..Default::default()
      },
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

    // Update temporal controls
    self.temporal_controls.update_timeline(current_time);
    if self.temporal_controls.is_playing {
      ctx.request_repaint();
    }

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

  /// Display performance monitoring
  fn show_performance_monitoring(ui: &mut egui::Ui, runtime_monitor: Option<&RuntimeMonitor>) {
    // Show runtime metrics if monitor is available
    if let Some(monitor) = runtime_monitor {
      let intervals: Vec<_> = monitor.intervals().take(1).collect();
      if let Some(metrics) = intervals.first() {
        ui.horizontal(|ui| {
          ui.label("Live tasks:");
          ui.label(
            egui::RichText::new(format!("{}", metrics.live_tasks_count))
              .strong()
              .color(egui::Color32::from_rgb(255, 140, 0))
          );
        });

        ui.horizontal(|ui| {
          ui.label("Parks/sec:");
          #[allow(clippy::cast_precision_loss)]
          let parks_per_sec = metrics.total_park_count as f64 / metrics.elapsed.as_secs_f64();
          ui.label(format!("{parks_per_sec:.0}"));
        });
      }
    } else {
      ui.label(
        egui::RichText::new("Runtime metrics unavailable")
          .italics()
          .color(egui::Color32::GRAY)
      );
    }

    ui.add_space(8.0);
    ui.separator();

    // Active tasks as subsection
    ui.label(egui::RichText::new("Active Tasks").strong().size(11.0));
    ui.add_space(4.0);

    Self::show_active_tasks(ui);
  }

  /// Display active task list
  fn show_active_tasks(ui: &mut egui::Ui) {
    let tracker = task_tracker();
    let tasks = tracker.snapshot();

    if tasks.is_empty() {
      ui.label(
        egui::RichText::new("No active tasks")
          .size(12.0)
          .italics()
          .color(egui::Color32::GRAY)
      );
      return;
    }

    // Sort by category for better organization
    let mut tasks_by_category = std::collections::HashMap::new();
    for (id, task) in tasks {
      tasks_by_category
        .entry(task.category.clone())
        .or_insert_with(Vec::new)
        .push((id, task));
    }

    // Display tasks by category in a specific order (server first, then others)
    let category_order = [
      TaskCategory::Server,
      TaskCategory::TileLoad,
      TaskCategory::TileSuperRes,
      TaskCategory::Search,
      TaskCategory::External,
      TaskCategory::Other,
    ];

    for category in category_order {
      if let Some(mut tasks) = tasks_by_category.remove(&category) {
        // Sort by elapsed time (newest first)
        tasks.sort_by_key(|(_, task)| std::cmp::Reverse(task.elapsed()));

        let category_name = match category {
          TaskCategory::TileLoad => "🗺 Tile Loading",
          TaskCategory::TileSuperRes => "🔍 Super Resolution",
          TaskCategory::Server => "🌐 Server",
          TaskCategory::Search => "🔍 Search",
          TaskCategory::External => "⚙ External",
          TaskCategory::Other => "📋 Other",
        };

        ui.group(|ui| {
          ui.label(egui::RichText::new(category_name).strong().size(11.0));
          ui.separator();

          if tasks.len() > 5 {
            // Show count if too many tasks
            ui.label(format!("{} active tasks", tasks.len()));
            for (_, task) in tasks.into_iter().take(3) {
              Self::show_task_info(ui, &task);
            }
            ui.label(
              egui::RichText::new("... and more")
                .size(10.0)
                .italics()
                .color(egui::Color32::GRAY)
            );
          } else {
            for (_, task) in tasks {
              Self::show_task_info(ui, &task);
            }
          }
        });

        ui.add_space(4.0);
      }
    }
  }

  /// Display individual task info
  fn show_task_info(ui: &mut egui::Ui, task: &crate::task_tracker::TaskInfo) {
    ui.horizontal(|ui| {
      ui.label(
        egui::RichText::new(&task.name)
          .size(10.0)
          .monospace()
          .color(egui::Color32::from_rgb(70, 70, 70))
      );

      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let elapsed = task.elapsed();
        let color = if elapsed.as_secs() > 10 {
          egui::Color32::from_rgb(200, 100, 0) // Orange for long-running
        } else if elapsed.as_secs() > 5 {
          egui::Color32::from_rgb(180, 180, 0) // Yellow for moderate
        } else {
          egui::Color32::from_rgb(100, 180, 100) // Green for recent
        };

        ui.label(
          egui::RichText::new(format!("{:.1}s", elapsed.as_secs_f32()))
            .size(9.0)
            .color(color)
        );
      });
    });
  }

  fn ui(&mut self, ui: &mut egui::Ui, runtime_monitor: Option<&RuntimeMonitor>) {
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

          // Timeline is now controlled through the Timeline layer in Map Layers section

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

          egui::CollapsingHeader::new("📊 Performance Monitoring")
            .default_open(false)
            .show(ui, |ui| {
              Self::show_performance_monitoring(ui, runtime_monitor);
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
  new_provider_tile_type: TileType,
  cache_directory: String,
  screenshot_path: String,
  settings_changed: bool,
  search_providers: Vec<SearchProviderConfig>,
  new_search_provider_name: String,
  new_search_provider_url: String,
  new_search_provider_headers: String,
  nominatim_base_url: String,
  // Style editing state
  style_config: StyleConfig,
  style_changed: bool,
}

#[derive(Clone, PartialEq)]
enum SettingsTab {
  TileProviders,
  SearchProviders,
  General,
  Styling,
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
      new_provider_tile_type: TileType::default(),
      cache_directory,
      screenshot_path,
      settings_changed: false,
      new_search_provider_name: String::new(),
      new_search_provider_url: String::new(),
      new_search_provider_headers: String::new(),
      nominatim_base_url: String::new(),
      style_config: style_config(),
      style_changed: false,
    }
  }

  /// Get the current config (this will include any unsaved changes made in the UI)
  fn get_current_config(&self) -> Config {
    self.config.clone()
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
          ui.selectable_value(&mut self.selected_tab, SettingsTab::Styling, "Styling");
        });

        ui.separator();

        egui::ScrollArea::vertical()
          .auto_shrink([false; 2])
          .show(ui, |ui| match self.selected_tab {
            SettingsTab::General => self.general_settings_ui(ui),
            SettingsTab::TileProviders => self.tile_providers_ui(ui),
            SettingsTab::SearchProviders => self.search_providers_ui(ui),
            SettingsTab::Styling => self.styling_ui(ui),
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
      ui.label("Heading Arrow Style:");
      ui.horizontal(|ui| {
        ui.label("Arrow style for points with heading:");
        egui::ComboBox::from_id_salt("heading_style")
          .selected_text(self.config.heading_style.name())
          .show_ui(ui, |ui| {
            for style in HeadingStyle::all() {
              if ui
                .selectable_value(&mut self.config.heading_style, *style, style.name())
                .clicked()
              {
                self.settings_changed = true;
              }
            }
          });
      });
      ui.small("Visual style for directional arrows on points with heading data");
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
          ui.label(format!("[{}]", provider.tile_type.name()));
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
      ui.horizontal(|ui| {
        ui.label("Type:");
        egui::ComboBox::from_id_salt("new_provider_tile_type")
          .selected_text(self.new_provider_tile_type.name())
          .show_ui(ui, |ui| {
            for tile_type in TileType::all() {
              ui.selectable_value(
                &mut self.new_provider_tile_type,
                *tile_type,
                tile_type.name(),
              );
            }
          });
      });
      ui.small("Use {x}, {y}, {zoom} as placeholders (e.g., https://tile.openstreetmap.org/{zoom}/{x}/{y}.png)");
      if ui.button("Add Provider").clicked() && !self.new_provider_name.is_empty() && !self.new_provider_url.is_empty() {
        self.tile_providers.push(TileProvider {
          name: self.new_provider_name.clone(),
          url: self.new_provider_url.clone(),
          tile_type: self.new_provider_tile_type,
          max_zoom: None,
        });
        self.new_provider_name.clear();
        self.new_provider_url.clear();
        self.new_provider_tile_type = TileType::default();
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

  #[allow(clippy::too_many_lines)]
  fn styling_ui(&mut self, ui: &mut egui::Ui) {
    ui.heading("Vector Tile Styling");
    ui.separator();

    ui.label("Configure colors and styling for vector tile rendering. Changes apply immediately.");
    ui.add_space(8.0);

    // Action buttons
    ui.horizontal(|ui| {
      if ui.button("Reset to Defaults").clicked() {
        self.style_config = StyleConfig::default();
        self.style_changed = true;
        set_style_config(self.style_config.clone());
      }

      if ui.button("Reload from File").clicked() {
        init_style_config(self.config.vector_style_file.as_deref());
        self.style_config = style_config();
        self.style_changed = false;
      }

      if self.style_changed {
        if ui.button("Save to File").clicked() {
          match save_style_config() {
            Ok(()) => {
              self.style_changed = false;
              log::info!("Style saved successfully");
            }
            Err(e) => log::error!("Failed to save style: {e}"),
          }
        }
        ui.label(egui::RichText::new("(unsaved changes)").italics().color(egui::Color32::YELLOW));
      }
    });

    ui.add_space(8.0);

    // Base colors section
    egui::CollapsingHeader::new("Base Colors")
      .default_open(true)
      .show(ui, |ui| {
        let mut changed = false;
        changed |= Self::color_edit(ui, "Background", &mut self.style_config.background);
        changed |= Self::color_edit(ui, "Water", &mut self.style_config.water);
        changed |= Self::color_edit(ui, "Land", &mut self.style_config.land);
        changed |= Self::color_edit(ui, "Park", &mut self.style_config.park);
        changed |= Self::color_edit(ui, "Building", &mut self.style_config.building);
        if changed {
          self.style_changed = true;
          set_style_config(self.style_config.clone());
        }
      });

    // Label colors section
    egui::CollapsingHeader::new("Label Colors")
      .default_open(false)
      .show(ui, |ui| {
        let mut changed = false;
        changed |= Self::color_edit(ui, "Place Labels", &mut self.style_config.place_label);
        changed |= Self::color_edit(ui, "Road Labels", &mut self.style_config.road_label);
        changed |= Self::color_edit(ui, "Water Labels", &mut self.style_config.water_label);
        changed |= Self::color_edit(ui, "Marker Dot", &mut self.style_config.marker_dot);
        if changed {
          self.style_changed = true;
          set_style_config(self.style_config.clone());
        }
      });

    // Road styles section
    egui::CollapsingHeader::new("Road Styles")
      .default_open(false)
      .show(ui, |ui| {
        let mut changed = false;
        changed |= Self::road_style_edit(ui, "Motorway", &mut self.style_config.roads.motorway);
        changed |= Self::road_style_edit(ui, "Trunk", &mut self.style_config.roads.trunk);
        changed |= Self::road_style_edit(ui, "Primary", &mut self.style_config.roads.primary);
        changed |= Self::road_style_edit(ui, "Secondary", &mut self.style_config.roads.secondary);
        changed |= Self::road_style_edit(ui, "Tertiary", &mut self.style_config.roads.tertiary);
        changed |= Self::road_style_edit(ui, "Residential", &mut self.style_config.roads.residential);
        changed |= Self::road_style_edit(ui, "Service", &mut self.style_config.roads.service);
        changed |= Self::road_style_edit(ui, "Path", &mut self.style_config.roads.path);
        if changed {
          self.style_changed = true;
          set_style_config(self.style_config.clone());
        }
      });

    // Rendering settings section
    egui::CollapsingHeader::new("Rendering Settings")
      .default_open(false)
      .show(ui, |ui| {
        ui.horizontal(|ui| {
          ui.label("Tile Size:");
          ui.label(format!("{}px", self.style_config.rendering.tile_size));
          ui.small("(restart required to change)");
        });
      });

    // Font sizes section
    egui::CollapsingHeader::new("Font Sizes")
      .default_open(false)
      .show(ui, |ui| {
        let mut changed = false;
        changed |= Self::float_slider(ui, "Country", &mut self.style_config.font_sizes.country, 8.0..=24.0);
        changed |= Self::float_slider(ui, "Capital", &mut self.style_config.font_sizes.capital, 8.0..=20.0);
        changed |= Self::float_slider(ui, "City", &mut self.style_config.font_sizes.city, 6.0..=18.0);
        changed |= Self::float_slider(ui, "Town", &mut self.style_config.font_sizes.town, 6.0..=16.0);
        changed |= Self::float_slider(ui, "Village", &mut self.style_config.font_sizes.village, 5.0..=14.0);
        changed |= Self::float_slider(ui, "Road Label", &mut self.style_config.font_sizes.road_label, 6.0..=16.0);
        changed |= Self::float_slider(ui, "Water Label", &mut self.style_config.font_sizes.water_label, 6.0..=16.0);
        if changed {
          self.style_changed = true;
          set_style_config(self.style_config.clone());
        }
      });
  }

  /// Interactive color editor - returns true if changed
  fn color_edit(ui: &mut egui::Ui, label: &str, rgb: &mut Rgb) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
      let mut color = [rgb.r, rgb.g, rgb.b];
      if ui.color_edit_button_srgb(&mut color).changed() {
        rgb.r = color[0];
        rgb.g = color[1];
        rgb.b = color[2];
        changed = true;
      }
      ui.label(label);
      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.monospace(rgb.to_hex());
      });
    });
    changed
  }

  /// Interactive road style editor - returns true if changed
  fn road_style_edit(ui: &mut egui::Ui, label: &str, style: &mut RoadStyle) -> bool {
    let mut changed = false;

    egui::CollapsingHeader::new(label)
      .default_open(false)
      .show(ui, |ui| {
        ui.horizontal(|ui| {
          let mut casing = [style.casing.r, style.casing.g, style.casing.b];
          if ui.color_edit_button_srgb(&mut casing).changed() {
            style.casing = Rgb::new(casing[0], casing[1], casing[2]);
            changed = true;
          }
          ui.label("Casing");

          let mut inner = [style.inner.r, style.inner.g, style.inner.b];
          if ui.color_edit_button_srgb(&mut inner).changed() {
            style.inner = Rgb::new(inner[0], inner[1], inner[2]);
            changed = true;
          }
          ui.label("Inner");
        });

        ui.horizontal(|ui| {
          ui.label("Casing width:");
          if ui.add(egui::DragValue::new(&mut style.casing_width).range(0.5..=20.0).speed(0.1)).changed() {
            changed = true;
          }
          ui.label("Inner width:");
          if ui.add(egui::DragValue::new(&mut style.inner_width).range(0.5..=15.0).speed(0.1)).changed() {
            changed = true;
          }
        });
      });

    changed
  }

  /// Float slider editor - returns true if changed
  fn float_slider(ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
      ui.label(format!("{label}:"));
      if ui.add(egui::Slider::new(value, range).step_by(0.5)).changed() {
        changed = true;
      }
    });
    changed
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
