# MapVas

[![CI](https://github.com/UdHo/mapvas/actions/workflows/rust.yml/badge.svg)](https://github.com/UdHo/mapvas/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/UdHo/mapvas/graph/badge.svg)](https://codecov.io/gh/UdHo/mapvas)
[![Crates.io](https://img.shields.io/crates/v/mapvas.svg)](https://crates.io/crates/mapvas)
[![Downloads](https://img.shields.io/crates/d/mapvas.svg)](https://crates.io/crates/mapvas)
[![License](https://img.shields.io/crates/l/mapvas.svg)](https://github.com/UdHo/mapvas#license)

A **map** can**vas** that shows all kind of geospatial data on an interactive map.

![mapvas](https://raw.githubusercontent.com/UdHo/mapvas/main/mapvas.png)

## Install

**Cargo:**

```bash
cargo install mapvas
```

**macOS (Homebrew):**

```bash
brew tap udho/mapvas && brew install mapvas
```

**From source:**

```bash
git clone https://github.com/UdHo/mapvas.git
cd mapvas && cargo install --path .
```

## Quick Start

**View a file:**

```bash
mapcat data.geojson
mapcat routes.gpx
mapcat points.kml
```

**Pipe data:**

```bash
echo "52.521853, 13.413015" | mapcat          # Point in Berlin
curl 'https://api.example.com/geo' | mapcat   # From API
```

Or just drag and drop files onto the map window.

## Heatmap

Render point data as a colour-gradient heatmap instead of individual shapes:

```bash
# All inputs as heatmap
mapcat -H points.geojson

# Mix: one file as heatmap, another normal
mapcat routes.geojson points.geojson:heatmap

# Headless heatmap
mapcat -o map.png -H points.geojson
```

## Headless Rendering

Render maps directly to PNG without opening a window:

```bash
# Render a file
mapcat -o map.png data.geojson

# Multiple files
mapcat -o map.png routes.geojson points.geojson

# Pipe data
cat data.geojson | mapcat -o map.png

# Custom image size
mapcat -o map.png data.geojson --width 3200 --height 2400

# Black background, no map tiles
mapcat -o map.png --no-map data.geojson
```

## Basic Controls

| Action           | Description        |
| ---------------- | ------------------ |
| **Scroll / +/-** | Zoom               |
| **Drag**         | Pan                |
| **V**            | Paste coordinates  |
| **Drop file**    | Load data          |
| **F**            | Focus all elements |
| **/**            | Search             |
| **F1**           | Toggle sidebar     |

## Neovim

[mapvas.nvim](https://github.com/UdHo/mapvas.nvim) provides a Neovim plugin with commands to send
buffers/selections to the map, coordinate highlighting, auto send-on-save, and a layer explorer
sidebar.

## Documentation

- [Usage Guide](docs/usage.md) - Full controls and features
- [Parsers](docs/parsers.md) - Supported formats (GeoJSON, GPX, KML, etc.)
- [Configuration](docs/config.md) - Tile providers, caching, environment variables

## License

MIT OR Apache-2.0
