# Unify raster + vector tile renderers behind a trait

## Problem
`src/map/tile_renderer/raster.rs` and `src/map/tile_renderer/vector.rs`
both turn `(tile bytes, zoom, coords)` into an image, but they share no
trait. `tile_layer.rs` (~999 lines) branches on the renderer kind in
several places.

## Direction
```rust
pub trait TileRenderer {
  fn render(&self, input: TileInput) -> Result<TileImage, RenderError>;
}
```

- Implement for `RasterRenderer` and `VectorRenderer`.
- `tile_layer.rs` holds `Box<dyn TileRenderer>` (or generic) and stops
  caring about the variant.
- Style config stays renderer-specific; expose via an associated type
  or a `configure(&mut self, &StyleConfig)` method.

## Payoff
- `tile_layer.rs` shrinks and stops being the seam between two
  renderers.
- Future renderers (e.g., a debug grid renderer, or a heatmap-only
  renderer) become drop-in.

## Risk
Low–medium. Mostly mechanical; the tricky bit is whether the vector
renderer's heavier config can hide behind the same trait without
`Any`-style downcasts.
