#!/usr/bin/env bash
# CI-only smoke: run under xvfb-run, never against a user's desktop session.
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
[[ ${ASEPRITE_TEST_XVFB:-} == 1 ]] || { echo 'Run only in the isolated CI Xvfb session.' >&2; exit 2; }
work=$(mktemp -d)
pid=''
trap '[[ -z "$pid" ]] || kill "$pid" 2>/dev/null || true; rm -rf -- "$work"' EXIT
export HOME="$work/home" XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME"
WINIT_UNIX_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1 "$repo/target/release/aseprite-metadata-editor" > "$work/app.log" 2>&1 &
pid=$!
window=''
for attempt in $(seq 1 100); do
  if ! kill -0 "$pid" 2>/dev/null; then cat "$work/app.log"; echo 'Application exited before creating its window.' >&2; exit 1; fi
  window=$(xdotool search --onlyvisible --class '^io\.github\.isaiahpettingill\.AsepriteMetadataEditor$' 2>/dev/null | head -1 || true)
  [[ -z "$window" ]] || break
  sleep 0.1
done
[[ -n "$window" ]] || { cat "$work/app.log"; echo 'Expected application WM_CLASS not found.' >&2; exit 1; }
class=$(xprop -id "$window" WM_CLASS)
# egui supplies app_id as the general class with an empty instance name.
# StartupWMClass matches the general class; an empty instance is valid ICCCM.
[[ "$class" == 'WM_CLASS(STRING) = "", "io.github.isaiahpettingill.AsepriteMetadataEditor"' ]] || { printf '%s\n' "$class" >&2; exit 1; }
# Override xprop's special icon ASCII-art display as well as its word format.
xprop -id "$window" -len 2097152 -notype -f _NET_WM_ICON 32c ' = $0+\n' _NET_WM_ICON > "$work/icon-property"
python3 - "$work/icon-property" <<'PY'
import re, sys
value = open(sys.argv[1], encoding='utf-8').read().split('=', 1)[1]
pixels = [int(n) for n in re.findall(r'\d+', value)]
assert pixels[:2] == [256, 256], f'Unexpected window icon dimensions: {pixels[:2]}'
assert len(pixels) >= 2 + 256 * 256, f'Incomplete window icon pixels: {len(pixels)} words; prefix={value[:100]!r}'
assert len(set(pixels[2:])) > 2, 'Window icon is blank or a monochrome placeholder'
PY
printf 'X11 WM_CLASS and 256x256 window icon verified.\n'
