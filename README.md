# Aseprite Metadata Editor

A compact Rust/egui editor for individual and bulk `.aseprite` / `.ase` metadata and artwork operations.
Sprite user data is the default target; metadata-only edits preserve artwork bytes without re-encoding.

## Use

Open a folder recursively, or open individual files. Select files in the left tree:
Ctrl-click toggles, Shift-click selects a range, and Ctrl+Shift adds a range.
Arrow keys move selection; Ctrl+arrow moves focus without changing selection;
Shift+arrow extends the range. Space toggles the focused file. Ctrl+A selects
visible files. Text fields retain their normal editing shortcuts.

Edit one file directly, or select several for bulk **Append** (default) / **Override**.
**Ctrl+S** saves the current edits in one step—no preview or apply step required.
The desktop app edits originals in-place with atomic replacement, unique `.bak`
recovery copies and external-change checks. Web Ctrl+S downloads an edited ZIP.

On desktop, double-click a file or use **Open in Aseprite** from its context menu.
Aseprite is preferred; installed LibreSprite is the fallback. Discovery checks
PATH and common installation locations once at startup (Flatpak-only installs
are not detected). Unsaved changes prompt **Save and open**, **Open without
saving**, or **Cancel**. Returning reloads clean external changes; conflicting
local edits are retained and saving remains blocked. This action is not available
in browsers.

**Find/Replace** is a separate tab. **Ctrl+F** or **Ctrl+H** opens it for the current
selection. Choose user data or tag names, then **Replace all**; Ctrl+S saves.
Outside text fields, **Ctrl+Z** undoes and **Ctrl+Shift+Z** redoes document changes.
Text fields retain their own selection and undo shortcuts.

Properties use name/type/value rows with expandable nested values. Bulk edits
change only touched properties, preserving unrelated values. Raw JSON is an
advanced option. **User-data color** is also under Advanced: it is a metadata
marker, not an artwork or palette color.

A small first-frame preview appears below metadata for the focused sprite, using
nearest-neighbor pixels and a transparency checkerboard. It is cached while you
edit. Oversized, tilemap, or unsupported previews show a short unavailable hint;
metadata editing still works.

Unsupported property encodings and unknown chunks are preserved rather than
guessed. Metadata-only edits leave artwork payloads unchanged. Malformed files are rejected, not
silently repaired. Individual tag names, bounds, direction and repeat are editable.

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

## Batch artwork tools

Open **Artwork tools** for the selected files. Changes are staged: **Ctrl+S** uses
native in-place saves with recovery backups, or downloads the existing web ZIP.
Undo/redo works before and after saving; the preview shows staged artwork.

- **Flip sprite** mirrors the complete canvas horizontally on every frame, including
  hidden layers, cel offsets and linked cels—not just the first frame.
- **Expand canvas** adds the entered pixels at the **right** and **bottom**. It does
  not stretch, center, move or crop existing artwork. Zero leaves that axis unchanged.
- **Convert to indexed** preserves exact colors and existing palette entries, adding
  missing colors without quantization. If the 256-entry palette/transparency limit or
  unsupported compositing would change the image, the entire file is skipped and
  listed with its reason; other selected files continue. **Convert to RGBA** resolves
  each frame's palette, including linked cels with animated palettes.
- Select **exactly two** files to **Merge** their animations end-to-end. The displayed
  order can be reversed. The result uses the larger width and height independently,
  retaining pixel coordinates, separate layers and all animation tags (including
  duplicate names). A new numbered merged copy is staged; neither input is replaced.
  Desktop Save creates that copy beside the first input and refuses any existing
  destination. Web Save includes the copy in the downloaded ZIP. Two indexed inputs
  remain indexed when the exact union fits; otherwise the merged copy uses lossless
  RGBA and reports why. Mixed/RGBA inputs produce RGBA with their palette swatches retained.

Artwork transforms safely skip unsupported tilemaps/reference layers. Indexed
conversion supports exact palette alpha for single-image-layer sprites, but skips
multilayer partial alpha or incompatible compositing rather than changing appearance. Merge requires compatible color profiles, global settings
and sprite-level user data; slices, tilesets, external references and unknown chunks
are rejected because their identities cannot safely be relocated. These restrictions
are reported rather than guessed. Metadata editing remains available for those files. See per-file issues before saving; a skipped file
keeps its previous staged changes.

## Build

Install stable Rust. On Debian/Ubuntu, install `libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev
libx11-dev libxi-dev libxrandr-dev libgl1-mesa-dev` first. Native X11 runs need
`libxkbcommon0` and `libxkbcommon-x11-0` at runtime, even when compilation succeeds.
The AppImage bundles these dynamically loaded keyboard libraries and their linked
dependencies; system graphics drivers and glibc remain host requirements.

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
  For the application menu, download `install-linux.sh` and
  `aseprite-metadata-editor.png` from the same release beside your AppImage, then run:

  ```sh
  bash install-linux.sh ./Aseprite-Metadata-Editor-linux-x86_64.AppImage
  ```

  This installs only this app under `${XDG_DATA_HOME:-$HOME/.local/share}`. It uses
  the supplied PNG through an absolute icon path, with matching X11/Wayland
  window identity (`io.github.isaiahpettingill.AsepriteMetadataEditor`). It does not run the AppImage, download anything, use sudo,
  claim file associations, or change defaults. Rerun with a newer AppImage to
  update. On KDE/Wayland, use this installed launcher: embedding a window PNG in
  the AppImage alone does not register the desktop entry with the compositor.
  The installer backs up only a recognized legacy entry from this app; unrelated
  launchers are left alone. If an older entry is pinned, close the old window,
  unpin that entry and pin the newly installed **Aseprite Metadata Editor**.
  No Plasma configuration reset is needed.
  The AppImage also includes hicolor icons, `.DirIcon`, and a `.desktop` entry
  for desktop-integration tools. A standalone template is in `packaging/`.

Build the Linux package with `cargo build --locked --release` then
`packaging/appimage.sh`. Build the Windows installer with NSIS from `packaging/`:
`makensis windows.nsi` after a release build. SHA-256 sums accompany release assets.
The AppImage packager verifies its pinned packaging tool before execution.

The application icon is the user-supplied pixel art, resized without generating
new artwork. See [LICENSE](LICENSE) for the BSD-3-Clause project license.
