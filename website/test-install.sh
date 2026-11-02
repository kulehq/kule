#!/bin/sh
# Tests website/static/install.sh, the installer the site serves, on the real
# file system: a release of a stand-in `kule` in a temporary folder, read
# through KULE_DOWNLOAD_URL (file://), installed into a temporary home.
# `uname`, `sw_vers` and `gh` are stand-ins on PATH, so every platform's
# path and every check is tried on any machine.
#
#   sh website/test-install.sh
#
# With KULE_TEST_ONLINE=1 (CI), it also installs the newest real release
# into a temporary home, as a person would; where the GitHub CLI is signed
# in there (CI: GH_TOKEN), its build provenance must be checked.

set -eu

here=$(cd "$(dirname "$0")" && pwd)
script="$here/static/install.sh"
work=$(mktemp -d 2>/dev/null || mktemp -d -t kule-install)
trap 'rm -rf "$work"' EXIT
sys=/usr/bin:/bin:/usr/sbin:/sbin
fails=0

ok() { printf 'ok    %s\n' "$1"; }
bad() {
    printf 'FAIL  %s\n' "$1"
    sed 's/^/      | /' "$work/out" 2>/dev/null || true
    fails=$((fails + 1))
}
sum() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

# Stand-ins: the platform and the GitHub CLI, as each test sets them.
shims="$work/shims"
mkdir -p "$shims"
cat >"$shims/uname" <<'EOF'
#!/bin/sh
case "$1" in
    -s) echo "${FAKE_OS:-Linux}" ;;
    -m) echo "${FAKE_ARCH:-x86_64}" ;;
    *) echo "${FAKE_OS:-Linux}" ;;
esac
EOF
cat >"$shims/sw_vers" <<'EOF'
#!/bin/sh
echo "${FAKE_MACOS:-14.5}"
EOF
# FAKE_GH: "out" (not signed in), "ok" (provenance checks), "bad" (it
# does not). Every call is noted in $GH_LOG.
cat >"$shims/gh" <<'EOF'
#!/bin/sh
echo "$*" >>"$GH_LOG"
case "${FAKE_GH:-out}:$1" in
    out:*) exit 1 ;;
    ok:*) exit 0 ;;
    bad:auth) exit 0 ;;
    *) exit 1 ;;
