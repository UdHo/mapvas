use mapvas::command_line::{Command, CommandLine, CommandLineMode};

#[test]
fn test_command_line_creation() {
  let cmd = CommandLine::new();
  assert_eq!(cmd.mode, CommandLineMode::Hidden);
  assert!(!cmd.is_visible());
}

#[test]
fn test_command_line_mode_transitions() {
  let mut cmd = CommandLine::new();

  // Start hidden
  assert_eq!(cmd.mode, CommandLineMode::Hidden);

  // Enter normal mode
  cmd.enter_command_mode();
  assert_eq!(cmd.mode, CommandLineMode::Normal);
  assert_eq!(cmd.input, ":");
  assert!(cmd.is_visible());

  // Enter search mode
  cmd.enter_search_mode();
  assert_eq!(cmd.mode, CommandLineMode::Search);
  assert_eq!(cmd.input, "/");
  assert!(cmd.is_visible());
}

#[test]
fn test_character_input() {
  let mut cmd = CommandLine::new();

  // Should not handle chars when hidden
  cmd.handle_char('a');
  assert_eq!(cmd.input, "");

  // Should handle chars when visible
  cmd.enter_command_mode();
  cmd.handle_char('q');
  assert_eq!(cmd.input, ":q");
}

#[test]
fn test_backspace_handling() {
  let mut cmd = CommandLine::new();
  cmd.enter_command_mode();
  cmd.handle_char('q');
  cmd.handle_char('u');
  cmd.handle_char('i');
  cmd.handle_char('t');
  assert_eq!(cmd.input, ":quit");

  // Backspace should remove characters
  cmd.handle_backspace();
  assert_eq!(cmd.input, ":qui");

  cmd.handle_backspace();
  cmd.handle_backspace();
  cmd.handle_backspace();
  assert_eq!(cmd.input, ":");

  // Backspacing the prefix should hide the command line
  cmd.handle_backspace();
  assert!(!cmd.is_visible());
  assert_eq!(cmd.mode, CommandLineMode::Hidden);
}

#[test]
fn test_command_execution() {
  let mut cmd = CommandLine::new();

  // Test quit command
  cmd.enter_command_mode();
  cmd.handle_char('q');
  if let Some(command) = cmd.handle_enter() {
    assert!(matches!(command, Command::Quit));
  } else {
    panic!("Expected quit command");
  }

  // Test search command
  cmd.enter_search_mode();
  cmd.handle_char('t');
  cmd.handle_char('e');
  cmd.handle_char('s');
  cmd.handle_char('t');
  if let Some(command) = cmd.handle_enter() {
    assert!(matches!(command, Command::Search(_)));
    if let Command::Search(query) = command {
      assert_eq!(query, "test");
    }
  } else {
    panic!("Expected search command");
  }
}

#[test]
fn test_history_navigation() {
  let mut cmd = CommandLine::new();

  // Add some history
  cmd.history.push(":quit".to_string());
  cmd.history.push("/search term".to_string());
  cmd.history.push(":write".to_string());

  // Enter command mode
  cmd.enter_command_mode();
  assert_eq!(cmd.input, ":");

  // Navigate up through history
  cmd.handle_up_arrow();
  assert_eq!(cmd.input, ":write");

  cmd.handle_up_arrow();
  assert_eq!(cmd.input, "/search term");

  cmd.handle_up_arrow();
  assert_eq!(cmd.input, ":quit");

  // Can't go up further
  cmd.handle_up_arrow();
  assert_eq!(cmd.input, ":quit");

  // Navigate down
  cmd.handle_down_arrow();
  assert_eq!(cmd.input, "/search term");

  cmd.handle_down_arrow();
  assert_eq!(cmd.input, ":write");

  // Going down from last item returns to empty command
  cmd.handle_down_arrow();
  assert_eq!(cmd.input, ":");
}

#[test]
fn test_escape_handling() {
  let mut cmd = CommandLine::new();

  // Enter command mode and type something
  cmd.enter_command_mode();
  cmd.handle_char('q');
  cmd.handle_char('u');
  cmd.handle_char('i');
  cmd.handle_char('t');
  assert_eq!(cmd.input, ":quit");
  assert!(cmd.is_visible());

  // Escape should hide and clear
  cmd.handle_escape();
  assert!(!cmd.is_visible());
  assert_eq!(cmd.mode, CommandLineMode::Hidden);
}

#[test]
fn test_regex_search_command_parsing() {
  let mut cmd = CommandLine::new();

  // Test regex search command parsing
  cmd.enter_search_mode();
  cmd.handle_char('b');
  cmd.handle_char('u');
  cmd.handle_char('i');
  cmd.handle_char('l');
  cmd.handle_char('d');
  cmd.handle_char('i');
  cmd.handle_char('n');
  cmd.handle_char('g');
  cmd.handle_char('*');

  assert_eq!(cmd.input, "/building*");

  if let Some(command) = cmd.handle_enter() {
    assert!(matches!(command, Command::Search(_)));
    if let Command::Search(query) = command {
      assert_eq!(query, "building*");
    }
  } else {
    panic!("Expected search command with regex");
  }
}
