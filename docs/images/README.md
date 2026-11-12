# GUI tutorial screenshots

The `gui-v3-*.webp` images were captured on 2026-11-12 from the running,
locally built Windows mviewer 3.0.0 application, built with:

```text
cargo build --release --locked --offline
```

Demo scene: https://marmoset.co/wp-content/uploads/viewer/scenes/examplelens.html

The URL was loaded through the GUI. Layer Views and GLB export were exercised
in the same application. The resulting `lens.glb` contained two meshes and
three embedded images, and its GLB v2 header and container length were checked.

Windows Computer Use capture timed out in this session, so screenshots were
captured directly from the live application's WebView2 renderer. They show
actual application content without native window decorations. Images were
resized to 1400 × 900 and encoded as WebP; controls and model content were not
composited or redrawn. The scene itself is not included in this documentation.

The GLB settings screenshot enables only `Include textures`; `Include animations`,
`Include cameras`, and `Include lights` are unchecked and are optional for users.
