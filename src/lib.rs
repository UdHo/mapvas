/// Contains the configuration for the application.
pub mod config;
/// Contains the main map logic, including drawing, zooming,...
pub mod map;
/// Contains the UI around the map.
pub mod mapvas_ui;
/// Parses different input to draw on the map.
pub mod parser;
/// Server logic to send data to a running application.
pub mod remote;
/// Location search functionality with multiple providers.
pub mod search;
