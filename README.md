# Aseprite Metadata Editor

A compact Rust/egui editor for individual and bulk `.aseprite` / `.ase` metadata.
Sprite user data is the default target; artwork is not decoded or re-encoded.

## Use

Open a folder recursively, or open individual files. Select files in the left tree:
Ctrl-click toggles, Shift-click selects a range, and Ctrl+Shift adds a range.
Arrow keys move selection; Ctrl+arrow moves focus without changing selection;
Shift+arrow extends the range. Space toggles the focused file. Ctrl+A selects
visible files. Text fields retain their normal editing shortcuts.

Choose the metadata scope on the right. **Append** is the default; **Override**
replaces the selected field. Preview changes, review the affected files, then
apply them to the in-memory documents. Save selected files with Ctrl+S; Ctrl+Z
undoes an applied metadata edit before another save. The desktop app retains
unique `.bak` recovery copies beside each saved file and refuses to overwrite
files changed externally after opening. A partial failure is reported per file.

User data text, typed properties and tag metadata have separate tabs. Unsupported
property encodings are preserved rather than guessed. Unknown chunks and image
payloads remain unchanged. Malformed files are rejected, not silently repaired.
Use the Tags tab for bulk find/replace in tag names or individual tag edits.

The web version starts with **Open files**: select one or more `.ase` / `.aseprite`
files. Only matching files are read locally; nothing is uploaded to a server.
**Open folder** remains optional and filters before reading file contents.
Opening adds files without replacing existing edits; cancelling changes nothing.
Same-name files receive numbered names, so ZIP entries never overwrite one another.
The web version downloads edited
files as a ZIP, keeping relative paths. **It does not overwrite original files or
write directly to a directory.** Folder picking depends on browser support;
individual-file picking is the fallback. Download your edits before closing the
tab. Duplicate or unsafe ZIP paths are rejected rather than overwritten.

## Build

Install stable Rust. On Debian/Ubuntu, install `libxkbcommon-dev libwayland-dev
libx11-dev libxi-dev libxrandr-dev libgl1-mesa-dev` first.

```sh
cargo run --locked --release
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```

For the browser build:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
trunk serve
```

File processing stays on the device. No server receives your sprite files.

## Distribution

- CI checks Linux, Windows and the WebAssembly build on pull requests.
- `main` publishes the web build through GitHub Pages. In repository Settings →
  Pages, select **GitHub Actions** as the source before the first deployment.
- A merge to `main` publishes desktop assets when the Cargo package version has
  no existing tag. Bump that version for a new release; unchanged versions are
  skipped. Existing tags pointing elsewhere are never replaced. Version-tag and
  manual builds remain supported; manual non-main branch builds produce artifacts
  without publishing.
- Windows: per-user NSIS installer, no administrator access required. The installer includes a destination-folder chooser and defaults to your
  local application-data directory; choose a writable folder. Installers
  are **unsigned** and may trigger Windows SmartScreen/publisher warnings.
- Linux: x86_64 AppImage built on Ubuntu 22.04 (glibc ≥ 2.35), requiring working
  graphics drivers. Run `chmod +x *.AppImage`, then open it. Without FUSE, launch
  with `APPIMAGE_EXTRACT_AND_RUN=1 ./Aseprite-Metadata-Editor-linux-x86_64.AppImage`.
  The package contains a `.desktop` entry; a standalone entry is also provided in
  `packaging/`. It does not silently claim file associations or change defaults.

Build the Linux package with `cargo build --locked --release` then
`packaging/appimage.sh`. Build the Windows installer with NSIS from `packaging/`:
`makensis windows.nsi` after a release build. SHA-256 sums accompany release assets.
The AppImage packager verifies its pinned packaging tool before execution.

The application icon is the user-supplied pixel art, resized without generating
new artwork. See [LICENSE](LICENSE) for the BSD-3-Clause project license.
