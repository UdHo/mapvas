# MapVas

A **map** can**vas** showing [OSM](https://openstreetmap.org) tiles with drawing functionality.
The repo contains two binaries,

- mapvas: the map window
- mapcat: an equivalent to [cat](<https://en.wikipedia.org/wiki/Cat_(Unix)>) to draw polygons on the map.

## Setup

Make sure you have the nighly Rust toolchain installed.

- [Install Rust](https://rustup.rs).

Manually:

- Clone this repository.
- `cd mapvas ; cargo install --path .`

Via cargo from [crates.io](https://crates.io/crates/mapvas):

- `cargo install mapvas`

## Usage

### mapvas

Start `mapvas` and a map window will appear.
![mapvas](https://github.com/UdHo/mapvas/blob/master/mapvas.png)
|Functionality | Description |
|--------------|-------------|
| zoom | Use the mouse wheel or +/- |
| focus to drawn elements | f centers the drawn elements |
| moving | Left mouse and dragging or arrow keys |
| paste | pressing v will paste the clipboard into the grep parser |

### mapcat

Mapcat currently reads only input from stdin and reads it line by line and pipes and uses it using various [parser](https://github.com/UdHo/mapvas/tree/master/src/parser).
It then shows the parsed result on a single instance of mapvas, which it spawns if none is running.

#### Grep (default)

This parser greps for coordinates latitude and longitude as float in a line. In addition it supports colors and filling of polygons.

The input can come from a pipe or read via a file.

```
    mapcat <file_with_coordinates>
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

- --invert-coordinates (-i) reverses the order of lat/lon:

```
    echo "13.413015, 52.521853" | mapcat -i
```

- clears all elements from the map.

```
echo "clear" | mapcat
```

The minus -r parameter clears the map before drawing new elements.

```
echo "52.5,12.5" | mapcat -r
```

- --focus (-f) zooms and pans to show all elements on the map.

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

### Advanced usage

#### Offline usage

To cache tile images for future runs set the environment variable `TILECACHE` to an existing directory.

```
    mkdir ~/.tilecache
    export TILECACHE=~/.tilecache
```
#### Different map tile url

To use tiles from a different provider than [openstreetmap] you can set a templated url. The url must contain `{zoom}`, `{x}`, and `{y}`. The tile provider should return tiles in the [pseudo/spherical-mercator projection](https://epsg.io/3857) in a size of 512x512 pixel. Examples:

```
    export MAPVAS_TILE_URL='https://tile.openstreetmap.org/{zoom}/{x}/{y}.png'
    export MAPVAS_TILE_URL='https://api.tomtom.com/map/1/tile/basic/main/{zoom}/{x}/{y}.png?tileSize=512&key=***'
    export MAPVAS_TILE_URL='https://maps.hereapi.com/v3/background/mc/{zoom}/{x}/{y}/png8?size=512&apiKey=***'
```

