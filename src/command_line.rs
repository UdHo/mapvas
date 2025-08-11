use egui::{Color32, Key};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum CommandLineMode {
  Normal, // Default mode, commands start with ':'
  Search, // Search mode, commands start with '/'
  Filter, // Filter mode, commands start with '&'
  Hidden, // Command line is not visible
}

#[derive(Clone, Debug)]
pub enum Command {
  // Basic commands
  Quit,
  Write,
  WriteQuit,

  // Search commands
  Search(String),
  SearchNext,
  SearchPrev,

  // Filter commands
  Filter(String),
  ClearFilter,

  // Navigation commands
  GoTo(String),
  Focus(String),

  // Layer commands
  ShowLayer(String),
  HideLayer(String),
  ToggleLayer(String),

  // View commands
  ZoomIn,
  ZoomOut,
  ZoomFit,

  // Unknown command
  Unknown(String),
}

pub struct CommandLine {
  pub mode: CommandLineMode,
  pub input: String,
  pub history: Vec<String>,
  pub history_index: Option<usize>,
  pub last_search: Option<String>,
  pub message: Option<(String, bool)>, // (message, is_error)
  pub command_handlers: HashMap<String, fn(&str) -> Command>,
}

impl Default for CommandLine {
  fn default() -> Self {
    Self::new()
  }
}

impl CommandLine {
  #[must_use]
  pub fn new() -> Self {
    let mut cmd = Self {
      mode: CommandLineMode::Hidden,
      input: String::new(),
      history: Vec::new(),
      history_index: None,
      last_search: None,
      message: None,
      command_handlers: HashMap::new(),
    };

    cmd.setup_command_handlers();
    cmd
  }

  fn setup_command_handlers(&mut self) {
    self
      .command_handlers
      .insert("q".to_string(), |_| Command::Quit);
    self
      .command_handlers
      .insert("quit".to_string(), |_| Command::Quit);
    self
      .command_handlers
      .insert("w".to_string(), |_| Command::Write);
    self
      .command_handlers
      .insert("write".to_string(), |_| Command::Write);
    self
      .command_handlers
      .insert("wq".to_string(), |_| Command::WriteQuit);
    self
      .command_handlers
      .insert("x".to_string(), |_| Command::WriteQuit);

    self
      .command_handlers
      .insert("n".to_string(), |_| Command::SearchNext);
    self
      .command_handlers
      .insert("N".to_string(), |_| Command::SearchPrev);

    self
      .command_handlers
      .insert("zi".to_string(), |_| Command::ZoomIn);
    self
      .command_handlers
      .insert("zo".to_string(), |_| Command::ZoomOut);
    self
      .command_handlers
      .insert("zf".to_string(), |_| Command::ZoomFit);
  }

  /// Enter command mode (show command line with ':')
  pub fn enter_command_mode(&mut self) {
    self.mode = CommandLineMode::Normal;
    self.input = ":".to_string();
  }

  /// Enter search mode (show command line with '/')
  pub fn enter_search_mode(&mut self) {
    self.mode = CommandLineMode::Search;
    self.input = "/".to_string();
  }

  /// Enter filter mode (show command line with '&')
  pub fn enter_filter_mode(&mut self) {
    self.mode = CommandLineMode::Filter;
    self.input = "&".to_string();
  }

  /// Hide the command line
  pub fn hide(&mut self) {
    self.mode = CommandLineMode::Hidden;
    self.input.clear();
    self.history_index = None;
  }

  /// Check if command line is visible
  #[must_use]
  pub fn is_visible(&self) -> bool {
    self.mode != CommandLineMode::Hidden
  }

  /// Process input character
  pub fn handle_char(&mut self, c: char) {
    if self.mode != CommandLineMode::Hidden {
      self.input.push(c);
    }
  }

  /// Handle backspace
  pub fn handle_backspace(&mut self) {
    if self.mode != CommandLineMode::Hidden {
      // Don't delete the command prefix (':' or '/')
      let min_len = match self.mode {
        // Keep '/'
        CommandLineMode::Filter | CommandLineMode::Search | CommandLineMode::Normal => 1, // Keep '&'
        CommandLineMode::Hidden => 0,
      };

      match self.input.len().cmp(&min_len) {
        std::cmp::Ordering::Greater => {
          self.input.pop();
        }
        std::cmp::Ordering::Equal => {
          self.hide();
        }
        std::cmp::Ordering::Less => {}
      }
    }
  }

