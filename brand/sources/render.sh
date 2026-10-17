#!/bin/sh
# Renders each brand page to a PNG in brand/, at twice its size where the
# site shows it on high-density screens. Needs Google Chrome and the network
# (the fonts come from Google Fonts).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
chrome=${CHROME:-"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"}
python3 "$here/build.py"
shot() { # name width height scale
  "$chrome" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor="$4" \
    --window-size="$2,$3" --virtual-time-budget=4000 \
    --screenshot="$here/../$1.png" "file://$here/$1.html" >/dev/null 2>&1
}
shot avatar 500 500 1
shot linkedin-logo 400 400 1
shot social-preview 1280 640 1
shot readme-header 1280 320 2
shot linkedin-cover 1128 191 2
