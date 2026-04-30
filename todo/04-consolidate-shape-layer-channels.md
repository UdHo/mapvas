# Consolidate ShapeLayer channels into one command enum

## Problem
`ShapeLayer` currently owns four mpsc channels (see
`src/map/mapvas_egui/layer/shape_layer.rs:89-105`):
- `geo_tile_sender / receiver` — rasterized tile results
- `vis_sender / receiver` — sub-layer visibility toggles
- `shape_vis_sender / receiver` — per-shape visibility toggles
- the base `MapEvent` channel

Both `external_cmd.rs` and `remote.rs` hold one or more of these senders
directly, which couples them to ShapeLayer internals and makes adding a
new command type a multi-file change.

## Direction
Define a single command enum:

```rust
enum ShapeLayerCommand {
  TileRendered(TileId, RenderedGeometry),
  SetSubLayerVisibility(SubLayerId, bool),
  SetShapeVisibility(ShapeId, bool),
  // future: Highlight, Select, ...
}
```

Replace the four channels with one `mpsc::Sender<ShapeLayerCommand>`.
Expose only the sender to the rest of the app; ShapeLayer drains it on
each frame.

## Pairs with
- todo 01 (split): the visibility module owns its commands.
- todo 12 (visibility shared state): the `Arc<RwLock<HashMap>>` goes
  away once HTTP writes go through this command channel.

## Risk
Medium. Touches HTTP, external command, and render paths. Can be staged:
introduce the enum and forward existing channels, then migrate callers
one by one.
