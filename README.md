<p align="center">
  <img src="marmoset_logo_red.png" alt="mviewer icon" width="96">
</p>

# Marmoset-mviewer v3.0

Rust-native viewer and exporter for Marmoset `.mview` scenes. One binary provides
an embedded desktop preview and a CLI: glTF/GLB export is available in both,
while selected static meshes can be exported as OBJ/MTL from the GUI.

Maintained by [Jeggery](https://github.com/Jeggery), based on upstream v2.1.2.
See the [changelog](CHANGELOG.md) for release history.

## Getting started

| What you need | Where to go |
| --- | --- |
| Download and launch the desktop app | [Platform downloads and installation](https://Jeggery.github.io/Marmoset-mviewer/#download) |
| Learn the graphical workflow | [Illustrated GUI guide](https://Jeggery.github.io/Marmoset-mviewer/#gui-guide) |
| Get release binaries and notes | [GitHub Releases](https://github.com/Jeggery/Marmoset-mviewer/releases/latest) |
| Export from a terminal | [Command-line reference](#command-line) |
| Understand output files and compatibility | [Export formats](#export-formats) and [known limitations](#known-limitations) |
| Compile or work on the project | [Build from source](#build-from-source) |

Prebuilt packages are available for Windows x64, Linux x64, and macOS Apple
Silicon/Intel. The website covers platform setup and the complete screenshot
walkthrough; this README covers CLI usage, export behavior, and repository development.

## Command line

Running `mviewer` without arguments opens the desktop app. Passing a local file
runs a direct export:

```text
mviewer INPUT [OUTPUT_DIR] [--format gltf|glb]
```

| Argument | Meaning |
| --- | --- |
| `INPUT` | Path to an existing `.mview` file. |
| `OUTPUT_DIR` | Destination folder. If omitted, defaults to `<input-name>_gltf` beside the input file, including for GLB. |
| `--format` | `gltf` (default) or `glb`. OBJ export is available through the GUI. |

Export glTF:

```text
mviewer input.mview output_dir
```

Export a single-file GLB:

```text
mviewer input.mview output_dir --format glb
```

Replace `input.mview` and `output_dir` with your actual paths. Quote paths that
contain spaces. For example, in Windows PowerShell, from the executable's folder:

```powershell
.\mviewer.exe "D:\Models\lens.mview" "D:\Exports\lens" --format glb
```

Use `./mviewer` on Linux/macOS when running the binary from its current directory.
If the executable is on your `PATH`, use `mviewer` as shown above.
The CLI writes directly into `OUTPUT_DIR`. In the GUI, the chosen folder is a
parent folder and exports go into a model-named subfolder.

```text
mviewer --help
mviewer --version
```

Mesh selection and optional scene-content controls are available in the
[desktop app](https://Jeggery.github.io/Marmoset-mviewer/#gui-guide). The CLI
exposes the input path, output directory, and format.

## Export formats

| Format | Available in | Output and intended use |
| --- | --- | --- |
| glTF | GUI and CLI | `.gltf`, `.bin`, and referenced texture files. Keep them together when importing. |
| GLB | GUI and CLI | One `.glb` with referenced images embedded. Convenient for moving a complete model between tools. |
| OBJ/MTL | GUI | Selected static meshes, materials, and optional source textures. |
| FBX | Downstream conversion | Export glTF/GLB first, then convert in a compatible tool such as Blender. |

Use a fresh output folder when switching formats or export options; exporting
does not remove files left by earlier exports.

For Blender, import the generated glTF/GLB directly. Animation, skins, cameras,
and lights are supported by the glTF/GLB exporter where present and included;
see [Known limitations](#known-limitations) before relying on exact scene parity.

<details>
<summary>glTF texture layout and material conversion</summary>

For glTF with textures included, referenced images are placed beside the `.gltf`
file. Untouched archive images, including thumbnails and 1×1 source images,
are backed up in `marmoset-source-texture/`. The glTF does not reference that
backup folder. GLB embeds its referenced images and writes no external texture
files or source-image backup folder.

Marmoset diffuse/specular/gloss materials are converted to core glTF
`pbrMetallicRoughness`: sRGB base color and linear roughness (G) / metallic (B).
The conversion decodes Marmoset's squared color encoding, reconstructs metal
base colors from colored specular reflection, and approximately converts legacy
Blinn-Phong gloss to GGX roughness. Authored tangent frames are exported for
consistent normal mapping.

Constant base color, metallic, and roughness maps become material factors rather
than images. Neutral 1×1 normal maps are omitted; non-neutral normal maps are
retained because core glTF has no equivalent normal factor. Original images
remain unmodified in the backup folder.

</details>

## URL cache and saved settings

URL-loaded models are cached locally, so the toolbar shows a local file path.
The app remembers recent files, recent URLs, and export settings.
When no export folder has been chosen, URL models use `Documents/mviewer-exports`
(or `exports` if Documents is unavailable).

On Windows, model caches are stored under:

```text
%LOCALAPPDATA%\jeggery\marmoset-mviewer\cache\scenes
```

<details>
<summary>Cache behavior and limits</summary>

- Each scene uses `scenes/<model-name>-<8-character-URL-hash>/<model-name>.mview`.
  Hashes are extended if needed after checking the stored source URL for collisions.
- Reopening a URL reuses its cache path and checks ETag/Last-Modified headers.
  Servers without those headers refresh the same cached file.
- Downloads are validated before replacing cached scenes. A failed download
  leaves the existing model intact.
- Least recently used models are removed when the cache exceeds **32 entries**
  or **512 MiB**. The active and newly loaded scenes are protected during cleanup,
  so limits can temporarily be exceeded. Individual downloads over 512 MiB are rejected.
- Settings, WebView data, and caches use this fork's `jeggery/marmoset-mviewer`
  namespace. Previous upstream settings, caches, and older files in Downloads
  are not migrated or deleted.

</details>

## Known limitations

- **Preview and export can look different.** The desktop uses the embedded
  Marmoset runtime. Other viewers render the converted standard glTF materials;
  additive glass, Fresnel controls, environment lighting, and post-processing
  are not reproduced exactly.
- **FBX requires another tool.** Direct FBX export is not implemented.
- **OBJ is static.** Use glTF/GLB when you need skins or animation.

## Build from source

Use a Rust stable toolchain and your platform's native build dependencies:

- **Windows:** an MSVC build toolchain and WebView2 Runtime for the GUI.
- **macOS:** Xcode Command Line Tools.
- **Linux:** GTK 3, WebKitGTK 4.1, and related development libraries. The package
  list used in CI on Ubuntu is:

```bash
sudo apt-get update
sudo apt-get install -y \
  libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev libglib2.0-dev libgdk-pixbuf-2.0-dev libxdo-dev
```

Build and test from the repository root:

```text
cargo build --release --locked
cargo test --release --locked --lib
```

The binary is written to `target/release/mviewer` (`mviewer.exe` on Windows).
To launch the GUI or export a repository sample with Cargo:

```text
cargo run --release --locked
cargo run --release --locked -- test_data/vivfox.mview test_output/vivfox
cargo run --release --locked -- test_data/vivfox.mview test_output/vivfox_glb --format glb
```

<details>
<summary>Release preparation</summary>

The package version is `3.0.0`; the release tag is `v3.0` (`v3.0.0` is also accepted).

1. Update `Cargo.toml`, `Cargo.lock`, and the matching [changelog](CHANGELOG.md) section.
2. Run `cargo test --release --locked --lib` and, with Python 3.11+,
   `python scripts/release_metadata.py --tag v3.0`.
3. Review and commit the changes, then create and push the matching version tag
   to trigger the [release workflow](.github/workflows/release.yml).

The workflow requires an existing tag matching the package version, builds and
tests the release platforms, and uses that version's changelog section as release
notes. Manual runs also require an explicit existing tag. Releases are published
to the repository running the workflow, including forks.

</details>

## Project and format references

Maintained by [Jeggery](https://github.com/Jeggery) in
[Jeggery/Marmoset-mviewer](https://github.com/Jeggery/Marmoset-mviewer), based on
upstream v2.1.2. See the [v3.0 changelog](CHANGELOG.md) for workflow, caching,
and PBR export changes.

The project reads `.mview` archives with a Rust backend and uses the embedded
Marmoset runtime for desktop preview. The older Python/Noesis workflow has been replaced.
For format and parity work, see the [format notes](docs/reverse-engineering/marmoset-js-spec.md)
and [bundled Marmoset JavaScript reference](docs/reverse-engineering/marmoset-d3f745560e47d383adc4f6a322092030.js).
