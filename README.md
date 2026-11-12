  <p align="center">
    <img src="marmoset_logo_red.png" alt="mviewer icon" width="128">
  </p>

# Marmoset-mviewer v3.0

Rust-native exporter for Marmoset `.mview` scenes.

Maintained by [Jeggery](https://github.com/Jeggery) in
[Jeggery/Marmoset-mviewer](https://github.com/Jeggery/Marmoset-mviewer).
This fork builds on upstream v2.1.2. The v3.0 release adds workflow, caching and
PBR export fixes. See [CHANGELOG.md](CHANGELOG.md).

Project preview: [Jeggery.github.io/Marmoset-mviewer](https://Jeggery.github.io/Marmoset-mviewer/)
(available after GitHub Pages deployment).

The application/package version is `3.0.0`; the release tag for this milestone is
`v3.0` (`v3.0.0` is also accepted by the release workflow).

![vivfox export preview](docs/images/sample.gif)

## Download

Prebuilt binaries are published from GitHub Actions releases:

- Windows x64
- Linux x64
- macOS arm64
- macOS x64

Latest release: [`latest`](https://github.com/Jeggery/Marmoset-mviewer/releases/latest)

Release asset names for v3.0:

- Windows 64-bit, direct executable: `mviewer-v3.0-windows-x64.exe`
- Windows 64-bit, archive: `mviewer-v3.0-windows-x64.zip`
- Linux 64-bit: `mviewer-v3.0-linux-x64.tar.gz`
- macOS Apple Silicon: `mviewer-v3.0-macos-arm64.tar.gz`
- macOS Intel: `mviewer-v3.0-macos-x64.tar.gz`

The standalone Windows EXE needs no extraction. Windows GUI preview requires the
Microsoft WebView2 Runtime. Asset names use the actual release tag.

After extracting a release archive, run:

```text
mviewer --help
```

## Feature Parity

### Marmoset `.mview` / JS Parity

- [x] `.mview` archive parsing
- [x] `scene.json` parsing
- [x] static glTF scene export
- [x] animated glTF scene export
- [x] skinning export
- [x] camera export
- [x] light export
- [x] primary UV export
- [x] corrected UV orientation parity
- [x] packed normal decoding
- [x] vertex color export
- [x] material texture extraction
- [x] alpha texture merge
- [x] metallic-roughness texture packing
- [x] embedded Marmoset runtime preview in the desktop GUI
- [x] direct OBJ export in the desktop GUI
- [ ] full plain-glTF parity for all Marmoset-only runtime behavior
- [x] `.glb` output
- [ ] direct `FBX` export

### Project Features

- [x] Rust-native command-line tool
- [x] Native desktop GUI with embedded Marmoset preview
- [x] Windows, Linux, and macOS builds
- [x] GitHub Actions CI and release packaging
- [x] GitHub Pages project site
- [x] sample animated fixtures in-repo

## Quick Start

```powershell
mviewer input.mview output_dir
```

Example:

```powershell
mviewer test_data\vivfox.mview test_output\vivfox
```

This writes for glTF:

- `<name>.gltf`
- `<name>.bin`
- generated PBR textures that contain spatial detail
- copies of referenced original images (such as normal maps), placed beside the
  `.gltf` file; every image URI points to a file in that same directory
- `marmoset-source-texture/`, containing the original archive images (including
  thumbnails and 1x1 source images), when textures are included; this folder is
  an original-image backup and is not referenced by the glTF

Constant base color, metallic and roughness maps are exported as material factors
instead of images, including 1x1 maps. Neutral 1x1 normal maps are omitted.
Non-neutral normal maps are retained because core glTF has no equivalent normal
factor. Original images are kept unmodified in the source folder for reference.

For GLB:

```powershell
mviewer test_data\vivfox.mview test_output\vivfox_glb --format glb
```

This writes:

- `<name>.glb`

All referenced images are embedded in the GLB; no texture files or source-image
folder are written. Exporting does not delete files already in the destination,
so use a fresh folder if it contains images from an earlier export.

## GUI Workflow

Launch the desktop app with:

```text
mviewer
```

The current GUI supports:

- opening a local `.mview` file
- loading a supported URL directly, including ArtStation artwork pages that expose a public `.mview`
- previewing the scene through the embedded Marmoset runtime
- exporting to glTF, GLB, or OBJ/MTL
- choosing export options per format
- remembering the last export defaults and recent files

URL-loaded models are stored in the application's local cache directory under
`scenes/<model name>-<8-character URL hash>/<model name>.mview`, rather than the Downloads folder. Reopening
the same model URL reuses its path and checks the server's ETag/Last-Modified
headers for updates; servers without these headers refresh the same cached file.
Downloads are validated before replacing cached models, so a failed download
does not overwrite an existing model.
If a short hash collides, it is extended after checking the stored source URL.

The cache automatically removes the least recently used models when it exceeds
32 entries or 512 MiB. The current model and the newly loaded model are protected
during cleanup, so these limits can temporarily be exceeded. Individual downloads
larger than 512 MiB are rejected. Previously downloaded files in Downloads are
left untouched. When no export folder has been chosen, URL models default to
`Documents/mviewer-exports` (or `exports` if Documents is unavailable).

The toolbar model path supports mouse selection and copying. Clear buttons in
Choose Source only clear the corresponding input; the toolbar clear button
unloads the current model.

Settings, WebView data and model caches use this fork's `jeggery/marmoset-mviewer`
namespace (Windows model cache: `%LOCALAPPDATA%\jeggery\marmoset-mviewer\cache\scenes`).
Previous upstream settings and caches are not migrated or deleted.

## Output Formats

mviewer exports `.mview` scenes to glTF and GLB directly.

The desktop GUI can also export selected meshes to OBJ/MTL with source textures.

If you need other formats:

- `mview to fbx`: export to glTF first, then convert with Blender or another downstream tool
- `mview to obj`: export to glTF first, then convert if you only need static geometry/materials
- `mview to blender`: import the generated glTF into Blender

glTF is the primary interchange format because it preserves modern materials, skins, animation, cameras, and lights better than the old script-based workflow.

Material export bakes Marmoset's diffuse/specular/gloss textures into standard
glTF `pbrMetallicRoughness`: sRGB base color and linear roughness (G) / metallic
(B). Colored specular reflection contributes to the reconstructed metal base
color. Marmoset's squared color encoding is decoded before conversion, and
legacy Blinn-Phong gloss is approximately converted to GGX roughness. Authored
tangent frames are exported for consistent normal mapping across viewers.
This is an approximation for materials outside the core metal/rough model;
Marmoset-specific additive glass, Fresnel controls, environment lighting and
post-processing are not reproduced exactly by ordinary glTF materials.

## Build From Source

Requirements:

- Rust stable toolchain

Build:

```powershell
cargo build --release
```

Run GUI:

```powershell
cargo run
```

Run CLI:

```powershell
cargo run --release -- <input.mview> [output_dir]
```

## Workflow Notes

mviewer is a Rust-native Marmoset `.mview` viewer, exporter, and converter focused on `.mview` to glTF export.

If you are looking for an `mview viewer`, `mview editor`, `mview to gltf`, `mview to fbx`, or `mview to obj` workflow, the current project path is:

`.mview` -> `glTF` -> optional conversion to `FBX`, `OBJ`, Blender, or other formats

This repository no longer uses the old Python extractors or the Noesis plugin as its primary workflow. The current implementation reads `.mview` archives directly, exports with a Rust backend, and uses the embedded Marmoset runtime in the desktop GUI for preview parity.

## Current Limitations

- stock third-party glTF viewers only see the standard glTF export
- direct `FBX` export is not implemented yet
- some animated exports still contain empty `children` arrays that fail strict
  glTF validation; the static lens verification does not establish animation parity

## Release Preparation

1. Update the Cargo package version, lockfile and matching changelog section.
2. Run `cargo test --release --locked --lib` and
   `python scripts/release_metadata.py --tag v3.0` (Python 3.11+).
3. After reviewing and committing the changes, create and push the version tag
   on that commit to trigger `.github/workflows/release.yml`.

The workflow requires an existing tag matching the package version, builds and
tests the release platforms, and uses the current changelog section as the release
notes. Manual runs require an explicit existing version tag; selecting a branch
does not publish a branch-named release. The workflow publishes to the repository
that runs it, including when this project is forked.

## Reverse Engineering References

These files are still kept in the repo as format references:

- `docs/reverse-engineering/marmoset-d3f745560e47d383adc4f6a322092030.js`

The newer bundled Marmoset JavaScript is the primary reference for format and runtime parity work.
