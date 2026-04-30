# Cargo.toml cleanup

Quick wins, all independent of each other.

## Drop `lazy_static`
Code already uses `std::sync::LazyLock` in 26 places. Verify with
`rg "lazy_static!"` — if zero hits, remove the dep.

## Move test-only deps to `[dev-dependencies]`
- `assert_approx_eq` — only used in `src/map/coordinates/transform.rs`
  tests.
- `egui_kittest` — snapshot test harness; should not ship in the main
  dep set.

These currently sit in `[dependencies]` and inflate downstream builds.

## Audit polyline crates
Both `polyline` (0.11.0) and `flexpolyline` (1.0.0) are pulled in.
`src/parser/grep.rs` imports both. Confirm whether both encodings are
actually parsed in the wild; if only flexpolyline is used in practice,
drop the other.

## Replace `once_cell` with `std::sync::OnceLock`
`once_cell` predates `OnceLock`/`LazyLock` stabilization. Migrate uses
and remove the dep.

## Tokio features
`tokio = { features = ["full"] }` pulls in everything. Once todo 02
lands (drop async-std/surf), narrow this to the actual feature set in
use (`rt-multi-thread`, `macros`, `net`, `time`, `sync`, `io-util`,
maybe `signal`).

## Risk
Trivial per item. Land as one PR per change so bisects stay clean.
