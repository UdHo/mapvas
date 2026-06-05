/// Shared color types.
pub mod color;
/// Contains everything needed to handle coordinates.
pub mod coordinates;
/// Distance calculation utilities for geometries and coordinates.
pub mod distance;
/// Handles geometry.
pub mod geometry_collection;
/// Shared neutral map layer traits and data.
pub mod layer;
/// Stuff to be send to the map client.
pub mod map_event;
/// The map widget.
pub mod mapvas_egui;
pub use mapvas_egui::{Map, MapLayerHolder, ParameterUpdate, timeline_widget::IntervalLock};
/// Map tile functionality.
pub mod tile_loader;
/// Tile rendering abstraction for raster and vector tiles.
pub mod tile_renderer;
/// Shared map viewport data used by UI and Bevy renderers.
pub mod viewport;
