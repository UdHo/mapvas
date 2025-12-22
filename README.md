# MapVas

[![CI](https://github.com/UdHo/mapvas/actions/workflows/rust.yml/badge.svg)](https://github.com/UdHo/mapvas/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/UdHo/mapvas/graph/badge.svg)](https://codecov.io/gh/UdHo/mapvas)
[![Crates.io](https://img.shields.io/crates/v/mapvas.svg)](https://crates.io/crates/mapvas)
[![Downloads](https://img.shields.io/crates/d/mapvas.svg)](https://crates.io/crates/mapvas)
[![License](https://img.shields.io/crates/l/mapvas.svg)](https://github.com/UdHo/mapvas#license)

A **map** can**vas** for displaying geographic data on [OpenStreetMap](https://openstreetmap.org) tiles.

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

## Documentation

- [Usage Guide](docs/usage.md) - Full controls and features
- [Parsers](docs/parsers.md) - Supported formats (GeoJSON, GPX, KML, etc.)
- [Configuration](docs/config.md) - Tile providers, caching, environment variables

## License

MIT OR Apache-2.0
