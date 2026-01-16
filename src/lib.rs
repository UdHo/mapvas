/// Vim-like command line interface.
pub mod command_line;
/// Contains the configuration for the application.
pub mod config;
/// Contains the main map logic, including drawing, zooming,...
pub mod map;
/// Contains the UI around the map.
pub mod mapvas_ui;
/// Parses different input to draw on the map.
pub mod parser;
/// Performance profiling utilities.
pub mod profiling;
/// Server logic to send data to a running application.
pub mod remote;
/// Rayon thread pool for CPU-bound tile rendering.
pub mod render_pool;
/// Location search functionality with multiple providers.
pub mod search;
/// Task tracking for UI display.
pub mod task_tracker;
