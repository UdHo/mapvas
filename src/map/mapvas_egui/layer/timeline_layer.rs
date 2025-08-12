use super::{Layer, LayerProperties};
use crate::map::coordinates::{BoundingBox, Transform};
use chrono::{DateTime, Duration, Utc};
use egui::{Color32, FontId, Pos2, Rect, Sense, Ui, Vec2};

/// Timeline overlay layer that displays temporal controls on the map
pub struct TimelineLayer {
  properties: LayerProperties,
  /// Start of the time range
  time_start: Option<DateTime<Utc>>,
  /// End of the time range
  time_end: Option<DateTime<Utc>>,
  /// Current time interval start
  interval_start: Option<DateTime<Utc>>,
  /// Current time interval end
  interval_end: Option<DateTime<Utc>>,
  /// Whether the timeline is currently playing
  is_playing: bool,
  /// Playback speed multiplier
  playback_speed: f32,
  /// Last update time for animation
  last_update: f64,
  /// Height of the timeline bar in pixels
  timeline_height: f32,
  /// Margin from screen edges
  margin: f32,
  /// Whether user is dragging the start handle
  dragging_start: bool,
  /// Whether user is dragging the end handle
  dragging_end: bool,
  /// Whether user is dragging the entire interval
  dragging_interval: bool,
  /// Original interval start when drag began
  drag_start_interval_start: Option<DateTime<Utc>>,
  /// Original interval end when drag began  
  drag_start_interval_end: Option<DateTime<Utc>>,
  /// Mouse position when drag started
  drag_start_mouse_pos: Option<egui::Pos2>,
}

impl TimelineLayer {
  #[must_use]
  pub fn new() -> Self {
    Self {
      properties: LayerProperties { visible: true },
      time_start: None,
      time_end: None,
      interval_start: None,
      interval_end: None,
      is_playing: false,
      playback_speed: 1.0,
      last_update: 0.0,
      timeline_height: 60.0,
      margin: 20.0,
      dragging_start: false,
      dragging_end: false,
      dragging_interval: false,
      drag_start_interval_start: None,
      drag_start_interval_end: None,
      drag_start_mouse_pos: None,
    }
  }

  /// Set the overall time range for the timeline
  pub fn set_time_range(&mut self, start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) {
    self.time_start = start;
    self.time_end = end;

    // Initialize interval to show the full range initially
    if let (Some(start), Some(end)) = (start, end) {
      // Only set the initial interval if we don't have one already
      if self.interval_start.is_none() || self.interval_end.is_none() {
        self.interval_start = Some(start);
        self.interval_end = Some(end);
      }
    }
  }

  /// Set the current time interval
  pub fn set_interval(&mut self, start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) {
    self.interval_start = start;
    self.interval_end = end;
  }

  /// Get the current time interval
  #[must_use]
  pub fn get_interval(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    (self.interval_start, self.interval_end)
  }

  /// Start/stop playback
  pub fn set_playing(&mut self, playing: bool) {
    self.is_playing = playing;
  }

  /// Set playback speed
  pub fn set_playback_speed(&mut self, speed: f32) {
    self.playback_speed = speed;
  }

  /// Update animation during playback
  pub fn update_animation(&mut self, current_time: f64) {
    if !self.is_playing {
      self.last_update = current_time;
      return;
    }

    if let (Some(time_start), Some(time_end), Some(interval_start), Some(interval_end)) = (
      self.time_start,
      self.time_end,
      self.interval_start,
      self.interval_end,
    ) {
      let delta_time = current_time - self.last_update;
      self.last_update = current_time;

      // Calculate how much to advance the interval
      let total_duration = time_end.signed_duration_since(time_start);
      let interval_duration = interval_end.signed_duration_since(interval_start);

      // Advance by a fraction of the total duration per second, scaled by playback speed
      let advance_fraction = delta_time * f64::from(self.playback_speed) / 10.0; // 10 seconds to traverse full timeline at 1x speed
      #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
      let advance_duration = Duration::nanoseconds(
        (total_duration.num_nanoseconds().unwrap_or(0) as f64 * advance_fraction) as i64,
      );

      let new_start = interval_start + advance_duration;
      let new_end = interval_end + advance_duration;

      // Stop at the end or wrap around
      if new_end > time_end {
        self.is_playing = false;
        // Reset to beginning
        self.interval_start = Some(time_start);
        self.interval_end = Some(time_start + interval_duration);
      } else {
        self.interval_start = Some(new_start);
        self.interval_end = Some(new_end);
      }
    }
  }

