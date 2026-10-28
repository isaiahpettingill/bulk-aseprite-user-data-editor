#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -m) == x86_64 ]] || { echo 'This package target is x86_64.' >&2; exit 1; }
mkdir -p dist
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/AsepriteMetadataEditor.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/lib" "$app/usr/share/licenses/aseprite-metadata-editor" \
  "$app/usr/share/icons/hicolor/256x256/apps" "$app/usr/share/applications"
install -m755 target/release/aseprite-metadata-editor "$app/usr/bin/"
install -m644 packaging/aseprite-metadata-editor.desktop "$app/"
install -m644 assets/icon-256.png "$app/usr/share/icons/hicolor/256x256/apps/aseprite-metadata-editor.png"
ln -s usr/share/icons/hicolor/256x256/apps/aseprite-metadata-editor.png "$app/aseprite-metadata-editor.png"
ln -s aseprite-metadata-editor.png "$app/.DirIcon"
install -m644 packaging/aseprite-metadata-editor.desktop "$app/usr/share/applications/"
install -m644 LICENSE "$app/usr/share/licenses/aseprite-metadata-editor/LICENSE"
# Bundle linked non-platform libraries. Keep libc, the dynamic loader and GPU
# drivers on the host; shipping those would make hardware compatibility worse.
copy_library() {
  case $(basename "$1") in
    libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|libresolv.so.*|ld-linux*|libGL*|libEGL*|libdrm*) ;;
    *) cp -L "$1" "$app/usr/lib/" ;;
  esac
}
bundle_dependencies() {
  local linked
  linked=$(ldd "$1")
  if grep -q '=> not found' <<< "$linked"; then
    printf 'Missing package dependencies for %s:\n%s\n' "$1" "$linked" >&2
    exit 1
  fi
  while IFS= read -r lib; do
    [[ -z "$lib" ]] || copy_library "$lib"
  done < <(awk '$2 == "=>" && $3 ~ /^\// {print $3}' <<< "$linked")
}
bundle_dependencies target/release/aseprite-metadata-editor
# xkbcommon-dl 0.4.2 loads these at runtime, so ldd on the app cannot see
# them. Include their complete linked closure (including libxcb-xkb), not libc.
for soname in libxkbcommon.so.0 libxkbcommon-x11.so.0; do
  library=$(ldconfig -p | awk -v name="$soname" '$1 == name && /x86-64/ && !found {print $NF; found=1}')
  [[ -n "$library" && -f "$library" ]] || {
    printf 'Missing %s; install libxkbcommon0 and libxkbcommon-x11-0 before packaging.\n' "$soname" >&2
    exit 1
  }
  copy_library "$library"
  bundle_dependencies "$library"
  # Support both loader candidates without requiring development packages.
  ln -sf "$soname" "$app/usr/lib/${soname%.0}"
done
cat > "$app/AppRun" <<'RUN'
#!/bin/sh
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# Keep the user's loader environment for external editor launches. Preserve
# unset versus empty; neither is the same as our private AppImage library path.
if [ "${LD_LIBRARY_PATH+x}" = x ]; then
  export ASEPRITE_METADATA_ORIGINAL_LD_LIBRARY_PATH="$LD_LIBRARY_PATH"
else
  unset ASEPRITE_METADATA_ORIGINAL_LD_LIBRARY_PATH
fi
export ASEPRITE_METADATA_APPIMAGE=1
export LD_LIBRARY_PATH="$HERE/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$HERE/usr/bin/aseprite-metadata-editor" "$@"
RUN
chmod +x "$app/AppRun"
url=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
curl --fail --location --retry 3 "$url" -o "$work/appimagetool"
echo 'ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0  '"$work/appimagetool" | sha256sum --check
chmod +x "$work/appimagetool"
# Reuse the verified tool's own embedded runtime, avoiding an unpinned runtime download.
offset=$("$work/appimagetool" --appimage-offset)
head -c "$offset" "$work/appimagetool" > "$work/runtime"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$work/appimagetool" --runtime-file "$work/runtime" "$app" dist/Aseprite-Metadata-Editor-linux-x86_64.AppImage
