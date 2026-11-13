#!/bin/sh
# The installer the built site serves is the reviewed and tested one, byte
# for byte: every install.sh in the build output (each language's folder)
# is website/static/install.sh as checked out, and there is one at the
# site's root. Runs no npm code; the docs workflow puts the Pages artifact
# together only after it.
#
#   sh website/check-site.sh <build-dir>
set -eu

fail() {
  echo "check-site: $*" >&2
  exit 1
}

[ $# -eq 1 ] || fail "usage: check-site.sh <build-dir>"
build=$1
reviewed=$(dirname "$0")/static/install.sh
[ -f "$reviewed" ] || fail "no $reviewed"
[ -d "$build" ] || fail "no build output at $build"
[ -f "$build/install.sh" ] && [ ! -L "$build/install.sh" ] || fail "no install.sh at the site's root"

# Anything named install.sh that is not a plain file is not the reviewed one.
odd=$(find "$build" -name install.sh ! -type f)
[ -z "$odd" ] || fail "an install.sh that is not a plain file: $odd"

count=0
list=$(mktemp)
trap 'rm -f "$list"' EXIT
find "$build" -name install.sh -type f >"$list"
while IFS= read -r file; do
  cmp -s "$reviewed" "$file" || fail "$file is not website/static/install.sh"
  count=$((count + 1))
done <"$list"
echo "check-site: $count install.sh, each the reviewed one"
