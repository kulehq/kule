#!/usr/bin/env bash
# Opens Kule's window on Linux (WebKitGTK) with a probe extension and checks
# what an extension's frame can and cannot do there, as on macOS: its files
# are served and its modules run, and it reaches nothing it should not
# (Tauri, the window, browser storage, the network, other local servers,
# WebRTC). Prints one line per check and exits 1 if any fails.
#
# Needs a display (or Xvfb), dbus-run-session, python3, the WebKitGTK
# development packages, and ImageMagick's `import` for the screenshot.
# run.sh runs it in a container from macOS.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
# Not under /tmp: Kule refuses a folder under development where agents write.
work=${WORK:-$HOME/kule-window-check}
out=${OUT:-$work/out}
rm -rf "$work/home" "$work/probe"
mkdir -p "$work/home" "$out"
export KULE_HOME="$work/home"

cargo build --locked -p kule -p kule-desktop --manifest-path "$repo/Cargo.toml"
bin=${CARGO_TARGET_DIR:-$repo/target}/debug

cp -R "$here/probe" "$work/probe"
"$bin/kule" ext dev "$work/probe" >/dev/null
"$bin/kule" ext approve check.window --yes >/dev/null
# The probe's panel open on the right, for the screenshot.
cat >"$KULE_HOME/layout.json" <<'JSON'
{"version":2,"regions":{
 "left":{"panels":["tasks","files"],"active":"tasks","open":true,"size":272},
 "main":{"panels":["review","timeline"],"active":"review","open":true,"size":0},
 "right":{"panels":["ext.check.window.probe"],"active":"ext.check.window.probe","open":true,"size":420},
 "bottom":{"panels":["live","terminal"],"active":"live","open":false,"size":300}}}
JSON

# Anything that reaches port 8766 is logged here.
python3 -m http.server 8766 --bind 127.0.0.1 >"$work/listener.log" 2>&1 &
listener=$!
xvfb=""
if [ -z "${DISPLAY:-}" ]; then
  Xvfb :99 -screen 0 1400x900x24 >/dev/null 2>&1 &
  xvfb=$!
  export DISPLAY=:99
  sleep 1
fi
dbus-run-session -- "$bin/kule-desktop" >"$work/app.log" 2>&1 &
app=$!
sleep "${WAIT:-30}"
import -window root "$out/linux-window.png" 2>/dev/null || echo "(no screenshot)"
kill "$app" "$listener" $xvfb 2>/dev/null || true
pkill -f "kule-desktop daemon" 2>/dev/null || true

python3 - "$KULE_HOME/extensions/data/check.window/storage.json" "$work/listener.log" <<'PY'
import json, sys
storage, listener = sys.argv[1], sys.argv[2]
try:
    seen = json.load(open(storage))
except Exception as e:
    seen = {}
    print(f"FAIL the probe kept nothing ({e}): its frame never ran or never reached Kule")
heard = open(listener).read()
checks = [
    ("its files are served and its modules run, with an import", seen.get("module") == "imported"),
    ("WebRTC is off", seen.get("rtc") == "undefined"),
    ("Tauri's internals are not in the frame", seen.get("tauri") == "undefined"),
    ("the window is out of reach", seen.get("parent") == "blocked"),
    ("browser storage is refused", seen.get("localStorage") == "blocked"),
    ("fetch is refused", seen.get("fetch") == "blocked" and "/fetch" not in heard),
    ("navigating to another local server is refused", "/nav" not in heard),
]
failed = 0
for what, ok in checks:
    print(("PASS " if ok else "FAIL ") + what)
    failed += not ok
print(f"seen: {json.dumps(seen)}")
sys.exit(1 if failed else 0)
PY
