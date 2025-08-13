# MapVas

A **map** can**vas** showing [OSM](https://openstreetmap.org) tiles with advanced visualization capabilities including temporal data support, interactive controls, and drawing functionality.

The repo contains two binaries:

- **mapvas**: Interactive map viewer with timeline controls, search, filtering, and drawing
- **mapcat**: Command-line tool equivalent to [cat](<https://en.wikipedia.org/wiki/Cat_(Unix)>) for displaying geographic data on the map

## Setup

Make sure you have Rust installed (stable toolchain recommended).

- [Install Rust](https://rustup.rs).

Manually:

- Clone this repository.
- `cd mapvas ; cargo install --path . --locked`

Via cargo from [crates.io](https://crates.io/crates/mapvas):

- `cargo install mapvas --locked`

Via brew on MacOs:

- `brew tap udho/mapvas && brew install mapvas`

## Usage

### mapvas

Start `mapvas` and a map window will appear.
![mapvas](https://github.com/UdHo/mapvas/blob/master/mapvas.png)

#### Interface Controls

**Navigation & View:**
| Key/Action | Description |
|------------|-------------|
| **Mouse Wheel / +/-** | Zoom in and out |
| **Left Click + Drag** | Pan the map |
| **Arrow Keys** | Pan the map with keyboard |
| **F** | Focus/center all drawn elements |
| **S** | Take screenshot of current view |

**Sidebar & UI:**
| Key/Action | Description |
|------------|-------------|
| **F1 / Ctrl+B** | Toggle sidebar |
| **☰ Button** | Show sidebar |
| **✕ Button** | Close sidebar |

**Search & Navigation:**
| Key/Action | Description |
|------------|-------------|
| **/** | Open search mode (vim-style) |
| **&** | Open filtering mode |
| **:** | Enter command mode |
| **Double-Click** | Navigate nested collections or trigger popups |
| **Right-Click** | Show context menu with element information |
| **L** | Show label at mouse position (Mac-friendly) |

**Timeline Controls (when temporal data loaded):**
| Key/Action | Description |
|------------|-------------|
| **Ctrl+L** | Toggle interval lock mode |
| **Timeline Handles** | Drag to adjust time interval |
| **Play/Pause** | Control temporal animation |
| **Step Buttons** | Step forward/backward through time |
| **Speed Slider** | Adjust playback speed (0.25x-4x) |

**Data Management:**
| Key/Action | Description |
|------------|-------------|
| **V** | Paste clipboard content (auto-parsed) |
| **File Drop** | Drag and drop files onto map |
| **Delete/Fn+Delete** | Clear all elements |
| **C** | Copy label text (when label popup shown) |

### mapcat

Mapcat reads input from stdin or files and uses intelligent auto-parsing to detect the format, or you can specify a parser explicitly. It uses various [parsers](https://github.com/UdHo/mapvas/tree/master/src/parser) to handle different data formats.
It shows the parsed result on a single instance of mapvas, which it spawns if none is running.

#### Auto Parser (default)

The auto parser (default since v0.2.6) intelligently detects input format:

- **File extension detection**: Uses `.geojson`, `.json`, `.gpx`, `.kml`, `.xml` extensions with fallback chains
- **Content analysis**: For stdin/piped data, analyzes content patterns to determine format
- **Smart fallbacks**: TTJson → JSON → GeoJSON → Grep for JSON files, with format-specific chains for others

```bash
# Auto-detect format from file extension
mapcat data.geojson
mapcat routes.gpx
mapcat points.kml

# Auto-detect format from piped content
curl 'https://api.example.com/geojson' | mapcat
cat coordinates.txt | mapcat
```

#### Explicit Parser Selection

You can override auto-detection and specify a parser explicitly:

```bash
# Force specific parser
mapcat -p grep data.txt
mapcat -p geojson data.json
mapcat -p ttjson routes.json
```

Available parsers: `auto` (default), `grep`, `ttjson`, `json`, `geojson`, `gpx`, `kml`

#### Grep Parser

This parser greps for coordinates latitude and longitude as float in a line. In addition it supports colors and filling of polygons.

The input can come from a pipe or from files.

```
    mapcat <files_with_coordinates>
```

Examples:

- draws a point at Berlin Alexanderplatz:

```
    echo "52.521853, 13.413015" | mapcat
```

- draws a line between Berlin and Cologne and a red line between Cologne and Amsterdam:

```
    echo "50.942878, 6.957936 52.521853, 13.413015 green\n 50.942878, 6.957936 52.373520, 4.899766 red" | mapcat
```

- draws a yellow polyline Cologne-Berlin-Amsterdam:

```
    echo "50.942878, 6.957936 random garbage words 52.521853, 13.413015 yellow spaces after the coordinate-comma is not important: 52.373520,4.899766" | mapcat
```

- draws a blue transparently filled polygon Cologne-Berlin-Amsterdam note that a fill ("transparent" or "solid"):

```
    echo "50.942878, 6.957936 52.521853, 13.413015 52.373520,4.899766 blue transparent" | mapcat
```

Filling a polyline causes it to be drawn as closed polygon.

- drawing flexpolylines or google polylines

```
    echo BFoz5xJ67i1B1B7PzIhaxL7Y | mapcat
    echo '_p~iF~ps|U_ulLnnqC_mqNvxq@' | mapcat

```

- --invert-coordinates (-i) reverses the order of lat/lon:

```
    echo "13.413015, 52.521853" | mapcat -i
```

- clears all elements from the map.

```
echo "clear" | mapcat
```

The -r parameter clears the map before drawing new elements.

```
echo "52.5,12.5" | mapcat -r
```

- --label-pattern (-l) defines a label pattern. A near label is shown when right click on the map happens. The label is copied (when shown) via the c key.
  The label requires exactly one capture group to be in the pattern. Default is `"(.*)"` which captures everything.

```
echo "52.4,12.4" | mapcat -l "(.*)"
```

- --focus (-f) zooms and pans to show all elements on the map.

- `--screenshot <file.png>` takes a screenshot of the map. If mapvas is not already running it should probably be combined with `-f`.
  If the path to the file is relative (or the `S` key is used) the screenshot will be saved in the working directory unless `MAPVAS_SCREENSHOT_PATH` is set.

#### Random (for performance testing)

Draws a random polyline of a given length. The following command draws a random walk consisting of 20000 polylines of a random length between 1 and 10.

```
    echo "20000" | mapcat -p random
```

#### TTJson

Draws routes or ranges from the [TomTom routing api](https://developer.tomtom.com/routing-api/documentation/routing/routing-service).

```
curl 'https://api.tomtom.com/routing/1...' | mapcat -p ttjson -c green
```

#### Directional Data Support

MapVas supports displaying directional information for point geometries:

**Heading support:**

- Supported fields: `heading`, `bearing`, `direction`, `course` (in order of precedence)
- Works with GeoJSON, JSON, and TTJson formats
- Heading values in degrees (0° = North, clockwise)
- Configurable arrow styles in settings

```json
// GeoJSON example with heading
{
  "type": "Feature",
  "geometry": { "type": "Point", "coordinates": [13.4, 52.5] },
  "properties": { "heading": 45.0, "label": "Northeast direction" }
}
```

### Advanced usage

#### Configuration

MapVas uses a configuration file located at `~/.config/mapvas/config.json`. The configuration file is automatically created on first run with default settings.

You can also set `MAPVAS_CONFIG` environment variable to use a custom config directory.

Example configuration:

```json
{
  "tile_provider": [
    {
      "name": "OpenStreetMap",
      "url": "https://tile.openstreetmap.org/{zoom}/{x}/{y}.png"
    },
    {
      "name": "TomTom",
      "url": "https://api.tomtom.com/map/1/tile/basic/main/{zoom}/{x}/{y}.png?tileSize=512&key=***"
    }
  ],
  "tile_cache_dir": "/Users/username/.mapvas_tile_cache",
  "commands_dir": "/Users/username/.config/mapvas/commands",
  "search_providers": [
    {"Coordinate": null},
    {"Nominatim": {"base_url": null}}
  ],
  "heading_style": "Arrow"
}
```

#### Tile Caching

Tile caching is enabled by default and tiles are stored in `~/.mapvas_tile_cache`. You can change this location in the config file or disable it by setting `tile_cache_dir` to `null`.

#### Performance Profiling

```bash
# Run with profiling enabled
cargo run --bin mapvas --features profiling

# View profiling data
cargo install puffin_viewer
puffin_viewer --url=http://127.0.0.1:8585
```

#### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MAPVAS_CONFIG` | Custom config directory | `~/.config/mapvas` |
| `MAPVAS_TILE_URL` | Custom tile provider URL (legacy) | Uses config file |
| `MAPVAS_SCREENSHOT_PATH` | Default screenshot save location | Current directory |
