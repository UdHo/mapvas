# Split shape_layer.rs

`src/map/mapvas_egui/layer/shape_layer.rs` is ~3329 lines and bundles five
unrelated concerns into a single file/struct.

## Concerns currently mixed
- Geometry rendering / paint pipeline
- Tile cache management for rasterized geometry
- R-tree spatial index
- Temporal filtering (timeline integration)
- HTTP-driven visibility state (read by `remote`)
- Selection + highlight bookkeeping

## Suggested split
- `shape_layer/mod.rs` — `ShapeLayer` struct, `Layer` impl, command dispatch
- `shape_layer/renderer.rs` — paint + tile-cache logic
- `shape_layer/spatial.rs` — R-tree wrapper, `Searchable` impl
- `shape_layer/temporal.rs` — time-window filtering
- `shape_layer/visibility.rs` — sub-layer + per-shape visibility state
- HTTP wiring moves into `remote` (see todo 12)

## Notes
- Currently effectively no unit tests in this file — splitting is a
  prerequisite for adding any.
- Pairs well with todo 04 (channel consolidation): the renderer/visibility
  split is cleaner once there is a single command channel.

## Risk
Medium. Touches a hot path; needs snapshot tests green after each move.
Land as a sequence of mechanical extractions, not one big rewrite.
