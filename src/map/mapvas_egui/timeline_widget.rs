use chrono::{DateTime, Duration, Utc};
use egui::{Color32, FontId, Pos2, Rect, Sense, Ui, Vec2};

// Color helper functions for timeline appearance
fn timeline_bg_color() -> Color32 {
  Color32::from_rgba_unmultiplied(0, 0, 0, 140)
}
fn timeline_border_color() -> Color32 {
  Color32::from_rgba_unmultiplied(255, 255, 255, 60)
}
fn track_color() -> Color32 {
  Color32::from_gray(40)
}
fn track_border_color() -> Color32 {
  Color32::from_gray(80)
}

fn button_normal_color() -> Color32 {
  Color32::from_rgba_unmultiplied(255, 255, 255, 20)
}
fn button_hover_color() -> Color32 {
  Color32::from_rgba_unmultiplied(255, 255, 255, 40)
}
fn button_border_color() -> Color32 {
  Color32::from_rgba_unmultiplied(255, 255, 255, 80)
}

fn handle_normal_color() -> Color32 {
  Color32::from_rgb(255, 255, 255)
}
fn handle_hover_color() -> Color32 {
  Color32::from_rgb(255, 255, 150)
}

fn interval_normal_color() -> Color32 {
  Color32::from_rgba_unmultiplied(100, 150, 255, 100)
}
fn interval_hover_color() -> Color32 {
  Color32::from_rgba_unmultiplied(110, 160, 255, 150)
}
fn interval_drag_color() -> Color32 {
  Color32::from_rgba_unmultiplied(120, 170, 255, 200)
}

fn label_bg_color() -> Color32 {
  Color32::from_rgba_unmultiplied(0, 0, 0, 100)
}
fn interval_label_bg_color() -> Color32 {
  Color32::from_rgba_unmultiplied(100, 150, 255, 220)
}

// Layout constants
const TRACK_MARGIN: f32 = 30.0;
const TRACK_Y_OFFSET: f32 = 30.0;
const TRACK_HEIGHT: f32 = 10.0;
const BUTTON_SPACING: f32 = 8.0;
const SLIDER_WIDTH: f32 = 60.0;
const BUTTON_WIDTH: f32 = 24.0;
const BUTTON_HEIGHT: f32 = 20.0;
const STEP_BUTTON_SIZE: f32 = 20.0;
const HANDLE_SIZE: f32 = 12.0;
const LABEL_PADDING: f32 = 4.0;
const MIN_LABEL_DISTANCE: f32 = 40.0;

// Behavior constants
const MIN_SPEED: f32 = 0.25;
const MAX_SPEED: f32 = 4.0;
const MIN_STEP_SIZE: f32 = 0.1;
const MAX_STEP_SIZE: f32 = 86400.0; // 1 day in seconds

/// Lock state for interval boundaries
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntervalLock {
  /// No locks - both handles can move freely
  None,
  /// Start time is locked - only end time can move
  Start,
  /// End time is locked - only start time can move  
  End,
}

/// Playback state for timeline controls
#[derive(Debug, Clone)]
struct PlaybackState {
  /// Whether the timeline is currently playing
  is_playing: bool,
  /// Playback speed multiplier
  speed: f32,
  /// Step size for manual stepping (in seconds)
  step_size: f32,
}

impl Default for PlaybackState {
  fn default() -> Self {
    Self {
      is_playing: false,
      speed: 1.0,
      step_size: 60.0, // Default 1 minute step
    }
  }
}

/// Time range and interval state
#[derive(Debug, Clone)]
struct TimelineState {
  /// Start of the overall time range
  time_start: Option<DateTime<Utc>>,
  /// End of the overall time range  
  time_end: Option<DateTime<Utc>>,
  /// Current time interval start
  interval_start: Option<DateTime<Utc>>,
  /// Current time interval end
  interval_end: Option<DateTime<Utc>>,
  /// Lock state for interval boundaries
  interval_lock: IntervalLock,
}

impl Default for TimelineState {
  fn default() -> Self {
    Self {
      time_start: None,
      time_end: None,
      interval_start: None,
      interval_end: None,
      interval_lock: IntervalLock::None,
    }
  }
}

/// Drag state for timeline interactions
#[derive(Debug, Clone, Default)]
struct DragState {
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

/// Timeline control widget with playback controls and sliders
pub struct TimelineWidget {
  /// Timeline state (time ranges and intervals)
  state: TimelineState,
  /// Playback controls state
  playback: PlaybackState,
  /// Drag interaction state
  drag: DragState,
}

impl Default for TimelineWidget {
  fn default() -> Self {
    Self::new()
  }
}

impl TimelineWidget {
  /// Create a new timeline widget
  #[must_use]
  pub fn new() -> Self {
    Self {
      state: TimelineState::default(),
      playback: PlaybackState::default(),
      drag: DragState::default(),
    }
  }

  /// Set the overall time range
  pub fn set_time_range(&mut self, start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) {
    self.state.time_start = start;
    self.state.time_end = end;
  }

  /// Get the overall time range
  #[must_use]
  pub fn get_time_range(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    (self.state.time_start, self.state.time_end)
  }

  /// Set the current interval
  pub fn set_interval(&mut self, start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) {
    self.state.interval_start = start;
    self.state.interval_end = end;
  }

  /// Get the current interval
  #[must_use]
  pub fn get_interval(&self) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    (self.state.interval_start, self.state.interval_end)
  }

  /// Set playback state
  pub fn set_playing(&mut self, playing: bool) {
    self.playback.is_playing = playing;
  }

  /// Get playback state
  #[must_use]
  pub fn is_playing(&self) -> bool {
    self.playback.is_playing
  }

  /// Set playback speed
  pub fn set_playback_speed(&mut self, speed: f32) {
    self.playback.speed = speed.clamp(MIN_SPEED, MAX_SPEED);
  }

  /// Get playback speed
  #[must_use]
  pub fn get_playback_speed(&self) -> f32 {
    self.playback.speed
  }

  /// Set step size in seconds
  pub fn set_step_size(&mut self, step_size: f32) {
    self.playback.step_size = step_size.clamp(MIN_STEP_SIZE, MAX_STEP_SIZE);
  }

  /// Get step size in seconds
  #[must_use]
  pub fn get_step_size(&self) -> f32 {
    self.playback.step_size
  }

  /// Set interval lock state
  pub fn set_interval_lock(&mut self, lock: IntervalLock) {
    self.state.interval_lock = lock;
  }

  /// Get interval lock state
  #[must_use]
  pub fn get_interval_lock(&self) -> IntervalLock {
    self.state.interval_lock
  }

