# Standardize error handling: thiserror in lib, anyhow in bins

## Problem
Error handling is inconsistent across the crate:
- `src/search.rs` returns `anyhow::Result` everywhere.
- `src/map/tile_renderer.rs` defines a custom `Error` with `thiserror`.
- `src/map/tile_loader.rs` mixes both.

`anyhow` in library code erases error types and forces callers into
string matching for recovery.

## Direction
- Library code (everything in `src/` except `src/bin/`) returns typed
  errors via `thiserror`.
- Define a small set of domain errors: `ParserError`, `TileError`,
  `RenderError`, `RemoteError`. Avoid one-mega-`MapError`.
- Bins (`mapvas`, `mapcat`) and `headless.rs` may use `anyhow` at the
  top level for ergonomic `?` chaining.
- Convert at the boundary with `From` impls / `?`.

## Order of operations
1. Pick the error enums and land them empty / minimal.
2. Convert `search.rs` (smallest blast radius).
3. Convert `tile_loader.rs` to fully use `TileError` (drop the anyhow
   path).
4. Audit remaining `anyhow::Result` in `src/` — convert or justify.

## Risk
Low. Type-driven; the compiler walks you through it.
