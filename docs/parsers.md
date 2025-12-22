# Parsers

MapVas uses intelligent auto-parsing to detect input formats. You can also specify a parser explicitly with `-p <parser>`.

## Auto Parser (default)

The auto parser detects format by:

1. **File extension**: `.geojson`, `.json`, `.gpx`, `.kml`, `.xml`
2. **Content analysis**: For stdin/piped data, analyzes patterns

## GeoJSON

Standard GeoJSON format with optional styling properties.

> **Note:** GeoJSON uses `[longitude, latitude]` order, not `[latitude, longitude]`.

```bash
mapcat data.geojson
cat data.geojson | mapcat -p geojson
```

**Supported geometry types:** Point, LineString, Polygon, MultiPoint, MultiLineString, MultiPolygon, GeometryCollection, FeatureCollection

**Styling properties:**

```json
{
  "type": "Feature",
  "geometry": { "type": "Point", "coordinates": [13.4, 52.5] },
  "properties": {
    "stroke": "#ff0000",
    "stroke-width": 2,
    "stroke-opacity": 0.8,
    "fill": "#00ff00",
    "fill-opacity": 0.5,
    "label": "My Point",
    "heading": 45.0
  }
}
```

**Temporal data:**

```json
{
  "properties": {
    "timestamp": "2024-01-15T10:30:00Z",
    "time": 1705315800
  }
}
```

## GPX

GPS Exchange Format for tracks, routes, and waypoints.

```bash
mapcat track.gpx
```

Supports temporal data from GPX timestamps.

## KML

Keyhole Markup Language (Google Earth format).

```bash
mapcat places.kml
```

Supports nested folders, styles, and temporal data.

## JSON

Simple JSON coordinate format.

```bash
echo '{"lat": 52.5, "lon": 13.4}' | mapcat -p json
echo '{"latitude": 52.5, "longitude": 13.4, "color": "red"}' | mapcat -p json
```

## TTJson

TomTom API response format.

```bash
curl 'https://api.tomtom.com/routing/1/...' | mapcat -p ttjson
```

## Grep

Extracts coordinates from any text using pattern matching.

```bash
# Simple coordinates
echo "52.521853, 13.413015" | mapcat

# With color
echo "52.5,13.4 50.9,6.9 green" | mapcat

# Filled polygon
echo "52.5,13.4 50.9,6.9 48.1,11.5 blue transparent" | mapcat

# Polylines (flex/google)
echo "BFoz5xJ67i1B1B7PzIhaxL7Y" | mapcat
echo "_p~iF~ps|U_ulLnnqC_mqNvxq@" | mapcat
```

**Colors:** red, green, blue, yellow, orange, purple, cyan, magenta, white, black, gray, or hex (#ff0000)

**Fill modes:** `transparent`, `solid`

## Random

For performance testing - generates random polylines.

```bash
echo "20000" | mapcat -p random
```

## Clear

Special command to clear the map.

```bash
echo "clear" | mapcat
```
