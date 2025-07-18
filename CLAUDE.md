# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Building and Installation
- `cargo build` - Build the project
- `cargo build --release` - Build optimized release version
- `cargo install --path . --locked` - Install locally from source
- `cargo run --bin mapvas` - Run the map viewer
- `cargo run --bin mapcat` - Run the command line tool

### Testing and Quality
- `cargo test` - Run all tests
- `cargo clippy` - Run linter (configured with pedantic warnings)
- `cargo fmt` - Format code (uses 2 spaces for tabs)

## Project Architecture

MapVas is a Rust-based map visualization tool with two main binaries:

### Core Components
- **mapvas**: GUI map viewer using egui/eframe with drawing capabilities
- **mapcat**: CLI tool that pipes coordinate data to mapvas via HTTP server

### Module Structure
- `src/lib.rs` - Main library with module declarations
- `src/config.rs` - Application configuration
- `src/map/` - Core map functionality (coordinates, geometry, tiles, egui integration)
- `src/parser/` - Input parsers (grep, JSON, TomTom API)
- `src/remote.rs` - HTTP server for mapcat communication
- `src/mapvas_ui.rs` - UI wrapper around map components

### Key Architecture Patterns
- **Client-Server**: mapcat communicates with mapvas via HTTP on localhost
- **Parser Pipeline**: Multiple input formats supported through parser modules
- **Layer System**: Map uses drawable layers (tiles, shapes) with screenshot capability
- **Coordinate Systems**: Handles various coordinate formats and transformations

### Technology Stack
- **GUI**: egui + eframe for cross-platform UI
- **HTTP**: axum for server, surf for client requests
- **Graphics**: Custom tile loading with OSM/custom tile providers
- **Parsing**: regex for coordinate extraction, serde for JSON

## Environment Variables
- `TILECACHE` - Directory for caching map tiles
- `MAPVAS_TILE_URL` - Custom tile provider URL template
- `MAPVAS_SCREENSHOT_PATH` - Default screenshot save location

## Rust Configuration
- Uses Rust 1.87.0 toolchain (specified in rust-toolchain.toml)
- Code formatting: 2 spaces, edition 2021
- Clippy configured for pedantic warnings with some exceptions