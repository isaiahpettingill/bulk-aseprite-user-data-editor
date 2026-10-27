#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -m) == x86_64 ]] || { echo 'This package target is x86_64.' >&2; exit 1; }
mkdir -p dist
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/AsepriteMetadataEditor.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/lib" "$app/usr/share/licenses/aseprite-metadata-editor"
install -m755 target/release/aseprite-metadata-editor "$app/usr/bin/"
install -m644 packaging/aseprite-metadata-editor.desktop "$app/"
install -m644 assets/icon-256.png "$app/aseprite-metadata-editor.png"
install -m644 LICENSE "$app/usr/share/licenses/aseprite-metadata-editor/LICENSE"
# Bundle linked non-platform libraries. Keep libc, the dynamic loader and GPU
# drivers on the host; shipping those would make hardware compatibility worse.
ldd target/release/aseprite-metadata-editor | awk '$2 == "=>" && $3 ~ /^\// {print $3}' | while IFS= read -r lib; do
  case $(basename "$lib") in
    libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|libresolv.so.*|libGL*|libEGL*|libdrm*) ;;
    *) cp -L "$lib" "$app/usr/lib/" ;;
  esac
done
cat > "$app/AppRun" <<'RUN'
#!/bin/sh
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
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