  /// Convert time to timeline position (0.0 to 1.0)
  fn time_to_position(&self, time: DateTime<Utc>) -> Option<f32> {
    if let (Some(start), Some(end)) = (self.time_start, self.time_end) {
      let total_duration = end.signed_duration_since(start);
      let time_offset = time.signed_duration_since(start);

      if total_duration.num_milliseconds() > 0 {
        #[allow(clippy::cast_precision_loss)]
        Some((time_offset.num_milliseconds() as f32) / (total_duration.num_milliseconds() as f32))
      } else {
        Some(0.0)
      }
    } else {
      None
    }
  }

  /// Convert timeline position (0.0 to 1.0) to time
  fn position_to_time(&self, position: f32) -> Option<DateTime<Utc>> {
    if let (Some(start), Some(end)) = (self.time_start, self.time_end) {
      let total_duration = end.signed_duration_since(start);
      #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
      let offset_duration = Duration::nanoseconds(
        (total_duration.num_nanoseconds().unwrap_or(0) as f32 * position) as i64,
      );
      Some(start + offset_duration)
    } else {
      None
    }
  }

  /// Format datetime intelligently based on the time span
  fn format_time(&self, time: DateTime<Utc>, is_detailed: bool) -> String {
    if let (Some(start), Some(end)) = (self.time_start, self.time_end) {
      let total_duration = end.signed_duration_since(start);

      if total_duration.num_days() > 365 {
        // Span over a year - show year
        if is_detailed {
          time.format("%Y-%m-%d %H:%M").to_string()
        } else {
          time.format("%Y-%m").to_string()
        }
      } else if total_duration.num_days() > 30 {
        // Span over a month - show month and day
        if is_detailed {
          time.format("%m-%d %H:%M").to_string()
        } else {
          time.format("%m-%d").to_string()
        }
      } else if total_duration.num_days() > 1 {
        // Span multiple days - show month, day and time
        if is_detailed {
          time.format("%m/%d %H:%M:%S").to_string()
        } else {
          time.format("%m/%d %H:%M").to_string()
        }
      } else {
        // Same day - show time only
        if is_detailed {
          time.format("%H:%M:%S").to_string()
        } else {
          time.format("%H:%M").to_string()
        }
      }
    } else {
      // Fallback format
      if is_detailed {
        time.format("%Y-%m-%d %H:%M:%S").to_string()
      } else {
        time.format("%Y-%m-%d").to_string()
      }
    }
  }

