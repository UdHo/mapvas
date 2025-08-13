use super::{Layer, LayerProperties};
use crate::map::coordinates::{BoundingBox, Transform};
use crate::map::mapvas_egui::timeline_widget::TimelineWidget;
use chrono::{DateTime, Utc};
use egui::{Rect, Ui};

/// Timeline overlay layer that displays temporal controls on the map
pub struct TimelineLayer {
  properties: LayerProperties,
  /// The timeline widget that handles all UI and interactions
  widget: TimelineWidget,
  /// Last update time for animation
  last_update: f64,
  /// Height of the timeline bar in pixels
  timeline_height: f32,
  /// Margin from screen edges
  margin: f32,
}

impl TimelineLayer {
  #[must_use]
  pub fn new() -> Self {
    Self {
      properties: LayerProperties { visible: true },
      widget: TimelineWidget::new(),
      last_update: 0.0,
      timeline_height: 75.0,
      margin: 20.0,
    }
  }

  /// Set the overall time range for the timeline
  pub fn set_time_range(&mut self, start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) {
    self.widget.set_time_range(start, end);

    // Initialize interval to show the full range initially
    if let (Some(start), Some(end)) = (start, end) {
      // Only set the initial interval if we don't have one already
      let (current_start, current_end) = self.widget.get_interval();
      if current_start.is_none() || current_end.is_none() {
        self.widget.set_interval(Some(start), Some(end));
      }
    }
  }

  /// Set the current time interval
  pub fn set_interval(&mut self, start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) {
    self.widget.set_interval(start, end);
  }

  /// Get the current time interval
  #[must_use]
  pub fn get_interval(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    self.widget.get_interval()
  }

  /// Start/stop playback
  pub fn set_playing(&mut self, playing: bool) {
    self.widget.set_playing(playing);
  }

  /// Set playback speed
  pub fn set_playback_speed(&mut self, speed: f32) {
    self.widget.set_playback_speed(speed);
  }

  /// Update animation during playback
  pub fn update_animation(&mut self, current_time: f64) {
    use crate::map::mapvas_egui::timeline_widget::IntervalLock;

    if !self.widget.is_playing() {
      self.last_update = current_time;
      return;
    }

    let (time_start, time_end) = self.widget.get_time_range();
    let (interval_start, interval_end) = self.widget.get_interval();
    let lock_state = self.widget.get_interval_lock();

    if let (Some(time_start), Some(time_end), Some(interval_start), Some(interval_end)) =
      (time_start, time_end, interval_start, interval_end)
    {
      let delta_time = current_time - self.last_update;
      self.last_update = current_time;

      // Calculate how much to advance the interval
      let total_duration = time_end.signed_duration_since(time_start);
      let interval_duration = interval_end.signed_duration_since(interval_start);

      // Advance by a fraction of the total duration per second, scaled by playback speed
      let advance_fraction = delta_time * f64::from(self.widget.get_playback_speed()) / 10.0; // 10 seconds to traverse full timeline at 1x speed
      #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
      let advance_duration = chrono::Duration::nanoseconds(
        (total_duration.num_nanoseconds().unwrap_or(0) as f64 * advance_fraction) as i64,
      );

      // Respect interval locks during playback
      let (new_start, new_end) = match lock_state {
        IntervalLock::Start => {
          // Start is locked, only move the end
          let new_end = interval_end + advance_duration;
          (interval_start, new_end)
        }
        IntervalLock::End => {
          // End is locked, only move the start
          let new_start = interval_start + advance_duration;
          (new_start, interval_end)
        }
        IntervalLock::None => {
          // No lock, move both handles to maintain interval size
          let new_start = interval_start + advance_duration;
          let new_end = interval_end + advance_duration;
          (new_start, new_end)
        }
      };

      // Check boundaries and stop/reset if needed
      let should_stop = match lock_state {
        IntervalLock::End => new_start < time_start,
        IntervalLock::None | IntervalLock::Start => new_end > time_end,
      };

      if should_stop {
        self.widget.set_playing(false);
        // Reset based on lock state
        match lock_state {
          IntervalLock::Start => {
            // Start locked, reset end to beginning of possible range
            self.widget.set_interval(
              Some(interval_start),
              Some(interval_start + interval_duration),
            );
          }
          IntervalLock::End => {
            // End locked, reset start to end of possible range
            self
              .widget
              .set_interval(Some(interval_end - interval_duration), Some(interval_end));
          }
          IntervalLock::None => {
            // No lock, reset to beginning
            self
              .widget
              .set_interval(Some(time_start), Some(time_start + interval_duration));
          }
        }
      } else {
        self.widget.set_interval(Some(new_start), Some(new_end));
      }
    }
  }

