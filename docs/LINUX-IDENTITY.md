# Linux application identity

The application ID, desktop file basename, StartupWMClass and installed theme
icon name are `io.github.isaiahpettingill.AsepriteMetadataEditor`. The executable
and user-local installation directory remain unchanged. The user-supplied icon
is unchanged.

## Why the old name was unsafe on KDE

KDE KIconLoader searches a theme for the full icon name, then successively
removes dash-separated suffixes before trying the next inherited theme. Its
source explicitly notes that this generic fallback is also applied to app icons:

- [KDE KIconLoader implementation](https://github.com/KDE/kiconthemes/blob/v5.92.0/src/kiconloader.cpp)
- [Current KDE implementation](https://github.com/KDE/kiconthemes/blob/master/src/kiconloader.cpp)
- [Wayland app ID / desktop entry matching](https://wayland.app/protocols/xdg-shell#xdg_toplevel:request:set_app_id)

Thus `aseprite-metadata-editor` can resolve to an active theme's `aseprite` icon
before hicolor's exact editor icon is checked. The reverse-DNS ID has no dashes
and cannot fall back to Aseprite in this way. The installer additionally uses an
absolute Icon path.

The regression test uses real Qt/KDE KIconLoader with an isolated theme containing
a deliberately wrong Aseprite icon, not a reimplementation of lookup. X11 CI
separately checks the actual window class and embedded PNG. These tests do not
claim to reproduce the user's installed Plasma version or pinned-launcher cache.

On Wayland the compositor needs the matching desktop entry installed on the
host; a desktop entry inside a mounted AppImage is not necessarily discoverable.
Use the provided per-user installer, then launch/re-pin its menu entry. Existing
custom launchers are preserved, and no Plasma settings are reset.
