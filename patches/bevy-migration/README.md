# Bevy migration patches

Apply these patches in order from the repository root:

1. `001-native-geometry-overlays-and-fills.patch`
2. `002-enable-native-geometry-by-default.patch`
3. `003-native-heading-styles.patch`
4. `004-native-polygon-earcut-triangulation.patch`
5. `005-move-native-viewport-to-map-module.patch`
6. `006-native-vector-tile-super-resolution.patch`
7. `007-cap-native-tile-preloading.patch`
8. `008-native-parent-tile-fallback.patch`
9. `009-native-tile-coordinate-overlay.patch`
10. `010-move-viewport-types-out-of-egui.patch`
11. `011-native-geometry-gizmo-stroke-width.patch`
12. `012-return-map-app-frame-state.patch`
13. `013-share-initial-config.patch`
14. `014-use-native-geometry-gizmo-group.patch`
15. `015-process-command-layer-before-draw.patch`
16. `016-native-command-geometry-snapshots.patch`
17. `017-suspend-native-map-input-for-egui-popups.patch`

Each step was formatted with `cargo fmt --all` and verified with `cargo c`.
The current environment cannot write `.git/index.lock`, so these patches replace the requested commits.