  /// Draw the timeline controls
  #[allow(clippy::too_many_lines)]
  fn draw_timeline(&mut self, ui: &mut Ui, screen_rect: Rect) {
    // Position timeline at bottom with better margins - adjusted height for vertical layout
    let timeline_width = (screen_rect.width() * 0.9).max(500.0).min(screen_rect.width() - 20.0); // Wider
    let timeline_x = screen_rect.center().x - timeline_width / 2.0;
    let timeline_height = 75.0; // Increased height for proper spacing
    
    let timeline_rect = Rect::from_min_size(
      Pos2::new(timeline_x, screen_rect.max.y - timeline_height - 20.0),
      Vec2::new(timeline_width, timeline_height),
    );

    // Better background with rounded corners and subtle shadow
    let bg_color = Color32::from_rgba_unmultiplied(0, 0, 0, 140);
    ui.painter().rect_filled(timeline_rect, 8.0, bg_color);
    
    // Add subtle border
    ui.painter().rect_stroke(
      timeline_rect, 
      8.0, 
      egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 60)),
      egui::epaint::StrokeKind::Outside
    );

    // Timeline track - centered within container for balanced spacing
    let track_margin = 30.0; // Larger margin so numbers stay inside container
    let track_y_offset = 30.0; // Balanced spacing from top and bottom
    let track_rect = Rect::from_min_size(
      timeline_rect.min + Vec2::new(track_margin, track_y_offset),
      Vec2::new(timeline_rect.width() - 2.0 * track_margin, 10.0),
    );

    // Draw the full timeline track with better styling
    ui.painter().rect_filled(track_rect, 5.0, Color32::from_gray(40));
    ui.painter().rect_stroke(track_rect, 5.0, egui::Stroke::new(1.0, Color32::from_gray(80)), egui::epaint::StrokeKind::Outside);

    // Control buttons - positioned below the track and centered
    let button_size = Vec2::new(24.0, 20.0); // Smaller buttons
    let button_spacing = 6.0; // Closer spacing for smaller buttons
    let total_button_width = button_size.x * 2.0 + button_spacing; // Two buttons plus spacing
    let buttons_start_x = timeline_rect.center().x - total_button_width / 2.0; // Center the buttons
    let button_y = track_rect.max.y + 6.0; // Position closer to track to save space

    // Draw time labels if we have time data - positioned below buttons
    if let (Some(start), Some(end)) = (self.time_start, self.time_end) {
      let start_text = self.format_time(start, false);
      let end_text = self.format_time(end, false);

      // Position labels better with background
      let label_bg = Color32::from_rgba_unmultiplied(0, 0, 0, 100);
      let label_padding = 4.0;
      
      // Start label - positioned at same height as buttons
      let start_label_pos = Pos2::new(track_rect.min.x, button_y + button_size.y / 2.0); // Center with buttons
      let start_text_size = ui.painter().layout_no_wrap(
        start_text.clone(), 
        FontId::proportional(10.0), // Smaller font
        Color32::WHITE
      ).size();
      ui.painter().rect_filled(
        Rect::from_min_size(
          start_label_pos - Vec2::new(label_padding, start_text_size.y / 2.0 + label_padding),
          start_text_size + Vec2::splat(label_padding * 2.0)
        ),
        3.0,
        label_bg
      );
      ui.painter().text(
        start_label_pos,
        egui::Align2::LEFT_CENTER,
        start_text,
        FontId::proportional(10.0),
        Color32::WHITE,
      );

      // End label - positioned at same height as buttons
      let end_label_pos = Pos2::new(track_rect.max.x, button_y + button_size.y / 2.0); // Center with buttons
      let end_text_size = ui.painter().layout_no_wrap(
        end_text.clone(), 
        FontId::proportional(10.0), // Smaller font
        Color32::WHITE
      ).size();
      ui.painter().rect_filled(
        Rect::from_min_size(
          end_label_pos - Vec2::new(end_text_size.x + label_padding, end_text_size.y / 2.0 + label_padding),
          end_text_size + Vec2::splat(label_padding * 2.0)
        ),
        3.0,
        label_bg
      );
      ui.painter().text(
        end_label_pos,
        egui::Align2::RIGHT_CENTER,
        end_text,
        FontId::proportional(10.0),
        Color32::WHITE,
      );
    }

    // Draw interval and handles if we have interval data
    if let (Some(interval_start), Some(interval_end)) = (self.interval_start, self.interval_end) {
      if let (Some(start_pos), Some(end_pos)) = (
        self.time_to_position(interval_start),
        self.time_to_position(interval_end),
      ) {
        let interval_start_x = track_rect.min.x + start_pos * track_rect.width();
        let interval_end_x = track_rect.min.x + end_pos * track_rect.width();

        // Draw interval highlight
        let interval_rect = Rect::from_min_max(
          Pos2::new(interval_start_x, track_rect.min.y),
          Pos2::new(interval_end_x, track_rect.max.y),
        );

        // Handle interval dragging (drag the entire interval)
        let interval_drag_response = ui.allocate_rect(interval_rect, Sense::drag());

        let interval_color = if self.dragging_interval || interval_drag_response.hovered() {
          Color32::from_rgba_unmultiplied(120, 170, 255, 200) // Slightly brighter when dragging/hovering
        } else {
          Color32::from_rgba_unmultiplied(100, 150, 255, 180) // Normal color
        };

        ui.painter().rect_filled(interval_rect, 4.0, interval_color);

        if interval_drag_response.drag_started() {
          self.dragging_interval = true;
          self.drag_start_interval_start = self.interval_start;
          self.drag_start_interval_end = self.interval_end;
          self.drag_start_mouse_pos = ui.input(|i| i.pointer.hover_pos());
        } else if interval_drag_response.dragged() && self.dragging_interval {
          if let (Some(orig_start), Some(orig_end), Some(start_mouse_pos)) = (
            self.drag_start_interval_start,
            self.drag_start_interval_end,
            self.drag_start_mouse_pos,
          ) {
            if let Some(current_mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
              // Calculate the mouse movement delta since drag started
              let mouse_delta_x = current_mouse_pos.x - start_mouse_pos.x;
              let position_delta = mouse_delta_x / track_rect.width();

              // Convert position delta to time delta
              if let (Some(timeline_start), Some(timeline_end)) = (self.time_start, self.time_end) {
                let timeline_duration = timeline_end.signed_duration_since(timeline_start);
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                let time_delta = Duration::nanoseconds(
                  (timeline_duration.num_nanoseconds().unwrap_or(0) as f32 * position_delta) as i64,
                );

                let new_start = orig_start + time_delta;
                let new_end = orig_end + time_delta;

                // Ensure the interval doesn't go outside the timeline bounds
                if new_start >= timeline_start && new_end <= timeline_end {
                  self.interval_start = Some(new_start);
                  self.interval_end = Some(new_end);
                } else if new_start < timeline_start {
                  // Clamp to start of timeline
                  let interval_duration = orig_end.signed_duration_since(orig_start);
                  self.interval_start = Some(timeline_start);
                  self.interval_end = Some(timeline_start + interval_duration);
                } else if new_end > timeline_end {
                  // Clamp to end of timeline
                  let interval_duration = orig_end.signed_duration_since(orig_start);
                  self.interval_end = Some(timeline_end);
                  self.interval_start = Some(timeline_end - interval_duration);
                }
              }
            }
          }
        } else if interval_drag_response.drag_stopped() {
          self.dragging_interval = false;
          self.drag_start_interval_start = None;
          self.drag_start_interval_end = None;
          self.drag_start_mouse_pos = None;
        }

        // Draw handles
        let handle_size = 12.0;
        let handle_color = Color32::from_rgb(255, 255, 255);

        // Start handle
        let start_handle_rect = Rect::from_center_size(
          Pos2::new(interval_start_x, track_rect.center().y),
          Vec2::splat(handle_size),
        );

        let start_handle_response = ui.allocate_rect(start_handle_rect, Sense::drag());

        if start_handle_response.dragged() && !self.dragging_interval {
          self.dragging_start = true;
          if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
            let new_pos = ((mouse_pos.x - track_rect.min.x) / track_rect.width()).clamp(0.0, 1.0);
            if let Some(new_time) = self.position_to_time(new_pos) {
              // Ensure start doesn't go past end
              if let Some(interval_end) = self.interval_end {
                if new_time < interval_end {
                  self.interval_start = Some(new_time);
                }
              }
            }
          }
        } else if start_handle_response.drag_stopped() {
          self.dragging_start = false;
        }

        ui.painter().circle_filled(
          start_handle_rect.center(),
          handle_size / 2.0,
          if start_handle_response.hovered() || self.dragging_start {
            Color32::from_rgb(255, 255, 150)
          } else {
            handle_color
          },
        );

        // End handle
        let end_handle_rect = Rect::from_center_size(
          Pos2::new(interval_end_x, track_rect.center().y),
          Vec2::splat(handle_size),
        );

        let end_handle_response = ui.allocate_rect(end_handle_rect, Sense::drag());

        if end_handle_response.dragged() && !self.dragging_interval {
          self.dragging_end = true;
          if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
            let new_pos = ((mouse_pos.x - track_rect.min.x) / track_rect.width()).clamp(0.0, 1.0);
            if let Some(new_time) = self.position_to_time(new_pos) {
              // Ensure end doesn't go before start
              if let Some(interval_start) = self.interval_start {
                if new_time > interval_start {
                  self.interval_end = Some(new_time);
                }
              }
            }
          }
        } else if end_handle_response.drag_stopped() {
          self.dragging_end = false;
        }

        ui.painter().circle_filled(
          end_handle_rect.center(),
          handle_size / 2.0,
          if end_handle_response.hovered() || self.dragging_end {
            Color32::from_rgb(255, 255, 150)
          } else {
            handle_color
          },
        );

        // Draw current interval time labels with better positioning
        let interval_start_text = self.format_time(interval_start, true);
        let interval_end_text = self.format_time(interval_end, true);

        // Only show interval labels if they don't overlap with handles
        let min_label_distance = 40.0; // Reduced minimum distance
        if (interval_end_x - interval_start_x) > min_label_distance {
          // Background for interval labels - more opaque blue
          let label_bg = Color32::from_rgba_unmultiplied(100, 150, 255, 220); // More opaque blue
          let label_padding = 4.0; // Increased padding
          
          // Start interval label - positioned above track with enough clearance
          let start_text_size = ui.painter().layout_no_wrap(
            interval_start_text.clone(), 
            FontId::proportional(10.0), 
            Color32::WHITE
          ).size();
          let start_label_pos = Pos2::new(interval_start_x, track_rect.min.y - 15.0); // Larger distance from track
          ui.painter().rect_filled(
            Rect::from_center_size(start_label_pos, start_text_size + Vec2::splat(label_padding * 2.0)),
            2.0,
            label_bg
          );
          ui.painter().text(
            start_label_pos,
            egui::Align2::CENTER_CENTER, // Changed from CENTER_BOTTOM to CENTER_CENTER
            interval_start_text,
            FontId::proportional(10.0),
            Color32::WHITE,
          );

          // End interval label - positioned above track with enough clearance
          let end_text_size = ui.painter().layout_no_wrap(
            interval_end_text.clone(), 
            FontId::proportional(10.0), 
            Color32::WHITE
          ).size();
          let end_label_pos = Pos2::new(interval_end_x, track_rect.min.y - 15.0); // Larger distance from track
          ui.painter().rect_filled(
            Rect::from_center_size(end_label_pos, end_text_size + Vec2::splat(label_padding * 2.0)),
            2.0,
            label_bg
          );
          ui.painter().text(
            end_label_pos,
            egui::Align2::CENTER_CENTER, // Changed from CENTER_BOTTOM to CENTER_CENTER
            interval_end_text,
            FontId::proportional(10.0),
            Color32::WHITE,
          );
        }
      }
    }

    // Play/Pause button with better styling
    let play_button_rect = Rect::from_min_size(
      Pos2::new(buttons_start_x, button_y), 
      button_size
    );

    let play_response = ui.allocate_rect(play_button_rect, Sense::click());
    if play_response.clicked() {
      self.is_playing = !self.is_playing;
    }

    // Button background
    let play_bg_color = if play_response.hovered() {
      Color32::from_rgba_unmultiplied(255, 255, 255, 40)
    } else {
      Color32::from_rgba_unmultiplied(255, 255, 255, 20)
    };
    ui.painter().rect_filled(play_button_rect, 4.0, play_bg_color);
    ui.painter().rect_stroke(
      play_button_rect, 
      4.0, 
      egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 80)),
      egui::epaint::StrokeKind::Outside
    );

    let play_color = if play_response.hovered() {
      Color32::WHITE
    } else {
      Color32::from_gray(220)
    };

    let play_text = if self.is_playing { "⏸" } else { "▶" };
    ui.painter().text(
      play_button_rect.center(),
      egui::Align2::CENTER_CENTER,
      play_text,
      FontId::proportional(14.0), // Smaller font for smaller buttons
      play_color,
    );

    // Stop button
    let stop_button_rect = Rect::from_min_size(
      Pos2::new(buttons_start_x + button_size.x + button_spacing, button_y), 
      button_size
    );

    let stop_response = ui.allocate_rect(stop_button_rect, Sense::click());
    if stop_response.clicked() {
      self.is_playing = false;
      // Reset interval to start
      if let (Some(time_start), Some(interval_start), Some(interval_end)) =
        (self.time_start, self.interval_start, self.interval_end)
      {
        let interval_duration = interval_end.signed_duration_since(interval_start);
        self.interval_start = Some(time_start);
        self.interval_end = Some(time_start + interval_duration);
      }
    }

    // Button background
    let stop_bg_color = if stop_response.hovered() {
      Color32::from_rgba_unmultiplied(255, 255, 255, 40)
    } else {
      Color32::from_rgba_unmultiplied(255, 255, 255, 20)
    };
    ui.painter().rect_filled(stop_button_rect, 4.0, stop_bg_color);
    ui.painter().rect_stroke(
      stop_button_rect, 
      4.0, 
      egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 80)),
      egui::epaint::StrokeKind::Outside
    );

    let stop_color = if stop_response.hovered() {
      Color32::WHITE
    } else {
      Color32::from_gray(220)
    };

    ui.painter().text(
      stop_button_rect.center(),
      egui::Align2::CENTER_CENTER,
      "⏹",
      FontId::proportional(14.0), // Smaller font for smaller buttons
      stop_color,
    );
  }
}

