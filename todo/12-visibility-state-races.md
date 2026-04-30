# Remove shared visibility state between remote and ShapeLayer

## Problem
`ShapeLayer` exposes an `Arc<RwLock<HashMap<...>>>` of shape/sub-layer
visibility info that `remote.rs` reads and writes directly (see
`src/map/mapvas_egui/layer/shape_layer.rs` around line 108). HTTP-driven
visibility toggles can land mid-frame; there is no version/generation
check, so cache invalidation relies on whichever side notices first.

## Direction
Fold visibility writes into the command channel (todo 04):

- `remote` no longer holds the `RwLock`; it holds a
  `mpsc::Sender<ShapeLayerCommand>`.
- HTTP handlers send `SetShapeVisibility` / `SetSubLayerVisibility`
  commands.
- ShapeLayer drains the channel on the next frame and bumps an internal
  generation counter; tile/render caches keyed on generation are
  invalidated naturally.

## Why a generation counter
Even after the channel migration, the renderer needs to know "did
visibility change since I last rasterized this tile?" A monotonic
`u64` per ShapeLayer makes that O(1) and avoids deep equality checks.

## Depends on
- todo 04 (single command channel) lands first.
- Coordinate with todo 01 — visibility lives in the new
  `shape_layer/visibility.rs`.

## Risk
Medium. HTTP is a public-ish API surface (mapcat talks to it); behavior
must stay equivalent. Add an integration test that toggles visibility
over HTTP and asserts the next render reflects it.
