#!/bin/sh
# check-site.sh, seen failing: a build whose installer differs from the
# reviewed one, is missing, or is a link is refused; the same build with
# the reviewed file passes.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

build() {
  rm -rf "$work/build"
  mkdir -p "$work/build/tr"
  cp "$here/static/install.sh" "$work/build/install.sh"
  cp "$here/static/install.sh" "$work/build/tr/install.sh"
}
refused() {
  if sh "$here/check-site.sh" "$work/build" >/dev/null 2>&1; then
    echo "test-check-site: accepted $1" >&2
    exit 1
  fi
}

build
sh "$here/check-site.sh" "$work/build" >/dev/null

build
printf 'echo changed\n' >>"$work/build/tr/install.sh"
refused "a changed installer in a language's folder"

build
printf 'x' | dd of="$work/build/install.sh" bs=1 seek=40 conv=notrunc 2>/dev/null
refused "one byte changed at the root"

build
rm "$work/build/install.sh"
refused "no installer at the root"

build
rm "$work/build/tr/install.sh"
ln -s /etc/hosts "$work/build/tr/install.sh"
refused "a link named install.sh"

echo "test-check-site: ok"