  /// Draw the timeline controls
  fn draw_timeline(&mut self, ui: &mut Ui, screen_rect: Rect) {
    // Position timeline at bottom with better margins - adjusted height for vertical layout
    let timeline_width = (screen_rect.width() * 0.9)
      .max(500.0)
      .min(screen_rect.width() - 20.0); // Wider
    let timeline_x = screen_rect.center().x - timeline_width / 2.0;

    let timeline_rect = Rect::from_min_size(
      egui::Pos2::new(
        timeline_x,
        screen_rect.max.y - self.timeline_height - self.margin,
      ),
      egui::Vec2::new(timeline_width, self.timeline_height),
    );

    // Delegate to the timeline widget
    self.widget.draw(ui, timeline_rect);
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
    let (time_start, time_end) = self.widget.get_time_range();
    if time_start.is_some() && time_end.is_some() {
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
    self.widget.get_time_range()
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
    (self.widget.is_playing(), self.widget.get_playback_speed())
  }

  fn toggle_timeline_interval_lock(&mut self) {
    self.widget.toggle_interval_lock();
  }

  fn get_timeline_interval_lock(&self) -> crate::map::mapvas_egui::timeline_widget::IntervalLock {
    self.widget.get_interval_lock()
  }

  fn ui_content(&mut self, ui: &mut Ui) {
    let (time_start, time_end) = self.widget.get_time_range();
    if time_start.is_none() || time_end.is_none() {
      ui.separator();
      ui.label("⚠ No timestamp data detected");
      return;
    }

    ui.separator();

    if let (Some(start), Some(end)) = (time_start, time_end) {
      ui.label(format!(
        "📅 Data Range: {} to {}",
        start.format("%Y-%m-%d %H:%M:%S"),
        end.format("%Y-%m-%d %H:%M:%S")
      ));
    }

    let (interval_start, interval_end) = self.widget.get_interval();
    if let (Some(interval_start), Some(interval_end)) = (interval_start, interval_end) {
      ui.label(format!(
        "🎯 Current Filter: {} to {}",
        interval_start.format("%Y-%m-%d %H:%M:%S"),
        interval_end.format("%Y-%m-%d %H:%M:%S")
      ));
    }

    ui.separator();

    ui.horizontal(|ui| {
      ui.label("⚡ Playback Speed:");
      let mut speed = self.widget.get_playback_speed();
      if ui
        .add(
          egui::Slider::new(&mut speed, 0.1..=10.0)
            .text("x")
            .logarithmic(true),
        )
        .changed()
      {
        self.widget.set_playback_speed(speed);
      }
    });

    ui.horizontal(|ui| {
      ui.label("▶ Playing:");
      ui.label(if self.widget.is_playing() {
        "Yes"
      } else {
        "No"
      });
    });

    ui.separator();

    ui.horizontal(|ui| {
      ui.label("🔒 Interval Lock:");
      let lock_state = self.widget.get_interval_lock();
      let lock_text = match lock_state {
        crate::map::mapvas_egui::timeline_widget::IntervalLock::None => "None",
        crate::map::mapvas_egui::timeline_widget::IntervalLock::Start => "Start Locked",
        crate::map::mapvas_egui::timeline_widget::IntervalLock::End => "End Locked",
      };
      ui.label(lock_text);

      if ui.button("Toggle Lock (Ctrl+L)").clicked() {
        self.widget.toggle_interval_lock();
      }
    });
  }
}