  /// Handle Enter key - execute command
  pub fn handle_enter(&mut self) -> Option<Command> {
    if self.mode == CommandLineMode::Hidden {
      return None;
    }

    let command_text = self.input.clone();

    // Add to history if it's not empty and different from last command
    if !command_text.is_empty() && self.history.last() != Some(&command_text) {
      self.history.push(command_text.clone());
      // Limit history size
      if self.history.len() > 100 {
        self.history.remove(0);
      }
    }

    let result = match self.mode {
      CommandLineMode::Normal => self.parse_normal_command(&command_text),
      CommandLineMode::Search => self.parse_search_command(&command_text),
      CommandLineMode::Filter => Self::parse_filter_command(&command_text),
      CommandLineMode::Hidden => None,
    };

    // Reset state
    self.hide();

    result
  }

  /// Handle Escape key - cancel command
  pub fn handle_escape(&mut self) {
    self.hide();
  }

  /// Handle Up arrow - previous command in history
  pub fn handle_up_arrow(&mut self) {
    if self.mode == CommandLineMode::Hidden || self.history.is_empty() {
      return;
    }

    let new_index = match self.history_index {
      None => Some(self.history.len() - 1),
      Some(0) => Some(0), // Stay at first item
      Some(i) => Some(i - 1),
    };

    if let Some(idx) = new_index {
      self.history_index = Some(idx);
      self.input = self.history[idx].clone();
    }
  }

  /// Handle Down arrow - next command in history
  pub fn handle_down_arrow(&mut self) {
    if let Some(idx) = self.history_index {
      if idx >= self.history.len() - 1 {
        // Go back to empty input
        self.history_index = None;
        self.input = match self.mode {
          CommandLineMode::Normal => ":".to_string(),
          CommandLineMode::Search => "/".to_string(),
          CommandLineMode::Filter => "&".to_string(),
          CommandLineMode::Hidden => String::new(),
        };
      } else {
        let new_idx = idx + 1;
        self.history_index = Some(new_idx);
        self.input = self.history[new_idx].clone();
      }
    }
  }

  /// Parse normal command (starting with ':')
  fn parse_normal_command(&mut self, input: &str) -> Option<Command> {
    if !input.starts_with(':') {
      return None;
    }

    let cmd_text = &input[1..].trim();

    if cmd_text.is_empty() {
      return None;
    }

    // Split command and arguments
    let parts: Vec<&str> = cmd_text.split_whitespace().collect();
    let cmd = parts[0];
    let args = if parts.len() > 1 {
      parts[1..].join(" ")
    } else {
      String::new()
    };

    // Check built-in commands first
    if let Some(handler) = self.command_handlers.get(cmd) {
      return Some(handler(&args));
    }

    // Handle parameterized commands
    match cmd {
      "go" | "goto" => Some(Command::GoTo(args)),
      "focus" => Some(Command::Focus(args)),
      "show" => Some(Command::ShowLayer(args)),
      "hide" => Some(Command::HideLayer(args)),
      "toggle" => Some(Command::ToggleLayer(args)),
      _ => Some(Command::Unknown((*cmd_text).to_string())),
    }
  }

  /// Parse search command (starting with '/')
  fn parse_search_command(&mut self, input: &str) -> Option<Command> {
    if !input.starts_with('/') {
      return None;
    }

    let search_term = input[1..].to_string();

    if search_term.is_empty() {
      return None;
    }

    self.last_search = Some(search_term.clone());
    Some(Command::Search(search_term))
  }

  /// Parse filter command (starting with '&')
  fn parse_filter_command(input: &str) -> Option<Command> {
    if !input.starts_with('&') {
      return None;
    }

    let filter_term = input[1..].to_string();

    if filter_term.is_empty() {
      // Empty filter means clear filter
      return Some(Command::ClearFilter);
    }

    Some(Command::Filter(filter_term))
  }

  /// Set a message to display
  pub fn set_message(&mut self, message: String, is_error: bool) {
    self.message = Some((message, is_error));
  }

  /// Clear the current message
  pub fn clear_message(&mut self) {
    self.message = None;
  }

  /// Get the current message
  #[must_use]
  pub fn get_message(&self) -> Option<&(String, bool)> {
    self.message.as_ref()
  }
}