esac
EOF
chmod 755 "$shims"/*

# A mirror laid out as GitHub serves the releases: a feed naming v9.9.10
# (a tag whose files are not up yet) and v9.9.9, and v9.9.9's files, both
# archives and SHA256SUMS. $1 is what the stand-in `kule --version` does:
# "runs" or "breaks".
feed() {
    printf '<?xml version="1.0" encoding="UTF-8"?>\n<feed xmlns="http://www.w3.org/2005/Atom">\n'
    printf '  <link type="text/html" rel="alternate" href="https://github.com/kulehq/kule/releases"/>\n'
    for v in "$@"; do
        printf '  <entry>\n    <id>tag:github.com,2008:Repository/1/v%s</id>\n' "$v"
        printf '    <link rel="alternate" type="text/html" href="https://github.com/kulehq/kule/releases/tag/v%s"/>\n  </entry>\n' "$v"
    done
    printf '</feed>\n'
}
release() {
    rel="$work/rel-$1"
    [ -d "$rel" ] && return 0
    mkdir -p "$rel/pack" "$rel/download/v9.9.9"
    feed 9.9.10 9.9.9 >"$rel/releases.atom"
    if [ "$1" = runs ]; then
        printf '#!/bin/sh\necho "kule 9.9.9"\n' >"$rel/pack/kule"
    else
        printf '#!/bin/sh\nexit 1\n' >"$rel/pack/kule"
    fi
    chmod 755 "$rel/pack/kule"
    for platform in linux-x86_64 macos-universal; do
        a="kule-9.9.9-$platform.tar.gz"
        tar -czf "$rel/download/v9.9.9/$a" -C "$rel/pack" kule
        printf '%s  %s\n' "$(sum "$rel/download/v9.9.9/$a")" "$a" >>"$rel/download/v9.9.9/SHA256SUMS"
    done
}
release runs
release breaks

# Runs the installer in a fresh home (or $HOME_DIR), with only the given
# settings: `install VAR=value ...`. Its output is in $work/out. HOME_DIR
# and TEST_SHELL hold for one call: an assignment before a function call
# outlives it in some shells.
install() {
    home=${HOME_DIR:-$(mktemp -d "$work/home.XXXXXX")}
    GH_LOG="$work/gh.log"
    : >"$GH_LOG"
    status=0
    env -i HOME="$home" PATH="$shims:$sys" SHELL="${TEST_SHELL:-/bin/bash}" \
        GH_LOG="$GH_LOG" KULE_VERSION=9.9.9 \
        KULE_DOWNLOAD_URL="file://$work/rel-runs" "$@" \
        sh "$script" >"$work/out" 2>&1 || status=$?
    unset HOME_DIR TEST_SHELL
}
installed() { [ -x "$home/.local/bin/kule" ]; }
said() { grep -qF "$1" "$work/out"; }

# --- Installs, and puts the folder on PATH once ---------------------------
install
if [ $status = 0 ] && installed && [ "$("$home/.local/bin/kule" --version)" = "kule 9.9.9" ] &&
    grep -qxF "export PATH=\"$home/.local/bin:\$PATH\"" "$home/.bashrc" &&
    said "build provenance NOT checked"; then
    ok "installs kule in ~/.local/bin, adds it to PATH in .bashrc, says provenance was not checked"
else
    bad "installs kule in ~/.local/bin, adds it to PATH in .bashrc, says provenance was not checked"
fi
HOME_DIR=$home install
if [ $status = 0 ] && [ "$(grep -c '^# kule$' "$home/.bashrc")" = 1 ]; then
    ok "a second run updates without adding PATH again"
else
    bad "a second run updates without adding PATH again"
fi

TEST_SHELL=/bin/zsh install
if [ $status = 0 ] && grep -qF "$home/.local/bin" "$home/.zshrc"; then
    ok "zsh: PATH in .zshrc"
else
    bad "zsh: PATH in .zshrc"
fi
TEST_SHELL=/usr/bin/fish install
if [ $status = 0 ] && grep -qxF "fish_add_path \"$home/.local/bin\"" "$home/.config/fish/conf.d/kule.fish"; then
    ok "fish: PATH in conf.d/kule.fish"
else
    bad "fish: PATH in conf.d/kule.fish"
fi
install FAKE_OS=Darwin FAKE_ARCH=arm64
if [ $status = 0 ] && installed && grep -qF "$home/.local/bin" "$home/.bash_profile" &&
    said "kule-9.9.9-macos-universal.tar.gz"; then
    ok "macOS on Apple silicon: the universal archive, PATH in .bash_profile"
else
    bad "macOS on Apple silicon: the universal archive, PATH in .bash_profile"
fi
install FAKE_OS=Darwin FAKE_ARCH=x86_64
if [ $status = 0 ] && installed && said "kule-9.9.9-macos-universal.tar.gz"; then
    ok "macOS on Intel: the universal archive"
else
    bad "macOS on Intel: the universal archive"
fi
install KULE_NO_MODIFY_PATH=1
if [ $status = 0 ] && installed && [ ! -e "$home/.bashrc" ] && said "is not on PATH"; then
    ok "KULE_NO_MODIFY_PATH=1 touches no startup file"
else
    bad "KULE_NO_MODIFY_PATH=1 touches no startup file"
fi
install KULE_INSTALL_DIR="$work/elsewhere"
if [ $status = 0 ] && [ -x "$work/elsewhere/kule" ] && ! installed; then
    ok "KULE_INSTALL_DIR puts it there"
else
    bad "KULE_INSTALL_DIR puts it there"
fi

# --- Nothing is installed when a check fails ------------------------------
tampered="$work/rel-tampered"
cp -R "$work/rel-runs" "$tampered"
printf 'more\n' >>"$tampered/download/v9.9.9/kule-9.9.9-linux-x86_64.tar.gz"
install KULE_DOWNLOAD_URL="file://$tampered"
if [ $status != 0 ] && ! installed && said "does not match its SHA256SUMS"; then
    ok "an archive that does not match SHA256SUMS installs nothing"
else
    bad "an archive that does not match SHA256SUMS installs nothing"
fi
unlisted="$work/rel-unlisted"
cp -R "$work/rel-runs" "$unlisted"
grep -v linux-x86_64 "$work/rel-runs/download/v9.9.9/SHA256SUMS" >"$unlisted/download/v9.9.9/SHA256SUMS"
install KULE_DOWNLOAD_URL="file://$unlisted"
if [ $status != 0 ] && ! installed && said "SHA256SUMS lists no"; then
    ok "an archive SHA256SUMS does not list installs nothing"
else
    bad "an archive SHA256SUMS does not list installs nothing"
fi

old=$(mktemp -d "$work/home.XXXXXX")
mkdir -p "$old/.local/bin"
printf '#!/bin/sh\necho "kule old"\n' >"$old/.local/bin/kule"
chmod 755 "$old/.local/bin/kule"
HOME_DIR=$old install KULE_DOWNLOAD_URL="file://$work/rel-breaks"
if [ $status != 0 ] && [ "$("$old/.local/bin/kule" --version)" = "kule old" ] &&
    said "does not run on this system"; then
    ok "a kule that does not run here replaces nothing"
else
    bad "a kule that does not run here replaces nothing"
fi

install FAKE_GH=ok
if [ $status = 0 ] && installed && said "build provenance checked" &&
    grep -q "^attestation verify .*kule-9.9.9-linux-x86_64.tar.gz --repo kulehq/kule --signer-workflow kulehq/kule/.github/workflows/release.yml --source-ref refs/tags/v9.9.9\$" "$work/gh.log"; then
    ok "with gh signed in, the provenance is checked: this repository, its release workflow, the version's tag"
else
    bad "with gh signed in, the provenance is checked: this repository, its release workflow, the version's tag"
fi
install FAKE_GH=bad
if [ $status != 0 ] && ! installed && said "has no build provenance"; then
    ok "provenance that does not check out installs nothing"
else
    bad "provenance that does not check out installs nothing"
fi
install KULE_REQUIRE_ATTESTATION=1
if [ $status != 0 ] && ! installed && said "KULE_REQUIRE_ATTESTATION=1"; then
    ok "KULE_REQUIRE_ATTESTATION=1 without gh signed in installs nothing"
else
    bad "KULE_REQUIRE_ATTESTATION=1 without gh signed in installs nothing"
fi

# --- Platforms without a release are told so ------------------------------
for case in "Linux aarch64 x no Kule release for Linux on aarch64" \
    "Darwin arm64 12.7 Kule needs macOS 13" \
    "MINGW64_NT-10.0 x86_64 x setup program" \
    "FreeBSD amd64 x this is FreeBSD"; do
    # shellcheck disable=SC2086 # the words of the case
    set -- $case
    os=$1 arch=$2 macos=$3
    shift 3
    install FAKE_OS="$os" FAKE_ARCH="$arch" FAKE_MACOS="$macos"
    if [ $status != 0 ] && ! installed && said "$*"; then
        ok "$os $arch: refused, \"$*\""
    else
        bad "$os $arch: refused, \"$*\""
    fi
done

# --- The newest release, from the feed -------------------------------------
install KULE_VERSION=
if [ $status = 0 ] && installed && said "v9.9.10 has no release files yet" &&
    said "kule-9.9.9-linux-x86_64.tar.gz"; then
    ok "without KULE_VERSION: the newest release in the feed with its files up, the one skipped said"
else
    bad "without KULE_VERSION: the newest release in the feed with its files up, the one skipped said"
fi
empty="$work/rel-empty"
mkdir -p "$empty"
feed 9.9.12 9.9.11 >"$empty/releases.atom"
install KULE_VERSION= KULE_DOWNLOAD_URL="file://$empty"
if [ $status != 0 ] && ! installed && said "found no release with its files"; then
    ok "a feed with no release's files up installs nothing"
else
    bad "a feed with no release's files up installs nothing"
fi
install KULE_VERSION=9.9.8
if [ $status != 0 ] && ! installed && said "could not download"; then
    ok "a KULE_VERSION with no release installs nothing"
else
    bad "a KULE_VERSION with no release installs nothing"
fi

# --- The releases' versions, from GitHub's feed --------------------------
eval "$(sed -n '/^tags_of() {$/,/^}$/p' "$script")"
feed 0.2.0-preview.21 0.2.0-preview.20 >"$work/many.atom"
tr -d '\n' <"$work/many.atom" >"$work/one.atom"
# shellcheck disable=SC2016 # kept as text, never run
printf '<link href="https://github.com/kulehq/kule/releases/tag/v1.0$(touch pwned)"/><link href="https://github.com/other/kule/releases/tag/v5.0"/><link href="https://github.com/kulehq/kule/releases/tag/v0.9"/>' >"$work/odd.atom"
if [ "$(tags_of "$work/many.atom" | tr '\n' ' ')" = "0.2.0-preview.21 0.2.0-preview.20 " ] &&
    [ "$(tags_of "$work/one.atom" | tr '\n' ' ')" = "0.2.0-preview.21 0.2.0-preview.20 " ] &&
    [ "$(tags_of "$work/odd.atom")" = 0.9 ]; then
    ok "the releases are read from the feed newest first, on many lines or one: this repository's, a version's characters only"
else
    : >"$work/out"
    bad "the releases are read from the feed newest first, on many lines or one: this repository's, a version's characters only"
fi

# --- The real thing, as a person runs it (CI) ------------------------------
if [ "${KULE_TEST_ONLINE:-}" = 1 ]; then
    home=$(mktemp -d "$work/home.XXXXXX")
    status=0
    env HOME="$home" SHELL=/bin/bash sh "$script" >"$work/out" 2>&1 || status=$?
    if [ $status = 0 ] && "$home/.local/bin/kule" --version | grep -q '^kule '; then
        ok "the newest release installs and runs: $("$home/.local/bin/kule" --version)"
    else
        bad "the newest release installs and runs"
    fi
    # Signed in as the installer saw it, in the same home: on CI through
    # GH_TOKEN, whatever the home.
    if command -v gh >/dev/null 2>&1 && HOME="$home" gh auth status >/dev/null 2>&1; then
        if said "build provenance checked"; then
            ok "its build provenance was checked"
        else
            bad "its build provenance was checked"
        fi
    fi
fi

if [ $fails != 0 ]; then
    printf '%s failed\n' "$fails"
    exit 1
fi
printf 'all passed\n'
