# Configuration

## Config File

MapVas stores configuration at `~/.config/mapvas/config.json` (created on first run).

```json
{
  "tile_provider": [
    {
      "name": "OpenStreetMap",
      "url": "https://tile.openstreetmap.org/{zoom}/{x}/{y}.png",
      "tile_type": "Raster"
    },
    {
      "name": "OpenFreeMap Vector",
      "url": "https://tiles.openfreemap.org/planet/20251231_001001_pt/{zoom}/{x}/{y}.pbf",
      "tile_type": "Vector"
    }
  ],
  "tile_cache_dir": "/Users/username/.mapvas_tile_cache",
  "commands_dir": "/Users/username/.config/mapvas/commands",
  "search_providers": [
    { "Coordinate": null },
    { "Nominatim": { "base_url": null } }
  ],
  "heading_style": "Arrow"
}
```

## Tile Providers

Add multiple tile providers in the config. Switch between them in the sidebar settings.

**Fields:**
- `name` - Display name for the provider
- `url` - Tile URL template
- `tile_type` - Either `"Raster"` (default) or `"Vector"`

**URL placeholders:**
- `{zoom}` - Zoom level
- `{x}` - Tile X coordinate
- `{y}` - Tile Y coordinate

**Tile Types:**
- `Raster` - PNG/JPEG tiles (default, used by OpenStreetMap)
- `Vector` - MVT/PBF tiles (Mapbox Vector Tiles, rendered client-side with OSM-style colors)

## Tile Caching

Tiles are cached locally for offline use and performance.

- Default location: `~/.mapvas_tile_cache`
- Set `tile_cache_dir` to `null` to disable
- Or set custom path in config

## Search Providers

Configure search providers for the `/` search command:

- **Coordinate**: Direct coordinate input (always enabled)
- **Nominatim**: OpenStreetMap search (default)
- **Custom**: Add your own geocoding API

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MAPVAS_CONFIG` | Config directory | `~/.config/mapvas` |
| `MAPVAS_TILE_CACHE_DIR` | Tile cache directory | `~/.mapvas_tile_cache` |
| `MAPVAS_TILE_URL` | Tile URL (legacy, use config) | - |
| `MAPVAS_SCREENSHOT_PATH` | Screenshot save location | Current directory |

## Performance Profiling

Build with profiling support:

```bash
cargo run --bin mapvas --features profiling
```

View profiling data:

```bash
cargo install puffin_viewer
puffin_viewer --url=http://127.0.0.1:8585
```