  /// Toggle interval lock between None -> Start -> End -> None
  pub fn toggle_interval_lock(&mut self) {
    self.state.interval_lock = match self.state.interval_lock {
      IntervalLock::None => IntervalLock::Start,
      IntervalLock::Start => IntervalLock::End,
      IntervalLock::End => IntervalLock::None,
    };
  }

  /// Calculate step duration from current step size
  fn calculate_step_duration(&self) -> Duration {
    if self.playback.step_size < 1.0 {
      #[allow(clippy::cast_possible_truncation)]
      let millis = (self.playback.step_size * 1000.0) as i64;
      Duration::milliseconds(millis)
    } else {
      #[allow(clippy::cast_possible_truncation)]
      let seconds = self.playback.step_size as i64;
      #[allow(clippy::cast_possible_truncation)]
      let millis = ((self.playback.step_size % 1.0) * 1000.0) as i64;
      Duration::seconds(seconds) + Duration::milliseconds(millis)
    }
  }

  /// Step the interval forward by `step_size`
  pub fn step_forward(&mut self) {
    if let (Some(start), Some(end)) = (self.state.interval_start, self.state.interval_end) {
      let step_duration = self.calculate_step_duration();

      match self.state.interval_lock {
        IntervalLock::None => {
          // Normal behavior - move entire interval
          let interval_duration = end.signed_duration_since(start);
          let new_start = start + step_duration;
          let new_end = new_start + interval_duration;

          // Don't step beyond the total time range
          if let Some(max_end) = self.state.time_end
            && new_end <= max_end
          {
            self.state.interval_start = Some(new_start);
            self.state.interval_end = Some(new_end);
          }
        }
        IntervalLock::Start => {
          // Start is locked - only extend end
          let new_end = end + step_duration;
          if let Some(max_end) = self.state.time_end
            && new_end <= max_end
          {
            self.state.interval_end = Some(new_end);
          }
        }
        IntervalLock::End => {
          // End is locked - extend start forward (reduce visible range from beginning)
          let new_start = start + step_duration;
          if new_start < end {
            self.state.interval_start = Some(new_start);
          }
        }
      }
    }
  }

  /// Step the interval backward by `step_size`
  pub fn step_backward(&mut self) {
    if let (Some(start), Some(end)) = (self.state.interval_start, self.state.interval_end) {
      let step_duration = self.calculate_step_duration();

      match self.state.interval_lock {
        IntervalLock::None => {
          // Normal behavior - move entire interval
          let interval_duration = end.signed_duration_since(start);
          let new_start = start - step_duration;
          let new_end = new_start + interval_duration;

          // Don't step before the total time range
          if let Some(min_start) = self.state.time_start
            && new_start >= min_start
          {
            self.state.interval_start = Some(new_start);
            self.state.interval_end = Some(new_end);
          }
        }
        IntervalLock::Start => {
          // Start is locked - reduce end (reduce visible range from end)
          let new_end = end - step_duration;
          if new_end > start {
            self.state.interval_end = Some(new_end);
          }
        }
        IntervalLock::End => {
          // End is locked - extend start backward (increase visible range from beginning)
          let new_start = start - step_duration;
          if let Some(min_start) = self.state.time_start
            && new_start >= min_start
          {
            self.state.interval_start = Some(new_start);
          }
        }
      }
    }
  }

  /// Step to the very end of the timeline
  pub fn step_to_end(&mut self) {
    if let (Some(time_end), Some(interval_start), Some(interval_end)) = (
      self.state.time_end,
      self.state.interval_start,
      self.state.interval_end,
    ) {
      let interval_duration = interval_end.signed_duration_since(interval_start);

      match self.state.interval_lock {
        IntervalLock::None => {
          // Move entire interval to the end
          self.state.interval_end = Some(time_end);
          self.state.interval_start = Some(time_end - interval_duration);
        }
        IntervalLock::Start => {
          // Start is locked, extend end to timeline end
          self.state.interval_end = Some(time_end);
        }
        IntervalLock::End => {
          // End is locked, move start as far forward as possible
          // (This doesn't change anything since end is already locked)
        }
      }
    }
  }

  /// Step to the very beginning of the timeline
  pub fn step_to_beginning(&mut self) {
    if let (Some(time_start), Some(interval_start), Some(interval_end)) = (
      self.state.time_start,
      self.state.interval_start,
      self.state.interval_end,
    ) {
      let interval_duration = interval_end.signed_duration_since(interval_start);

      match self.state.interval_lock {
        IntervalLock::None => {
          // Move entire interval to the beginning
          self.state.interval_start = Some(time_start);
          self.state.interval_end = Some(time_start + interval_duration);
        }
        IntervalLock::Start => {
          // Start is locked, move end to minimum possible
          // (This doesn't change anything since start is already locked)
        }
        IntervalLock::End => {
          // End is locked, extend start to timeline beginning
          self.state.interval_start = Some(time_start);
        }
      }
    }
  }

  /// Draw the timeline widget in the given rect
  pub fn draw(&mut self, ui: &mut Ui, rect: Rect) {
    if self.state.time_start.is_none() || self.state.time_end.is_none() {
      return; // No data to display
    }

    // Timeline background
    ui.painter().rect_filled(rect, 8.0, timeline_bg_color());
    ui.painter().rect_stroke(
      rect,
      8.0,
      egui::Stroke::new(1.0, timeline_border_color()),
      egui::epaint::StrokeKind::Outside,
    );

    // Timeline track
    let track_rect = Rect::from_min_size(
      rect.min + Vec2::new(TRACK_MARGIN, TRACK_Y_OFFSET),
      Vec2::new(rect.width() - 2.0 * TRACK_MARGIN, TRACK_HEIGHT),
    );

    // Draw the timeline track
    ui.painter().rect_filled(track_rect, 5.0, track_color());
    ui.painter().rect_stroke(
      track_rect,
      5.0,
      egui::Stroke::new(1.0, track_border_color()),
      egui::epaint::StrokeKind::Outside,
    );

    // Control button dimensions
    let button_size = Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT);
    let step_button_size = Vec2::splat(STEP_BUTTON_SIZE);

    // Calculate control positions
    let total_control_width = SLIDER_WIDTH  // Speed slider
      + BUTTON_SPACING
      + step_button_size.x  // Lock button
      + BUTTON_SPACING
      + step_button_size.x  // Step to beginning
      + BUTTON_SPACING
      + step_button_size.x  // Step backward
      + BUTTON_SPACING
      + button_size.x       // Play/Pause
      + BUTTON_SPACING
      + step_button_size.x  // Step forward
      + BUTTON_SPACING
      + step_button_size.x  // Step to end
      + BUTTON_SPACING
      + SLIDER_WIDTH; // Step size slider
    let controls_start_x = rect.center().x - total_control_width / 2.0;
    let button_y = track_rect.max.y + 6.0;

