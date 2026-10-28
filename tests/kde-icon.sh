#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
# Isolate config, cache, themes and installer output from the real desktop.
export HOME="$work/home"
export XDG_DATA_HOME="$HOME/data"
export XDG_CONFIG_HOME="$HOME/config"
export XDG_CACHE_HOME="$HOME/cache"
export XDG_DATA_DIRS="$work/system-data"
export XDG_CONFIG_DIRS="$work/system-config"
export XDG_RUNTIME_DIR="$work/runtime"
export QT_QPA_PLATFORM=offscreen
export QT_SCALE_FACTOR=1
export QT_AUTO_SCREEN_SCALE_FACTOR=0
mkdir -p "$HOME" "$XDG_RUNTIME_DIR" "$XDG_DATA_DIRS" "$XDG_CONFIG_DIRS"
chmod 700 "$XDG_RUNTIME_DIR"
printf '#!/bin/sh\nexit 99\n' > "$work/fixture.AppImage"
# The installer only copies the fixture; it must never run it.
bash "$repo/packaging/install-linux.sh" "$work/fixture.AppImage" "$repo/assets/icon-256.png"
cmake -S "$repo/tests/kde-icon" -B "$work/build" -DCMAKE_BUILD_TYPE=Release
cmake --build "$work/build" --parallel 2
"$work/build/kde-icon-regression" "$repo/assets/icon-256.png" \
  "$repo/packaging/io.github.isaiahpettingill.AsepriteMetadataEditor.desktop"
