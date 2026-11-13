# Changelog

## 3.0.1 - 2026-11-13

### glTF and GLB export

- Fix invalid empty node children arrays and incorrect bufferView targets in
  animated exports.
- Correct initial skinning poses, scene scaling, animation timing, and clip
  duration. Preserve constant transform components when switching clips and
  support up to eight skin influences per vertex without discarding weights.
- Fix emissive color conversion and intensity handling to prevent models from
  appearing white or excessively bright.
- Preserve emissive texture resolution, atlas coordinates, and UV tiling without
  adding materials or splitting mesh primitives.
- Preserve camera descriptions when merging runtime snapshots, including FOV,
  clipping planes, and static transforms. Keep animated camera transforms intact.

### OBJ and mesh selection

- Correct OBJ texture orientation so source textures display properly without
  manual UV flipping.
- Fix selective OBJ exports that could produce empty or incomplete geometry.
- Reject empty and out-of-range mesh selections before writing export files.

### Scene loading and reliability

- Return errors for malformed archive lengths, duplicate entries, truncated
  keyframes, invalid mesh counts and indices, and cyclic animation references.
- Bound archive loading and decompression: default limits are 512 MiB per input
  or decoded entry, 1 GiB of total decoded data, and 65,536 entries. Library
  callers can configure these limits.

### Documentation

- Refresh the project website and README, and add an illustrated GUI guide with
  platform installation and export instructions.
- Fix validation of relative homepage links in the GitHub Pages build.

### Packaging

- Generate release icons explicitly before packaging so cached builds and Cargo
  output directory layouts cannot leave release bundles without required icons.

## 3.0.0 - 2026-11-11

First major release of [Jeggery/Marmoset-mviewer](https://github.com/Jeggery/Marmoset-mviewer),
based on upstream v2.1.2.

### Desktop workflow

- Added a toolbar clear button to unload the current scene and clear its path.
- Added independent clear buttons for the local-file and URL source inputs.
- Made the current model path selectable and copyable, with the filename visible
  when a newly loaded path is wider than the field.
- Gave this fork its own settings, WebView and cache namespace; older application
  settings and caches are not migrated.

### URL loading

- Store URL models in application-managed caches instead of creating numbered
  downloads in the user's Downloads folder.
- Use readable model-name directories with short URL hashes and collision checks.
- Revalidate cached models using ETag/Last-Modified, validate downloads before
  replacement, and prune older entries above 32 models or 512 MiB while protecting
  active models. Individual downloads are limited to 512 MiB.

### glTF and GLB export

- Reconstruct core Metallic/Roughness materials from Marmoset diffuse, RGB
  reflectivity and gloss maps, including metal base colors and color-space handling.
- Convert legacy Blinn-Phong gloss approximately to GGX roughness and export
  authored tangent frames for more consistent normal mapping.
- Export constant color, metallic and roughness maps as numeric material factors;
  omit neutral 1x1 normal maps.
- Write a single self-contained file for GLB, with no external texture files.
- Put all glTF image dependencies beside the glTF and keep untouched archive
  images in `marmoset-source-texture/`; resolve flattened filename conflicts.
- Correct embedded-image bufferView usage and add export/material/cache regression
  coverage.

### Distribution and remaining limits

- Publish from this fork with version/tag validation and a standalone Windows x64
  EXE alongside platform archives. Fix missing macOS icon sizes.
- Core metal/rough conversion remains approximate for Marmoset-specific additive
  glass, Fresnel behavior, lighting and post-processing.
- Existing animated exports can still contain invalid empty `children` arrays;
  that separate animation issue is not fixed in this release.

## 2.1.2 - 2026-04-03

- added visible progress/loading feedback for URL-based scene loading in the desktop app
- switched release publishing to GitHub CLI to avoid deprecated Node 20 release actions

## 2.1.1 - 2026-04-03

- refreshed release docs/media assets
- added the animated preview GIF to the published docs assets

## 2.1.0 - 2026-04-03

- unified the project into a single `mviewer` binary: no arguments launch the GUI, arguments run the CLI
- rebuilt the desktop app around an embedded Marmoset preview using `tao + wry`
- added direct URL loading in the GUI, including ArtStation artwork page resolution
- added GUI export options for glTF, GLB, and OBJ/MTL
- added CLI `--format gltf|glb`
- added persistent app settings, recent files/URLs, and appdata-backed WebView storage
- updated icons, packaging, CI, and release assets for the single-binary desktop app

## 2.0.0 - 2026-04-02

- replaced the legacy Python and Noesis workflow with a Rust-native `.mview` exporter
- added glTF export for static and animated scenes
- added runtime sidecar export with generated `viewer.html`
- added sample animated fixtures and updated reverse-engineering references
- added CI, tagged release packaging, and GitHub Pages site