    // Handle interactions first, then draw
    self.handle_controls_interaction(
      ui,
      controls_start_x,
      button_y,
      button_size,
      step_button_size,
      SLIDER_WIDTH,
      BUTTON_SPACING,
    );
    self.handle_timeline_interaction(ui, track_rect);

    // Draw everything
    self.draw_controls(
      ui,
      controls_start_x,
      button_y,
      button_size,
      step_button_size,
      SLIDER_WIDTH,
      BUTTON_SPACING,
    );
    self.draw_timeline_track(ui, track_rect);
  }

  // Helper functions for common UI patterns

  /// Draw a button with consistent styling and hover effects
  fn draw_button(ui: &mut Ui, rect: Rect, text: &str, font_size: f32, is_hovered: bool) {
    let bg_color = if is_hovered {
      button_hover_color()
    } else {
      button_normal_color()
    };

    ui.painter().rect_filled(rect, 4.0, bg_color);
    ui.painter().rect_stroke(
      rect,
      4.0,
      egui::Stroke::new(1.0, button_border_color()),
      egui::epaint::StrokeKind::Outside,
    );
    ui.painter().text(
      rect.center(),
      egui::Align2::CENTER_CENTER,
      text,
      FontId::proportional(font_size),
      Color32::WHITE,
    );
  }

  /// Draw a slider track with consistent styling
  fn draw_slider_track(ui: &mut Ui, rect: Rect) {
    ui.painter().rect_filled(rect, 5.0, track_color());
    ui.painter().rect_stroke(
      rect,
      5.0,
      egui::Stroke::new(1.0, track_border_color()),
      egui::epaint::StrokeKind::Outside,
    );
  }

  /// Draw a slider handle with hover effects
  fn draw_slider_handle(ui: &mut Ui, center: Pos2, is_hovered: bool) {
    let color = if is_hovered {
      handle_hover_color()
    } else {
      handle_normal_color()
    };
    ui.painter().circle_filled(center, HANDLE_SIZE / 2.0, color);
  }

  /// Calculate normalized position for speed slider (0.0 to 1.0)
  fn speed_to_position(&self) -> f32 {
    (self.playback.speed - MIN_SPEED) / (MAX_SPEED - MIN_SPEED)
  }

  /// Calculate normalized position for step size slider (0.0 to 1.0)  
  fn step_size_to_position(&self) -> f32 {
    (self.playback.step_size.ln() - MIN_STEP_SIZE.ln()) / (MAX_STEP_SIZE.ln() - MIN_STEP_SIZE.ln())
  }

  /// Draw a small lock icon on a handle
  fn draw_lock_icon(ui: &mut Ui, center: Pos2) {
    let lock_size = 4.0;
    let stroke = egui::Stroke::new(1.0, Color32::BLACK);

    // Draw a simple lock shape (small rectangle with arc on top)
    let rect_top = center.y - lock_size / 4.0;
    let rect_bottom = center.y + lock_size / 2.0;
    let rect_left = center.x - lock_size / 3.0;
    let rect_right = center.x + lock_size / 3.0;

    // Lock body (rectangle)
    ui.painter().rect_stroke(
      Rect::from_min_max(
        Pos2::new(rect_left, rect_top),
        Pos2::new(rect_right, rect_bottom),
      ),
      0.0,
      stroke,
      egui::StrokeKind::Outside,
    );

    // Lock shackle (small arc on top)
    ui.painter().circle_stroke(
      center + Vec2::new(0.0, -lock_size / 2.5),
      lock_size / 4.0,
      egui::Stroke::new(1.0, Color32::BLACK),
    );
  }

  /// Handle interactions for the timeline track (interval dragging and handle dragging)
  fn handle_timeline_interaction(&mut self, ui: &mut Ui, track_rect: Rect) {
    // Handle interval and handle interactions if we have interval data
    if let (Some(interval_start), Some(interval_end)) =
      (self.state.interval_start, self.state.interval_end)
      && let (Some(start_pos), Some(end_pos)) = (
        self.time_to_position(interval_start),
        self.time_to_position(interval_end),
      )
    {
      let interval_start_x = track_rect.min.x + start_pos * track_rect.width();
      let interval_end_x = track_rect.min.x + end_pos * track_rect.width();

      // Handle interval dragging (drag the entire interval)
      let interval_rect = Rect::from_min_max(
        Pos2::new(interval_start_x, track_rect.min.y),
        Pos2::new(interval_end_x, track_rect.max.y),
      );
      let interval_drag_response = ui.allocate_rect(interval_rect, Sense::drag());

      if interval_drag_response.drag_started() {
        self.drag.dragging_interval = true;
        self.drag.drag_start_interval_start = self.state.interval_start;
        self.drag.drag_start_interval_end = self.state.interval_end;
        self.drag.drag_start_mouse_pos = ui.input(|i| i.pointer.hover_pos());
      } else if interval_drag_response.dragged() && self.drag.dragging_interval {
        self.handle_interval_drag(ui, track_rect);
      } else if interval_drag_response.drag_stopped() {
        self.drag.dragging_interval = false;
        self.drag.drag_start_interval_start = None;
        self.drag.drag_start_interval_end = None;
        self.drag.drag_start_mouse_pos = None;
      }

      // Handle individual handle dragging
      self.handle_handles_interaction(ui, track_rect, interval_start_x, interval_end_x);
    }
  }

  /// Handle dragging of the entire interval
  fn handle_interval_drag(&mut self, ui: &mut Ui, track_rect: Rect) {
    if let (Some(orig_start), Some(orig_end), Some(start_mouse_pos)) = (
      self.drag.drag_start_interval_start,
      self.drag.drag_start_interval_end,
      self.drag.drag_start_mouse_pos,
    ) && let Some(current_mouse_pos) = ui.input(|i| i.pointer.hover_pos())
    {
      // Calculate the mouse movement delta since drag started
      let mouse_delta_x = current_mouse_pos.x - start_mouse_pos.x;
      let position_delta = mouse_delta_x / track_rect.width();

      // Convert position delta to time delta
      if let (Some(timeline_start), Some(timeline_end)) =
        (self.state.time_start, self.state.time_end)
      {
        let timeline_duration = timeline_end.signed_duration_since(timeline_start);
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let time_delta = chrono::Duration::nanoseconds(
          (timeline_duration.num_nanoseconds().unwrap_or(0) as f32 * position_delta) as i64,
        );

        // Respect interval locks during dragging
        match self.state.interval_lock {
          IntervalLock::None => {
            // Normal behavior - move entire interval
            let new_start = orig_start + time_delta;
            let new_end = orig_end + time_delta;

            // Ensure the interval doesn't go outside the timeline bounds
            if new_start >= timeline_start && new_end <= timeline_end {
              self.state.interval_start = Some(new_start);
              self.state.interval_end = Some(new_end);
            } else if new_start < timeline_start {
              // Clamp to start of timeline
              let interval_duration = orig_end.signed_duration_since(orig_start);
              self.state.interval_start = Some(timeline_start);
              self.state.interval_end = Some(timeline_start + interval_duration);
            } else if new_end > timeline_end {
              // Clamp to end of timeline
              let interval_duration = orig_end.signed_duration_since(orig_start);
              self.state.interval_end = Some(timeline_end);
              self.state.interval_start = Some(timeline_end - interval_duration);
            }
          }
          IntervalLock::Start => {
            // Start is locked, only move the end
            let new_end = orig_end + time_delta;

            if new_end <= timeline_end && new_end > orig_start {
              self.state.interval_end = Some(new_end);
            } else if new_end > timeline_end {
              // Clamp end to timeline boundary
              self.state.interval_end = Some(timeline_end);
            }
            // Start remains unchanged
          }
          IntervalLock::End => {
            // End is locked, only move the start
            let new_start = orig_start + time_delta;

            if new_start >= timeline_start && new_start < orig_end {
              self.state.interval_start = Some(new_start);
            } else if new_start < timeline_start {
              // Clamp start to timeline boundary
              self.state.interval_start = Some(timeline_start);
            }
            // End remains unchanged
          }
        }
      }
    }
  }

  /// Handle interactions for interval handles (start and end dragging)
  fn handle_handles_interaction(
    &mut self,
    ui: &mut Ui,
    track_rect: Rect,
    interval_start_x: f32,
    interval_end_x: f32,
  ) {
    let handle_size = 12.0;

    // Start handle
    let start_handle_rect = Rect::from_center_size(
      Pos2::new(interval_start_x, track_rect.center().y),
      Vec2::splat(handle_size),
    );
    let start_handle_response = ui.allocate_rect(start_handle_rect, Sense::click_and_drag());

    // Handle context menu for start handle
    start_handle_response.context_menu(|ui| {
      ui.set_min_width(120.0);
      match self.state.interval_lock {
        IntervalLock::Start => {
          if ui.button("🔓 Unlock Start").clicked() {
            self.state.interval_lock = IntervalLock::None;
            ui.close();
          }
        }
        _ => {
          if ui.button("🔒 Lock Start").clicked() {
            self.state.interval_lock = IntervalLock::Start;
            ui.close();
          }
        }
      }

      if ui.button("🔄 Toggle Lock Mode").clicked() {
        self.toggle_interval_lock();
        ui.close();
      }
    });

    // Handle dragging only if not locked
    if self.state.interval_lock != IntervalLock::Start {
      if start_handle_response.dragged() && !self.drag.dragging_interval {
        self.drag.dragging_start = true;
        if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
          let new_pos = ((mouse_pos.x - track_rect.min.x) / track_rect.width()).clamp(0.0, 1.0);
          if let Some(new_time) = self.position_to_time(new_pos)
            && let Some(interval_end) = self.state.interval_end
            && new_time < interval_end
          {
            self.state.interval_start = Some(new_time);
          }
        }
      } else if start_handle_response.drag_stopped() {
        self.drag.dragging_start = false;
      }
    }

    // End handle
    let end_handle_rect = Rect::from_center_size(
      Pos2::new(interval_end_x, track_rect.center().y),
      Vec2::splat(handle_size),
    );
    let end_handle_response = ui.allocate_rect(end_handle_rect, Sense::click_and_drag());

    // Handle context menu for end handle
    end_handle_response.context_menu(|ui| {
      ui.set_min_width(120.0);
      match self.state.interval_lock {
        IntervalLock::End => {
          if ui.button("🔓 Unlock End").clicked() {
            self.state.interval_lock = IntervalLock::None;
            ui.close();
          }
        }
        _ => {
          if ui.button("🔒 Lock End").clicked() {
            self.state.interval_lock = IntervalLock::End;
            ui.close();
          }
        }
      }

      if ui.button("🔄 Toggle Lock Mode").clicked() {
        self.toggle_interval_lock();
        ui.close();
      }
    });

    // Handle dragging only if not locked
    if self.state.interval_lock != IntervalLock::End {
      if end_handle_response.dragged() && !self.drag.dragging_interval {
        self.drag.dragging_end = true;
        if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
          let new_pos = ((mouse_pos.x - track_rect.min.x) / track_rect.width()).clamp(0.0, 1.0);
          if let Some(new_time) = self.position_to_time(new_pos)
            && let Some(interval_start) = self.state.interval_start
            && new_time > interval_start
          {
            self.state.interval_end = Some(new_time);
          }
        }
      } else if end_handle_response.drag_stopped() {
        self.drag.dragging_end = false;
      }
    }
  }

  /// Handle interactions for control buttons and sliders
  #[allow(clippy::too_many_arguments)]
  fn handle_controls_interaction(
    &mut self,
    ui: &mut Ui,
    controls_start_x: f32,
    button_y: f32,
    button_size: Vec2,
    step_button_size: Vec2,
    slider_width: f32,
    spacing: f32,
  ) {
    let mut x_pos = controls_start_x;

    // Speed slider interaction
    self.handle_speed_slider_interaction(ui, &mut x_pos, button_y, slider_width);
    x_pos += slider_width + spacing;

    // Lock button
    self.handle_lock_button_interaction(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Step to beginning button
    self.handle_step_to_beginning_interaction(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Step backward button
    self.handle_step_backward_interaction(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Play/Pause button
    self.handle_play_button_interaction(ui, &mut x_pos, button_y, button_size);
    x_pos += button_size.x + spacing;

    // Step forward button
    self.handle_step_forward_interaction(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Step to end button
    self.handle_step_to_end_interaction(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Step size slider interaction
    self.handle_step_slider_interaction(ui, x_pos, button_y, slider_width);
  }

  /// Handle speed slider interaction
  fn handle_speed_slider_interaction(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    slider_width: f32,
  ) {
    let speed_slider_rect = Rect::from_min_size(
      Pos2::new(*x_pos - 2.0, button_y + 5.0),
      Vec2::new(slider_width, 10.0),
    );
    let speed_response = ui.allocate_rect(speed_slider_rect, Sense::click_and_drag());

    if speed_response.dragged()
      && let Some(pointer_pos) = ui.ctx().pointer_interact_pos()
    {
      let relative_x = (pointer_pos.x - speed_slider_rect.min.x) / speed_slider_rect.width();
      let normalized = relative_x.clamp(0.0, 1.0);
      self.playback.speed =
        (normalized * (MAX_SPEED - MIN_SPEED) + MIN_SPEED).clamp(MIN_SPEED, MAX_SPEED);
    }
  }

  /// Handle step size slider interaction
  fn handle_step_slider_interaction(
    &mut self,
    ui: &mut Ui,
    x_pos: f32,
    button_y: f32,
    slider_width: f32,
  ) {
    let step_slider_rect = Rect::from_min_size(
      Pos2::new(x_pos + 2.0, button_y + 5.0),
      Vec2::new(slider_width, 10.0),
    );
    let step_response = ui.allocate_rect(step_slider_rect, Sense::click_and_drag());

    if step_response.dragged()
      && let Some(pointer_pos) = ui.ctx().pointer_interact_pos()
    {
      let relative_x = (pointer_pos.x - step_slider_rect.min.x) / step_slider_rect.width();
      let normalized = relative_x.clamp(0.0, 1.0);
      let log_value = MIN_STEP_SIZE.ln() + normalized * (MAX_STEP_SIZE.ln() - MIN_STEP_SIZE.ln());
      self.playback.step_size = log_value.exp().clamp(MIN_STEP_SIZE, MAX_STEP_SIZE);
    }
  }

  /// Handle step backward button interaction
  fn handle_step_backward_interaction(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::click());
    if response.clicked() {
      self.step_backward();
    }
  }

  /// Handle step forward button interaction
  fn handle_step_forward_interaction(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::click());
    if response.clicked() {
      self.step_forward();
    }
  }

  /// Handle play/pause button interaction
  fn handle_play_button_interaction(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), button_size);
    let response = ui.allocate_rect(button_rect, Sense::click());
    if response.clicked() {
      self.playback.is_playing = !self.playback.is_playing;
    }
  }

  /// Handle lock button interaction
  fn handle_lock_button_interaction(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::click());
    if response.clicked() {
      self.toggle_interval_lock();
    }
  }

  /// Handle step-to-end button interaction
  fn handle_step_to_end_interaction(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::click());
    if response.clicked() {
      self.step_to_end();
    }
  }

  /// Handle step-to-beginning button interaction
  fn handle_step_to_beginning_interaction(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::click());
    if response.clicked() {
      self.step_to_beginning();
    }
  }

  /// Draw the timeline track with interval and handles
  fn draw_timeline_track(&mut self, ui: &mut Ui, track_rect: Rect) {
    // Draw time labels if we have time data
    if let (Some(start), Some(end)) = (self.state.time_start, self.state.time_end) {
      let start_text = self.format_time(start, false);
      let end_text = self.format_time(end, false);

      // Start label
      let start_label_pos = Pos2::new(track_rect.min.x, track_rect.max.y + 8.0);
      let start_text_size = ui
        .painter()
        .layout_no_wrap(
          start_text.clone(),
          FontId::proportional(10.0),
          Color32::WHITE,
        )
        .size();
      ui.painter().rect_filled(
        Rect::from_min_size(
          start_label_pos - Vec2::new(LABEL_PADDING, LABEL_PADDING),
          start_text_size + Vec2::splat(LABEL_PADDING * 2.0),
        ),
        3.0,
        label_bg_color(),
      );
      ui.painter().text(
        start_label_pos,
        egui::Align2::LEFT_TOP,
        start_text,
        FontId::proportional(10.0),
        Color32::WHITE,
      );

      // End label
      let end_label_pos = Pos2::new(track_rect.max.x, track_rect.max.y + 8.0);
      let end_text_size = ui
        .painter()
        .layout_no_wrap(end_text.clone(), FontId::proportional(10.0), Color32::WHITE)
        .size();
      ui.painter().rect_filled(
        Rect::from_min_size(
          end_label_pos - Vec2::new(end_text_size.x + LABEL_PADDING, LABEL_PADDING),
          end_text_size + Vec2::splat(LABEL_PADDING * 2.0),
        ),
        3.0,
        label_bg_color(),
      );
      ui.painter().text(
        end_label_pos,
        egui::Align2::RIGHT_TOP,
        end_text,
        FontId::proportional(10.0),
        Color32::WHITE,
      );
    }

    // Draw interval and handles if we have interval data
    if let (Some(interval_start), Some(interval_end)) =
      (self.state.interval_start, self.state.interval_end)
      && let (Some(start_pos), Some(end_pos)) = (
        self.time_to_position(interval_start),
        self.time_to_position(interval_end),
      )
    {
      let interval_start_x = track_rect.min.x + start_pos * track_rect.width();
      let interval_end_x = track_rect.min.x + end_pos * track_rect.width();

      // Draw interval highlight with visual feedback for dragging state
      let interval_rect = Rect::from_min_max(
        Pos2::new(interval_start_x, track_rect.min.y),
        Pos2::new(interval_end_x, track_rect.max.y),
      );

      // Check hover state for interval coloring
      let interval_hover_response = ui.allocate_rect(interval_rect, Sense::hover());

      let interval_color = if self.drag.dragging_interval {
        interval_drag_color()
      } else if interval_hover_response.hovered() {
        interval_hover_color()
      } else {
        interval_normal_color()
      };

      ui.painter().rect_filled(interval_rect, 5.0, interval_color);

      self.draw_interval_handles(ui, track_rect, interval_start_x, interval_end_x);
      self.draw_interval_labels(
        ui,
        track_rect,
        interval_start_x,
        interval_end_x,
        interval_start,
        interval_end,
      );
    }
  }

  /// Draw the control buttons and sliders
  #[allow(clippy::too_many_arguments)]
  fn draw_controls(
    &mut self,
    ui: &mut Ui,
    controls_start_x: f32,
    button_y: f32,
    button_size: Vec2,
    step_button_size: Vec2,
    slider_width: f32,
    spacing: f32,
  ) {
    let mut x_pos = controls_start_x;

    // Speed slider and label
    self.draw_speed_control(ui, &mut x_pos, button_y, button_size, slider_width);
    x_pos += slider_width + spacing;

    // Lock button
    self.draw_lock_button(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Step to beginning button
    self.draw_step_to_beginning_button(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Step backward button
    self.draw_step_backward_button(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Play/Pause button
    self.draw_play_button(ui, &mut x_pos, button_y, button_size);
    x_pos += button_size.x + spacing;

    // Step forward button
    self.draw_step_forward_button(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Step to end button
    self.draw_step_to_end_button(ui, &mut x_pos, button_y, step_button_size);
    x_pos += step_button_size.x + spacing;

    // Step size slider and label
    self.draw_step_control(ui, x_pos, button_y, button_size, slider_width);
  }

  /// Draw speed control (slider and label)
  fn draw_speed_control(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    button_size: Vec2,
    slider_width: f32,
  ) {
    // Speed label
    let speed_text = format!("Speed: {:.1}x", self.playback.speed);
    ui.painter().text(
      Pos2::new(*x_pos - 9.0, button_y + button_size.y / 2.0),
      egui::Align2::RIGHT_CENTER,
      speed_text,
      FontId::proportional(8.0),
      Color32::from_gray(180),
    );

    // Speed slider
    let speed_slider_rect = Rect::from_min_size(
      Pos2::new(*x_pos - 2.0, button_y + 5.0),
      Vec2::new(slider_width, 10.0),
    );

    // Check hover state for handle coloring
    let speed_response = ui.allocate_rect(speed_slider_rect, Sense::hover());

    // Draw slider track
    Self::draw_slider_track(ui, speed_slider_rect);

    // Draw handle
    let speed_handle_x =
      speed_slider_rect.min.x + self.speed_to_position() * speed_slider_rect.width();
    let handle_center = Pos2::new(speed_handle_x, speed_slider_rect.center().y);
    Self::draw_slider_handle(ui, handle_center, speed_response.hovered());
  }

  /// Draw step size control (slider and label)  
  fn draw_step_control(
    &mut self,
    ui: &mut Ui,
    x_pos: f32,
    button_y: f32,
    button_size: Vec2,
    slider_width: f32,
  ) {
    // Step slider
    let step_slider_rect = Rect::from_min_size(
      Pos2::new(x_pos + 2.0, button_y + 5.0),
      Vec2::new(slider_width, 10.0),
    );

    // Draw slider track
    Self::draw_slider_track(ui, step_slider_rect);

    // Check hover state for handle coloring
    let step_response = ui.allocate_rect(step_slider_rect, Sense::hover());

    // Draw handle
    let step_handle_x =
      step_slider_rect.min.x + self.step_size_to_position() * step_slider_rect.width();
    let handle_center = Pos2::new(step_handle_x, step_slider_rect.center().y);
    Self::draw_slider_handle(ui, handle_center, step_response.hovered());

    // Step label
    let step_text = if self.playback.step_size < 1.0 {
      format!("Step: {:.1}s", self.playback.step_size)
    } else if self.playback.step_size < 60.0 {
      if self.playback.step_size.fract() == 0.0 {
        #[allow(clippy::cast_possible_truncation)]
        let seconds = self.playback.step_size as i32;
        format!("Step: {seconds}s")
      } else {
        format!("Step: {:.1}s", self.playback.step_size)
      }
    } else if self.playback.step_size < 3600.0 {
      #[allow(clippy::cast_possible_truncation)]
      let minutes = (self.playback.step_size / 60.0) as i32;
      format!("Step: {minutes}m")
    } else if self.playback.step_size < 86400.0 {
      format!("Step: {:.1}h", self.playback.step_size / 3600.0)
    } else {
      #[allow(clippy::cast_possible_truncation)]
      let days = (self.playback.step_size / 86400.0) as i32;
      format!("Step: {days}d")
    };
    ui.painter().text(
      Pos2::new(step_slider_rect.max.x + 9.0, button_y + button_size.y / 2.0),
      egui::Align2::LEFT_CENTER,
      step_text,
      FontId::proportional(8.0),
      Color32::from_gray(180),
    );
  }

  /// Draw step backward button
  #[allow(clippy::unused_self)]
  fn draw_step_backward_button(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::hover());

    Self::draw_button(ui, button_rect, "|◀", 10.0, response.hovered());
  }

  /// Draw step forward button
  #[allow(clippy::unused_self)]
  fn draw_step_forward_button(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::hover());

    Self::draw_button(ui, button_rect, "▶|", 10.0, response.hovered());
  }

  /// Draw play/pause button
  #[allow(clippy::unused_self)]
  fn draw_play_button(&mut self, ui: &mut Ui, x_pos: &mut f32, button_y: f32, button_size: Vec2) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), button_size);
    let response = ui.allocate_rect(button_rect, Sense::hover());

    let play_text = if self.playback.is_playing {
      "⏸"
    } else {
      "▶"
    };
    Self::draw_button(ui, button_rect, play_text, 14.0, response.hovered());
  }

  /// Draw lock button
  #[allow(clippy::unused_self)]
  fn draw_lock_button(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::hover());

    let lock_text = match self.state.interval_lock {
      IntervalLock::None => "🔓",
      IntervalLock::Start => "🔒S",
      IntervalLock::End => "🔒E",
    };
    Self::draw_button(ui, button_rect, lock_text, 10.0, response.hovered());
  }

  /// Draw step to end button
  #[allow(clippy::unused_self)]
  fn draw_step_to_end_button(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::hover());

    Self::draw_button(ui, button_rect, "⏭", 12.0, response.hovered());
  }

  /// Draw step to beginning button
  #[allow(clippy::unused_self)]
  fn draw_step_to_beginning_button(
    &mut self,
    ui: &mut Ui,
    x_pos: &mut f32,
    button_y: f32,
    step_button_size: Vec2,
  ) {
    let button_rect = Rect::from_min_size(Pos2::new(*x_pos, button_y), step_button_size);
    let response = ui.allocate_rect(button_rect, Sense::hover());

    Self::draw_button(ui, button_rect, "⏮", 12.0, response.hovered());
  }

  /// Draw interval handles with visual feedback for dragging and lock state
  #[allow(clippy::unused_self)]
  fn draw_interval_handles(
    &mut self,
    ui: &mut Ui,
    track_rect: Rect,
    interval_start_x: f32,
    interval_end_x: f32,
  ) {
    // Start handle
    let start_color = if self.state.interval_lock == IntervalLock::Start {
      Color32::from_rgb(255, 165, 0) // Orange for locked
    } else if self.drag.dragging_start {
      handle_hover_color()
    } else {
      handle_normal_color()
    };

    ui.painter().circle_filled(
      Pos2::new(interval_start_x, track_rect.center().y),
      HANDLE_SIZE / 2.0,
      start_color,
    );

    // Draw lock indicator for start handle
    if self.state.interval_lock == IntervalLock::Start {
      Self::draw_lock_icon(ui, Pos2::new(interval_start_x, track_rect.center().y));
    }

    // End handle
    let end_color = if self.state.interval_lock == IntervalLock::End {
      Color32::from_rgb(255, 165, 0)
    } else if self.drag.dragging_end {
      handle_hover_color()
    } else {
      handle_normal_color()
    };

    ui.painter().circle_filled(
      Pos2::new(interval_end_x, track_rect.center().y),
      HANDLE_SIZE / 2.0,
      end_color,
    );

    // Draw lock indicator for end handle
    if self.state.interval_lock == IntervalLock::End {
      Self::draw_lock_icon(ui, Pos2::new(interval_end_x, track_rect.center().y));
    }
  }

  /// Draw interval labels above the timeline
  #[allow(clippy::unused_self)]
  fn draw_interval_labels(
    &mut self,
    ui: &mut Ui,
    track_rect: Rect,
    interval_start_x: f32,
    interval_end_x: f32,
    interval_start: DateTime<Utc>,
    interval_end: DateTime<Utc>,
  ) {
    let interval_start_text = self.format_time(interval_start, true);
    let interval_end_text = self.format_time(interval_end, true);

    if (interval_end_x - interval_start_x) > MIN_LABEL_DISTANCE {
      // Start interval label
      let start_text_size = ui
        .painter()
        .layout_no_wrap(
          interval_start_text.clone(),
          FontId::proportional(10.0),
          Color32::WHITE,
        )
        .size();
      let start_label_pos = Pos2::new(interval_start_x, track_rect.min.y - 15.0);
      ui.painter().rect_filled(
        Rect::from_center_size(
          start_label_pos,
          start_text_size + Vec2::splat(LABEL_PADDING * 2.0),
        ),
        2.0,
        interval_label_bg_color(),
      );
      ui.painter().text(
        start_label_pos,
        egui::Align2::CENTER_CENTER,
        interval_start_text,
        FontId::proportional(10.0),
        Color32::WHITE,
      );

      // End interval label
      let end_text_size = ui
        .painter()
        .layout_no_wrap(
          interval_end_text.clone(),
          FontId::proportional(10.0),
          Color32::WHITE,
        )
        .size();
      let end_label_pos = Pos2::new(interval_end_x, track_rect.min.y - 15.0);
      ui.painter().rect_filled(
        Rect::from_center_size(
          end_label_pos,
          end_text_size + Vec2::splat(LABEL_PADDING * 2.0),
        ),
        2.0,
        interval_label_bg_color(),
      );
      ui.painter().text(
        end_label_pos,
        egui::Align2::CENTER_CENTER,
        interval_end_text,
        FontId::proportional(10.0),
        Color32::WHITE,
      );
    }
  }

  /// Convert time to position (0.0 to 1.0) on timeline
  fn time_to_position(&self, time: DateTime<Utc>) -> Option<f32> {
    if let (Some(start), Some(end)) = (self.state.time_start, self.state.time_end) {
      let total_duration = end.signed_duration_since(start);
      let time_offset = time.signed_duration_since(start);

      if total_duration.num_nanoseconds().unwrap_or(0) == 0 {
        return Some(0.0);
      }

      #[allow(clippy::cast_precision_loss)]
      let position = time_offset.num_nanoseconds().unwrap_or(0) as f32
        / total_duration.num_nanoseconds().unwrap_or(1) as f32;
      Some(position.clamp(0.0, 1.0))
    } else {
      None
    }
  }

  /// Convert timeline position (0.0 to 1.0) to time
  fn position_to_time(&self, position: f32) -> Option<DateTime<Utc>> {
    if let (Some(start), Some(end)) = (self.state.time_start, self.state.time_end) {
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
    if let (Some(start), Some(end)) = (self.state.time_start, self.state.time_end) {
      let total_duration = end.signed_duration_since(start);

      if total_duration.num_days() > 365 {
        if is_detailed {
          time.format("%Y-%m-%d %H:%M").to_string()
        } else {
          time.format("%Y-%m").to_string()
        }
      } else if total_duration.num_days() > 30 {
        if is_detailed {
          time.format("%m-%d %H:%M").to_string()
        } else {
          time.format("%m-%d").to_string()
        }
      } else if total_duration.num_days() > 1 {
        if is_detailed {
          time.format("%m/%d %H:%M:%S").to_string()
        } else {
          time.format("%m/%d %H:%M").to_string()
        }
      } else if is_detailed {
        time.format("%H:%M:%S").to_string()
      } else {
        time.format("%H:%M").to_string()
      }
    } else {
      time.format("%H:%M:%S").to_string()
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use chrono::{TimeZone, Utc};
  use rstest::rstest;

  #[test]
  fn test_step_forward_basic() {
    let mut widget = TimelineWidget::new();

    // Set up a timeline from 10:00 to 10:10 (10 minutes)
    let start_time = Utc.with_ymd_and_hms(2023, 1, 1, 10, 0, 0).unwrap();
    let end_time = Utc.with_ymd_and_hms(2023, 1, 1, 10, 10, 0).unwrap();

    widget.set_time_range(Some(start_time), Some(end_time));

    // Set initial interval from 10:00 to 10:02 (2 minutes)
    let interval_start = Utc.with_ymd_and_hms(2023, 1, 1, 10, 0, 0).unwrap();
    let interval_end = Utc.with_ymd_and_hms(2023, 1, 1, 10, 2, 0).unwrap();
    widget.set_interval(Some(interval_start), Some(interval_end));

    // Set step size to 1 second
    widget.set_step_size(1.0);

    // Step forward once
    widget.step_forward();

    let (new_start, new_end) = widget.get_interval();

    // Should advance by 1 second
    let expected_start = Utc.with_ymd_and_hms(2023, 1, 1, 10, 0, 1).unwrap();
    let expected_end = Utc.with_ymd_and_hms(2023, 1, 1, 10, 2, 1).unwrap();

    assert_eq!(
      new_start,
      Some(expected_start),
      "Start should advance by 1 second"
    );
    assert_eq!(
      new_end,
      Some(expected_end),
      "End should advance by 1 second"
    );
  }

  #[test]
  fn test_step_forward_with_start_lock() {
    let mut widget = TimelineWidget::new();

    // Set up timeline
    let start_time = Utc.with_ymd_and_hms(2023, 1, 1, 10, 0, 0).unwrap();
    let end_time = Utc.with_ymd_and_hms(2023, 1, 1, 10, 10, 0).unwrap();
    widget.set_time_range(Some(start_time), Some(end_time));

    // Set initial interval
    let interval_start = Utc.with_ymd_and_hms(2023, 1, 1, 10, 0, 0).unwrap();
    let interval_end = Utc.with_ymd_and_hms(2023, 1, 1, 10, 2, 0).unwrap();
    widget.set_interval(Some(interval_start), Some(interval_end));

    // Lock the start
    widget.state.interval_lock = IntervalLock::Start;

    // Set step size to 1 second and step forward
    widget.set_step_size(1.0);
    widget.step_forward();

    let (new_start, new_end) = widget.get_interval();

    // Start should remain locked, only end should advance
    let expected_end = Utc.with_ymd_and_hms(2023, 1, 1, 10, 2, 1).unwrap();

    assert_eq!(
      new_start,
      Some(interval_start),
      "Start should remain locked"
    );
    assert_eq!(
      new_end,
      Some(expected_end),
      "End should advance by 1 second"
    );
  }

  #[test]
  fn test_step_size_logarithmic_scaling() {
    let _widget = TimelineWidget::new();

    // Test the logarithmic scaling function
    let mut test_widget = TimelineWidget::new();

    // Test minimum step size (0.1 second)
    test_widget.playback.step_size = MIN_STEP_SIZE;
    let pos_min = test_widget.step_size_to_position();
    assert!(
      (pos_min - 0.0).abs() < 0.01,
      "Min step size should map to ~0.0, got {pos_min}"
    );

    // Test maximum step size (1 day = 86400 seconds)
    test_widget.playback.step_size = MAX_STEP_SIZE;
    let pos_max = test_widget.step_size_to_position();
    assert!(
      (pos_max - 1.0).abs() < 0.01,
      "Max step size should map to ~1.0, got {pos_max}"
    );

    // Test middle value (around 1 minute = 60 seconds)
    test_widget.playback.step_size = 60.0;
    let pos_mid = test_widget.step_size_to_position();
    assert!(
      pos_mid > 0.0 && pos_mid < 1.0,
      "Mid step size should be between 0 and 1, got {pos_mid}"
    );
  }

  #[test]
  fn test_step_size_conversion_roundtrip() {
    // Test the conversion from normalized position back to step size
    let test_cases = vec![0.1, 1.0, 5.0, 60.0, 300.0, 3600.0, 86400.0];

    for original_step_size in test_cases {
      let mut widget = TimelineWidget::new();
      widget.playback.step_size = original_step_size;

      // Get the normalized position
      let normalized = widget.step_size_to_position();

      // Convert back using the same logic as the slider
      let log_value = MIN_STEP_SIZE.ln() + normalized * (MAX_STEP_SIZE.ln() - MIN_STEP_SIZE.ln());
      let converted_step_size = log_value.exp().clamp(MIN_STEP_SIZE, MAX_STEP_SIZE);

      let diff = (converted_step_size - original_step_size).abs();
      let relative_error = diff / original_step_size;

      assert!(
        relative_error < 0.01,
        "Step size conversion failed: {original_step_size} -> {normalized} -> {converted_step_size}, relative error: {relative_error}"
      );
    }
  }

  #[rstest]
  #[case(0.5, 500, 0)]
  #[case(1.0, 0, 1)]
  #[case(1.5, 500, 1)]
  #[case(60.0, 0, 60)]
  #[case(60.5, 500, 60)]
  fn test_step_duration_precision(
    #[case] step_size: f32,
    #[case] expected_millis: i64,
    #[case] expected_secs: i64,
  ) {
    let mut widget = TimelineWidget::new();
    widget.set_step_size(step_size);

    let expected = Duration::seconds(expected_secs) + Duration::milliseconds(expected_millis);
    let actual = calc_step_duration(widget.playback.step_size);
    assert_eq!(actual, expected);
  }

  #[allow(clippy::cast_possible_truncation)]
  fn calc_step_duration(step_size: f32) -> Duration {
    if step_size < 1.0 {
      Duration::milliseconds(i64::from((step_size * 1000.0) as i32))
    } else {
      Duration::seconds(i64::from(step_size as i32))
        + Duration::milliseconds(i64::from(((step_size % 1.0) * 1000.0) as i32))
    }
  }

  #[test]
  fn debug_step_size_edge_cases() {
    let mut widget = TimelineWidget::new();

    // Test what happens at slider extremes
    println!("Testing edge cases for step size calculation:");

    // Test normalized = 0.0 (leftmost position)
    let normalized = 0.0;
    let log_value = MIN_STEP_SIZE.ln() + normalized * (MAX_STEP_SIZE.ln() - MIN_STEP_SIZE.ln());
    let step_size_0 = log_value.exp().clamp(MIN_STEP_SIZE, MAX_STEP_SIZE);
    println!("normalized=0.0: log_value={log_value}, step_size={step_size_0}");

    // Test normalized = 1.0 (rightmost position)
    let normalized = 1.0;
    let log_value = MIN_STEP_SIZE.ln() + normalized * (MAX_STEP_SIZE.ln() - MIN_STEP_SIZE.ln());
    let step_size_1 = log_value.exp().clamp(MIN_STEP_SIZE, MAX_STEP_SIZE);
    println!("normalized=1.0: log_value={log_value}, step_size={step_size_1}");

    // Test what step size gives us when we try to set 1.0
    widget.set_step_size(1.0);
    let actual_step_size = widget.get_step_size();
    println!("set_step_size(1.0) -> get_step_size()={actual_step_size}");

    assert!(
      step_size_0 >= MIN_STEP_SIZE,
      "Minimum position should not give step size below MIN_STEP_SIZE"
    );
    assert!(
      step_size_1 <= MAX_STEP_SIZE,
      "Maximum position should not give step size above MAX_STEP_SIZE"
    );
    assert!(
      (actual_step_size - 1.0).abs() < f32::EPSILON,
      "Setting step size to 1.0 should work"
    );
  }

  #[test]
  fn test_step_with_large_timeline() {
    let mut widget = TimelineWidget::new();

    // Create a large timeline: 2023-01-01 to 2023-12-31 (full year)
    let start_time = chrono::Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let end_time = chrono::Utc
      .with_ymd_and_hms(2023, 12, 31, 23, 59, 59)
      .unwrap();

    widget.set_time_range(Some(start_time), Some(end_time));

    // Set a small initial interval: first hour of the year
    let interval_start = start_time;
    let interval_end = start_time + chrono::Duration::hours(1);
    widget.set_interval(Some(interval_start), Some(interval_end));

    // Set step size to 1 second
    widget.set_step_size(1.0);

    println!(
      "Before step: interval=({:?}, {:?})",
      widget.get_interval().0,
      widget.get_interval().1
    );

    // Step forward once
    widget.step_forward();

    let (new_start, new_end) = widget.get_interval();
    println!("After step: interval=({new_start:?}, {new_end:?})");

    // Should advance by exactly 1 second
    let expected_start = start_time + chrono::Duration::seconds(1);
    let expected_end = interval_end + chrono::Duration::seconds(1);

    assert_eq!(
      new_start,
      Some(expected_start),
      "Start should advance by 1 second, not jump to end"
    );
    assert_eq!(
      new_end,
      Some(expected_end),
      "End should advance by 1 second, not jump to end"
    );

    // Verify we didn't jump anywhere near the end
    if let Some(actual_end) = new_end {
      let time_to_end = end_time.signed_duration_since(actual_end);
      assert!(
        time_to_end.num_days() > 360,
        "Should not have jumped close to end of year, time remaining: {} days",
        time_to_end.num_days()
      );
    }
  }
}