/// Handle keyboard input for vim-like command line
pub fn handle_command_line_input(cmd: &mut CommandLine, ctx: &egui::Context) -> Option<Command> {
  let mut result = None;

  ctx.input(|i| {
    // Handle key presses
    for event in &i.events {
      if let egui::Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
      } = event
      {
        match key {
          Key::Colon if modifiers.is_none() && !cmd.is_visible() => {
            cmd.enter_command_mode();
          }
          Key::Slash if modifiers.is_none() && !cmd.is_visible() => {
            cmd.enter_search_mode();
          }
          _ if *key == Key::Num7 && modifiers.shift && !cmd.is_visible() => {
            // Shift+7 produces '&' on most keyboard layouts
            cmd.enter_filter_mode();
          }
          Key::Enter if cmd.is_visible() => {
            result = cmd.handle_enter();
          }
          Key::Escape if cmd.is_visible() => {
            cmd.handle_escape();
          }
          // Let TextEdit handle backspace naturally
          Key::ArrowUp if cmd.is_visible() => {
            cmd.handle_up_arrow();
          }
          Key::ArrowDown if cmd.is_visible() => {
            cmd.handle_down_arrow();
          }
          _ => {}
        }
      }

      // Handle text input only for activation keys when command line is not visible
      if let egui::Event::Text(text) = event {
        if !cmd.is_visible() {
          for c in text.chars() {
            // Handle vim-like activation keys when command line is not visible
            match c {
              ':' => {
                cmd.enter_command_mode();
              }
              '/' => {
                cmd.enter_search_mode();
              }
              _ => {
                // Ignore other chars when not visible
              }
            }
          }
        }
        // When visible, let the TextEdit widget handle all text input
      }
    }
  });

  result
}

/// Show the command line UI at the bottom of the screen
pub fn show_command_line_ui(cmd: &mut CommandLine, ctx: &egui::Context) {
  if !cmd.is_visible() {
    // Show message if present
    if let Some((message, is_error)) = &cmd.message {
      show_status_message(message, *is_error, ctx);
    }
    return;
  }

  let screen_rect = ctx.screen_rect();
  let height = 30.0;

  egui::Area::new(egui::Id::new("command_line"))
    .fixed_pos(egui::pos2(0.0, screen_rect.max.y - height))
    .show(ctx, |ui| {
      ui.set_min_width(screen_rect.width());
      ui.set_min_height(height);

      let frame = egui::Frame::NONE
        .fill(Color32::from_gray(40))
        .stroke(egui::Stroke::new(1.0, Color32::from_gray(80)));

      frame.show(ui, |ui| {
        ui.set_min_height(height);
        ui.horizontal(|ui| {
          ui.add_space(8.0);

          // Show command line input
          let text_edit = egui::TextEdit::singleline(&mut cmd.input)
            .desired_width(ui.available_width() - 16.0)
            .font(egui::TextStyle::Monospace)
            .text_color(Color32::WHITE);

          let response = ui.add(text_edit);

          // Check if command prefix was deleted - if so, hide command line
          let expected_prefix = match cmd.mode {
            CommandLineMode::Normal => ":",
            CommandLineMode::Search => "/",
            CommandLineMode::Filter => "&",
            CommandLineMode::Hidden => "",
          };

          if cmd.is_visible() && !cmd.input.starts_with(expected_prefix) {
            cmd.hide();
          }

          // Auto-focus the text input
          if cmd.is_visible() {
            response.request_focus();
          }
        });
      });
    });
}

/// Show a status message at the bottom of the screen
fn show_status_message(message: &str, is_error: bool, ctx: &egui::Context) {
  let screen_rect = ctx.screen_rect();
  let height = 25.0;

  egui::Area::new(egui::Id::new("status_message"))
    .fixed_pos(egui::pos2(0.0, screen_rect.max.y - height))
    .show(ctx, |ui| {
      ui.set_min_width(screen_rect.width());
      ui.set_min_height(height);

      let (bg_color, text_color) = if is_error {
        (Color32::from_rgb(150, 40, 40), Color32::WHITE)
      } else {
        (Color32::from_gray(60), Color32::WHITE)
      };

      let frame = egui::Frame::NONE
        .fill(bg_color)
        .stroke(egui::Stroke::new(1.0, Color32::from_gray(100)));

      frame.show(ui, |ui| {
        ui.set_min_height(height);
        ui.horizontal(|ui| {
          ui.add_space(8.0);
          ui.label(egui::RichText::new(message).color(text_color));
        });
      });
    });
}
