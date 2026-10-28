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
icon="$icon_dir/io.github.isaiahpettingill.AsepriteMetadataEditor.png"
desktop="$desktop_dir/io.github.isaiahpettingill.AsepriteMetadataEditor.desktop"
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
    'StartupWMClass=io.github.isaiahpettingill.AsepriteMetadataEditor', ''
])
Path(sys.argv[3]).write_text(text, encoding='utf-8')
PY
chmod 644 "$temporary"
mv -f -- "$temporary" "$desktop"
# Only retire a launcher produced by our earlier installer, never Aseprite's
# launcher or a customized command. Keep its exact bytes in a non-launcher backup.
python3 - "$desktop_dir/aseprite-metadata-editor.desktop" "$desktop" <<'PYMIGRATE'
import os, sys, tempfile
from pathlib import Path
legacy, current = map(Path, sys.argv[1:])
def fields(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 65536:
        return {}
    result = {}; active = False
    for line in path.read_text(encoding='utf-8').splitlines():
        if line.startswith('['):
            active = line == '[Desktop Entry]'
        elif active and '=' in line and not line.startswith('#'):
            key, value = line.split('=', 1)
            if key in result:
                return {}
            result[key] = value
    return result
try:
    old, new = fields(legacy), fields(current)
    expected = dict(new)
    expected['StartupWMClass'] = 'aseprite-metadata-editor'
    expected['Icon'] = expected.get('Icon', '').replace(
        'io.github.isaiahpettingill.AsepriteMetadataEditor.png', 'aseprite-metadata-editor.png')
    # Exact known fields protect custom labels, icons, actions and commands too.
    if old == expected and new.get('Exec'):
        fd, backup = tempfile.mkstemp(prefix=legacy.name + '.backup-', dir=legacy.parent)
        os.close(fd)
        try:
            os.replace(legacy, backup)
        except OSError:
            os.unlink(backup)
            raise
        print('Previous launcher preserved as ' + Path(backup).name)
except (OSError, UnicodeError) as error:
    print('Previous launcher left unchanged: ' + str(error), file=sys.stderr)
PYMIGRATE
if command -v update-desktop-database >/dev/null; then update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true; fi
if command -v gtk-update-icon-cache >/dev/null; then gtk-update-icon-cache -f -t "$data_home/icons/hicolor" >/dev/null 2>&1 || true; fi
printf 'Installed Aseprite Metadata Editor.\nLauncher: %s\nRun this script again with a newer AppImage to update.\n' "$desktop"
