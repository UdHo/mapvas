# Hide layer internals behind a facade

## Problem
`src/mapvas_ui.rs` imports concrete layer types (`ShapeLayer`,
`TileLayer`, `CommandLayer`) and reaches into their internals
(`SubLayerInfo`, `ParameterUpdate`, layer-specific methods). Any
refactor inside a layer ripples into the UI file.

## Direction
Two viable shapes:

**A. Trait-only facade.** Tighten the `Layer` trait so the UI never
needs concrete types. Sidebar interaction (sub-layer visibility,
parameter editing) goes through `Layer::ui_controls(&mut self, ui)` or
similar.

**B. `MapLayer` enum.** A small enum wrapping the known layer variants;
UI matches on the enum but never names internal types.

A is cleaner long-term; B is the smaller diff. Recommend A unless it
forces awkward trait methods.

## Touch points
- `src/map/mapvas_egui/layer/*` — trait surface
- `src/mapvas_ui.rs` — replace type-specific calls with trait calls

## Risk
Medium. Forces the `Layer` trait to grow; do this after todo 01 so the
trait shape reflects the post-split responsibilities.
