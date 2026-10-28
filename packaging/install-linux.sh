#!/usr/bin/env bash
# Install an already-downloaded AppImage and the release's supplied PNG. No network or sudo.
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 ]]; then
  printf 'Usage: bash %s /path/to/Aseprite-Metadata-Editor-linux-x86_64.AppImage [icon.png]\n' "$0" >&2
  exit 2
fi
command -v python3 >/dev/null || { echo 'Python 3 is required for safe desktop-entry generation.' >&2; exit 1; }
source_app=$1
source_icon=${2:-"$(cd -- "$(dirname -- "$0")" && pwd)/aseprite-metadata-editor.png"}
[[ -f "$source_app" && -r "$source_app" ]] || { echo 'AppImage not found or unreadable.' >&2; exit 1; }
[[ -f "$source_icon" && -r "$source_icon" ]] || { echo 'Icon not found. Download aseprite-metadata-editor.png beside this script, or pass its path as the second argument.' >&2; exit 1; }
: "${HOME:?HOME must be set}"
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
[[ "$data_home" == /* ]] || { echo 'XDG_DATA_HOME must be absolute.' >&2; exit 1; }
# Validate inputs before making any installation changes. Never execute the input AppImage.
python3 - "$source_icon" "$data_home" <<'PY'
import sys
from pathlib import Path
if Path(sys.argv[1]).read_bytes()[:8] != b'\x89PNG\r\n\x1a\n':
    raise SystemExit('The supplied icon is not a PNG.')
if any(ord(c) < 32 or ord(c) == 127 for c in sys.argv[2]):
    raise SystemExit('Installation paths must not contain control characters.')
PY
app_dir="$data_home/aseprite-metadata-editor"
icon_dir="$data_home/icons/hicolor/256x256/apps"
desktop_dir="$data_home/applications"
mkdir -p -- "$app_dir" "$icon_dir" "$desktop_dir"
app="$app_dir/Aseprite-Metadata-Editor.AppImage"
icon="$icon_dir/aseprite-metadata-editor.png"
desktop="$desktop_dir/aseprite-metadata-editor.desktop"
temps=()
trap 'for file in "${temps[@]}"; do rm -f -- "$file"; done' EXIT
copy_atomic() {
  local src=$1 dst=$2 mode=$3 temporary
  temporary=$(mktemp "${dst}.tmp.XXXXXX")
  temps+=("$temporary")
  cp -- "$src" "$temporary"
  chmod "$mode" "$temporary"
  mv -f -- "$temporary" "$dst"
}
copy_atomic "$source_app" "$app" 755
copy_atomic "$source_icon" "$icon" 644
temporary=$(mktemp "${desktop}.tmp.XXXXXX")
temps+=("$temporary")
python3 - "$app" "$icon" "$temporary" <<'PY'
import sys
from pathlib import Path
# Exec quoting has two escaping layers: desktop string escapes and Exec arguments.
def desktop_escape(value):
    return value.replace('\\', '\\\\')
def exec_argument(value):
    value = value.replace('%', '%%')
    value = ''.join('\\' + c if c in '\\"`$' else c for c in value)
    return '"' + desktop_escape(value) + '"'
text = '\n'.join([
    '[Desktop Entry]', 'Version=1.0', 'Type=Application',
    'Name=Aseprite Metadata Editor', 'Comment=Edit sprite metadata in bulk',
    'Exec=' + exec_argument(sys.argv[1]), 'Icon=' + desktop_escape(sys.argv[2]),
    'Terminal=false', 'Categories=Graphics;2DGraphics;', 'StartupNotify=true',
    'StartupWMClass=aseprite-metadata-editor', ''
])
Path(sys.argv[3]).write_text(text, encoding='utf-8')
PY
chmod 644 "$temporary"
mv -f -- "$temporary" "$desktop"
if command -v update-desktop-database >/dev/null; then update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true; fi
if command -v gtk-update-icon-cache >/dev/null; then gtk-update-icon-cache -f -t "$data_home/icons/hicolor" >/dev/null 2>&1 || true; fi
printf 'Installed Aseprite Metadata Editor.\nLauncher: %s\nRun this script again with a newer AppImage to update.\n' "$desktop"