impl Default for TimelineLayer {
  fn default() -> Self {
    Self::new()
  }
}

impl Layer for TimelineLayer {
  fn draw(&mut self, ui: &mut Ui, _transform: &Transform, rect: Rect) {
    if !self.properties.visible {
      return;
    }

    // Update animation
    let current_time = ui.input(|i| i.time);
    self.update_animation(current_time);

    // Only draw if we have time data
    if self.time_start.is_some() && self.time_end.is_some() {
      self.draw_timeline(ui, rect);
    }
  }

  fn name(&self) -> &'static str {
    "Timeline"
  }

  fn visible(&self) -> bool {
    self.properties.visible
  }

  fn set_visible(&mut self, visible: bool) {
    self.properties.visible = visible;
  }

  fn visible_mut(&mut self) -> &mut bool {
    &mut self.properties.visible
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    None // Timeline is a UI overlay, not geographical
  }

  fn get_temporal_range(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    (self.time_start, self.time_end)
  }

  fn update_timeline(
    &mut self,
    time_range: (
      Option<chrono::DateTime<chrono::Utc>>,
      Option<chrono::DateTime<chrono::Utc>>,
    ),
    current_interval: (
      Option<chrono::DateTime<chrono::Utc>>,
      Option<chrono::DateTime<chrono::Utc>>,
    ),
    is_playing: bool,
    playback_speed: f32,
  ) {
    self.set_time_range(time_range.0, time_range.1);
    self.set_interval(current_interval.0, current_interval.1);
    self.set_playing(is_playing);
    self.set_playback_speed(playback_speed);
  }

  fn get_timeline_interval(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  ) {
    self.get_interval()
  }

  fn get_timeline_playback_state(&self) -> (bool, f32) {
    (self.is_playing, self.playback_speed)
  }

  fn ui_content(&mut self, ui: &mut Ui) {
    if self.time_start.is_none() || self.time_end.is_none() {
      ui.separator();
      ui.label("⚠ No timestamp data detected");
      return;
    }

    ui.separator();

    if let (Some(start), Some(end)) = (self.time_start, self.time_end) {
      ui.label(format!(
        "📅 Data Range: {} to {}",
        self.format_time(start, true),
        self.format_time(end, true)
      ));
    }

    if let (Some(interval_start), Some(interval_end)) = (self.interval_start, self.interval_end) {
      ui.label(format!(
        "🎯 Current Filter: {} to {}",
        self.format_time(interval_start, true),
        self.format_time(interval_end, true)
      ));
    }

    ui.separator();

    ui.horizontal(|ui| {
      ui.label("⚡ Playback Speed:");
      ui.add(
        egui::Slider::new(&mut self.playback_speed, 0.1..=10.0)
          .text("x")
          .logarithmic(true),
      );
    });

    ui.horizontal(|ui| {
      ui.label("▶ Playing:");
      ui.label(if self.is_playing { "Yes" } else { "No" });
    });
  }
}

