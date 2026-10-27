#!/usr/bin/env bash
# Whether a change touches anything but documentation, for CI (ci.yml's
# `changes` job). Prints `code=true` or `code=false`, and writes it to
# $GITHUB_OUTPUT when that is set.
#
# Deny by default: a change is documentation only when it changes at least
# one file and every file it changes, by its old name and its new one, is
# on the list in `documentation`. A range that is not two commit ids, or
# that git cannot read, is code.
#
#   scripts/ci-changes.sh BASE HEAD
set -euo pipefail

# The files that hold no code, nothing a build or a test reads, and no
# dependency: the docs site's own lockfile is a dependency (osv-scanner
# reads it in the `deny` job), so it is not here.
documentation() {
  case "$1" in
    website/package.json | website/package-lock.json) return 1 ;;
    docs/* | website/*) return 0 ;;
    README.md | CHANGELOG.md | MAINTAINING.md | SECURITY.md | CLAUDE.md) return 0 ;;
    *) return 1 ;;
  esac
}

decide() {
  local base=$1 head=$2 file files=0
  [[ $base =~ ^[0-9a-f]{40}$ && $head =~ ^[0-9a-f]{40}$ ]] || return 1
  [[ $base =~ ^0+$ ]] && return 1
  local list
  list=$(mktemp)
  # shellcheck disable=SC2064 # the name is fixed now, on purpose
  trap "rm -f '$list'" RETURN
  git diff --name-only --no-renames -z "$base...$head" >"$list" || return 1
  while IFS= read -r -d '' file; do
    files=$((files + 1))
    documentation "$file" || return 1
  done <"$list"
  ((files > 0))
}

if decide "${1:-}" "${2:-}"; then
  code=false
else
  code=true
fi
echo "code=$code"
if [[ -n ${GITHUB_OUTPUT:-} ]]; then
  echo "code=$code" >>"$GITHUB_OUTPUT"
fi
