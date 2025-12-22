# Usage Guide

MapVas consists of two binaries:

- **mapvas**: Interactive map viewer
- **mapcat**: Command-line tool for piping data to the map

## mapvas

Start `mapvas` to open an interactive map window.

### Navigation

| Key/Action | Description |
|------------|-------------|
| **Mouse Wheel / +/-** | Zoom in and out |
| **Left Click + Drag** | Pan the map |
| **Arrow Keys** | Pan with keyboard |
| **F** | Focus/center all elements |

### Sidebar

| Key/Action | Description |
|------------|-------------|
| **F1 / Ctrl+B** | Toggle sidebar |
| **☰ Button** | Show sidebar (top-left) |
| **✕ Button** | Close sidebar |

### Search & Navigation

| Key/Action | Description |
|------------|-------------|
| **/** | Open search mode (vim-style) |
| **&** | Open filtering mode |
| **:** | Enter command mode |
| **Double-Click** | Navigate nested collections or show popup |
| **Right-Click** | Context menu with element info |
| **L** | Show label at mouse position |

### Data Management

| Key/Action | Description |
|------------|-------------|
| **V** | Paste clipboard content (auto-parsed) |
| **File Drop** | Drag and drop files onto map |
| **Delete / Fn+Delete** | Clear all elements |
| **C** | Copy label text (when popup shown) |
| **S** | Take screenshot |

### Timeline Controls

When temporal data is loaded, a timeline appears at the bottom:

| Key/Action | Description |
|------------|-------------|
| **Ctrl+L** | Toggle interval lock mode |
| **Timeline Handles** | Drag to adjust time interval |
| **Play/Pause** | Control animation |
| **Step Buttons** | Step through time |
| **Speed Slider** | Adjust playback (0.25x-4x) |

## mapcat

Reads geographic data from stdin or files and displays it on mapvas. Spawns mapvas if not running.

### Basic Usage

```bash
# From files
mapcat data.geojson
mapcat routes.gpx points.kml

# From stdin
echo "52.521853, 13.413015" | mapcat
curl 'https://api.example.com/geo' | mapcat
```

### Options

| Option | Description |
|--------|-------------|
| `-p, --parser <name>` | Force specific parser (auto, grep, geojson, json, ttjson, gpx, kml) |
| `-c, --color <color>` | Set color for elements |
| `-i, --invert-coordinates` | Swap lat/lon order |
| `-r, --clear` | Clear map before drawing |
| `-f, --focus` | Zoom to show all elements |
| `-l, --label-pattern <regex>` | Pattern for labels (default: `(.*)`) |
| `--screenshot <file.png>` | Save screenshot |

### Examples

```bash
# Draw colored line
echo "52.5,13.4 50.9,6.9 green" | mapcat

# Clear and focus
mapcat -r -f routes.gpx

# Force parser
mapcat -p geojson ambiguous.json

# Screenshot after loading
mapcat -f --screenshot map.png data.geojson
```

## Directional Data

Points with heading information display directional arrows:

```json
{
  "type": "Feature",
  "geometry": { "type": "Point", "coordinates": [13.4, 52.5] },
  "properties": { "heading": 45.0 }
}
```

Supported fields: `heading`, `bearing`, `direction`, `course` (degrees, 0° = North)
