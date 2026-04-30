# Replace surf, drop async-std

## Problem
`surf` 2.3.2 is the HTTP client used by `mapcat` and tile fetching. It is
unmaintained (last release 2021) and pulls in `async-std` as its runtime.
The rest of the project uses tokio, so we currently ship two async
runtimes side by side.

## Direction
- Replace `surf` with `reqwest` (tokio-native, maintained, ubiquitous).
- Remove `async-std` and `surf-governor` from `Cargo.toml`.
- Re-evaluate rate limiting: `tower::limit` or a small custom semaphore
  can replace `surf-governor`.

## Touch points
- `src/bin/mapcat/sender.rs` — client requests
- `src/map/tile_loader.rs` — tile fetches
- Any other `use surf` sites (`rg "surf::"`).

## Risk
Low–medium. API shapes are similar; the main work is verifying retry /
timeout / rate-limit parity against the tile provider.

## Side benefits
- One async runtime → smaller binary, less lock contention surface.
- Unblocks dropping `async-std` from the dep tree entirely.
