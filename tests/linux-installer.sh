#!/usr/bin/env bash
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
# Never touch the actual user home or launch an untrusted AppImage.
export HOME="$work/home with spaces"
export XDG_DATA_HOME="$HOME/data 100% with \"quotes\" and \$dollar and \`tick\` and \\slash"
mkdir -p "$HOME/downloads"
source_app="$HOME/downloads/editor fixture.AppImage"
printf '#!/bin/sh\nprintf "fixture ran" > "$HOME/launched"\n' > "$source_app"
export source_app repo
bash "$repo/packaging/install-linux.sh" "$source_app" "$repo/assets/icon-256.png"
app="$XDG_DATA_HOME/aseprite-metadata-editor/Aseprite-Metadata-Editor.AppImage"
icon="$XDG_DATA_HOME/icons/hicolor/256x256/apps/aseprite-metadata-editor.png"
desktop="$XDG_DATA_HOME/applications/aseprite-metadata-editor.desktop"
test -x "$app"
cmp "$source_app" "$app"
cmp "$repo/assets/icon-256.png" "$icon"
! test -e "$HOME/launched"
python3 - "$desktop" "$app" "$icon" <<'PY'
import sys, shlex
from pathlib import Path
fields = dict(line.split('=', 1) for line in Path(sys.argv[1]).read_text().splitlines() if '=' in line)
assert fields['StartupWMClass'] == 'aseprite-metadata-editor'
assert fields['Icon'].replace('\\\\', '\\') == sys.argv[3]
# Unescape the desktop-entry string first, then parse the quoted Exec argument.
command = fields['Exec'].replace('\\\\', '\\')
# shlex does not unescape dollars/backticks in quotes like desktop Exec does.
command = command.replace('\\$', '$').replace('\\`', '`')
args = shlex.split(command)
assert len(args) == 1, args
assert args[0].replace('%%', '%') == sys.argv[2], args
PY
if command -v desktop-file-validate >/dev/null; then desktop-file-validate "$desktop"; fi
# Reinstall replaces the executable and leaves exactly one dedicated launcher.
printf '#!/bin/sh\nexit 0\n# updated fixture\n' > "$source_app"
bash "$repo/packaging/install-linux.sh" "$source_app" "$repo/assets/icon-256.png"
cmp "$source_app" "$app"
test "$(find "$XDG_DATA_HOME/applications" -type f -name '*.desktop' | wc -l)" -eq 1
! find "$XDG_DATA_HOME" -name '*.tmp.*' | grep -q .
# Missing icon fails before touching any application files.
if bash "$repo/packaging/install-linux.sh" "$source_app" "$HOME/missing.png"; then exit 1; fi
cmp "$source_app" "$app"
# Sibling release icon default and default XDG location work too.
unset XDG_DATA_HOME
cp "$repo/packaging/install-linux.sh" "$HOME/downloads/install-linux.sh"
cp "$repo/assets/icon-256.png" "$HOME/downloads/aseprite-metadata-editor.png"
bash "$HOME/downloads/install-linux.sh" "$source_app"
test -f "$HOME/.local/share/applications/aseprite-metadata-editor.desktop"
printf 'Linux installer: custom paths, escaping, icon bytes, repeat install and missing-icon guard passed.\n'
