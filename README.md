# MapVas

A **map** can**vas** showing [OSM](https://openstreetmap.org) tiles with drawing functionality.
The repo contains two binaries, 
- mapvas: the map window
- mapcat: an equivalent to [cat](https://en.wikipedia.org/wiki/Cat_(Unix)) to draw polygons on the map.

## Setup

Manually:
- Clone this repository.
- [Install Rust](https://rustup.rs).
- `cd mapvas ; cargo install --path .`

Via cargo:
- [Install Rust](https://rustup.rs).
- `cargo install mapvas`

## Usage
### mapvas
Start `mapvas` and a map window will appear.
![mapvas](https://github.com/UdHo/mapvas/blob/master/mapvas.png)
|Functionality | Description | 
|--------------|-------------|
| zoom         | Use the mouse wheel or +/- |
| moving       | Left mouse and dragging or arrow keys |

### mapcat
Mapcat currently reads only input from stdin and reads it line by line and pipes and uses it using various [parser](https://github.com/UdHo/mapvas/tree/master/src/parser).
It then shows the parsed result on a single instance of mapvas, which it spawns if none is running.

#### Grep (default)
This parser greps for coordinates latitude and longitude as float in a line. In addition it supports colors and filling of polygons.
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
- draws a white transparently filled polygon Cologne-Berlin-Amsterdam note that a fill ("transparent" or "solid"):
```
    echo "50.942878, 6.957936 52.521853, 13.413015 52.373520,4.899766 blue transparent" | mapcat 
```
- --invert-coordinates (-i) reverses the order of lat/lon:
```
    echo "13.413015, 52.521853" | mapcat -i 
```

Filling a polyline causes it to be drawn as closed polygon.

#### Random (for performance testing)
Draws a random polyline of a given length. The following command draws a random walk consisting of 20000 polylines of a random length between 1 and 10.
```
    echo "20000" | mapvat -p random
``` 

#### TTJson
Draws routes or ranges from the [TomTom routing api](https://developer.tomtom.com/routing-api/documentation/routing/routing-service).
```
curl 'https://api.tomtom.com/routing/1...' | mapcat -p ttjson -c green
```
