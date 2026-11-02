#!/bin/sh
# Kule's installer: puts the prebuilt `kule` command for this machine on PATH.
#
#   curl -fsSL https://kulehq.github.io/kule/install.sh | sh
#
# It finds the newest release (previews included) in the feed of
# https://github.com/kulehq/kule/releases, skipping one whose files are not
# up yet, downloads the `kule` archive for
# this system (macOS 13 or later, Apple silicon and Intel; Linux x86_64),
# and checks it against the release's SHA256SUMS. With the GitHub CLI
# signed in, it also checks the archive's build provenance (that the
# release workflow built it from the release's tag). Nothing is installed
# when a check fails, or when the program does not run here. It puts
# `kule` in ~/.local/bin and, when that is not on PATH yet, adds it in your
# shell's startup file once. Running it again updates.
#
# The desktop app is not installed: it comes as a .dmg, .deb or .rpm from
# the releases page.
#
# Settings, from the environment:
#   KULE_VERSION=0.2.0-preview.21  a version instead of the newest
#   KULE_INSTALL_DIR=DIR           where kule goes (default: ~/.local/bin)
#   KULE_NO_MODIFY_PATH=1          never touch a startup file
#   KULE_REQUIRE_ATTESTATION=1     install nothing unless the build
#                                  provenance was checked too
#   KULE_DOWNLOAD_URL=URL          a mirror laid out as GitHub serves the
#                                  releases: releases.atom and
#                                  download/v<version>/ (the tests give a
#                                  file:// directory)
#
# Uninstall: rm ~/.local/bin/kule (and the "# kule" lines it added).
#
# Everything runs from `main` at the very end, so a download cut short
# runs nothing.

set -eu

REPO="kulehq/kule"
RELEASES="https://github.com/$REPO/releases"
DOCS="https://kulehq.github.io/kule/docs/getting-started/install"

say() {
    printf 'kule: %s\n' "$*"
}

die() {
    printf 'kule: %s\n' "$*" >&2
    exit 1
}

has() {
    command -v "$1" >/dev/null 2>&1
}

# The release archive this machine takes, by the name the release gives it.
platform() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Darwin)
            major=$(sw_vers -productVersion 2>/dev/null | cut -d . -f 1)
            case "$major" in
                '' | *[!0-9]*) die "could not tell which macOS this is" ;;
            esac
            [ "$major" -ge 13 ] ||
                die "Kule needs macOS 13 (Ventura) or later; this is macOS $major"
            # One build for Apple silicon and Intel.
            printf 'macos-universal\n'
            ;;
        Linux)
            case "$arch" in
                x86_64 | amd64) printf 'linux-x86_64\n' ;;
                *) die "no Kule release for Linux on $arch; build it from source: $DOCS#build-from-source" ;;
            esac
            ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            die "on Windows, install Kule with its setup program from $RELEASES"
            ;;
        *)
            die "Kule runs on macOS, Linux and Windows; this is $os"
            ;;
    esac
}

# Fetches $1 to the file $2, over HTTPS only (a mirror given in
# KULE_DOWNLOAD_URL may also be a local directory); fails quietly.
fetch() {
    if has curl; then
        proto='=https'
        if [ -n "${KULE_DOWNLOAD_URL:-}" ]; then
            proto='=https,file'
        fi
        curl --proto "$proto" --proto-redir '=https' --tlsv1.2 \
            --fail --silent --location --retry 3 \
            --output "$2" "$1"
    elif has wget; then
        wget --https-only --quiet --output-document="$2" "$1"
    else
        die "curl or wget is needed to download Kule"
    fi
}

download() {
    fetch "$1" "$2" || die "could not download $1"
}

# The versions of the releases in GitHub's feed $1 (releases.atom), newest
# first: each release's tag without the "v", from its link, whether the
# feed comes on one line or many. Only what a version holds is taken.
tags_of() {
    tr '"' '\n' <"$1" | sed -n 's#^https://github\.com/kulehq/kule/releases/tag/v\([0-9A-Za-z.+-]*\)$#\1#p'
}

sha256() {
    if has sha256sum; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif has shasum; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        die "sha256sum or shasum is needed to check the download"
    fi
}

# The line that puts $1 on PATH in the startup file of the person's shell,
# and that file, as "FILE|LINE". `$PATH` is written as it is, for the shell
# to expand when it starts.
# shellcheck disable=SC2016
path_line() {
    case "$(basename "${SHELL:-sh}")" in
        zsh)
            printf '%s|export PATH="%s:$PATH"\n' "${ZDOTDIR:-$HOME}/.zshrc" "$1"
            ;;
        bash)
            # macOS terminals open login shells, which read .bash_profile.
            if [ "$(uname -s)" = Darwin ]; then
                rc="$HOME/.bash_profile"
            else
                rc="$HOME/.bashrc"
            fi
            printf '%s|export PATH="%s:$PATH"\n' "$rc" "$1"
            ;;
        fish)
            printf '%s|fish_add_path "%s"\n' \
                "${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/kule.fish" "$1"
            ;;
        *)
            printf '%s|export PATH="%s:$PATH"\n' "$HOME/.profile" "$1"
            ;;
    esac
}

main() {
    for tool in uname tar mkdir chmod cp mv rm grep cut sed head tr basename mktemp; do
        has "$tool" || die "$tool is needed"
    done
    platform=$(platform)
    dir="${KULE_INSTALL_DIR:-$HOME/.local/bin}"

    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t kule)
    trap 'rm -rf "$tmp"' EXIT
    trap 'exit 1' INT TERM HUP

    if [ -n "${KULE_DOWNLOAD_URL:-}" ]; then
        mirror="${KULE_DOWNLOAD_URL%/}"
        feed="$mirror/releases.atom"
        files="$mirror/download"
    else
        feed="$RELEASES.atom"
        files="$RELEASES/download"
    fi
    if [ -n "${KULE_VERSION:-}" ]; then
        version=${KULE_VERSION#v}
        download "$files/v$version/SHA256SUMS" "$tmp/SHA256SUMS"
    else
        # GitHub's "latest" leaves previews out; its feed has every
        # release. A tag pushed before its release's files are up is
        # skipped, and said so.
        download "$feed" "$tmp/releases.atom"
        version=""
        for v in $(tags_of "$tmp/releases.atom" | head -n 3); do
            if fetch "$files/v$v/SHA256SUMS" "$tmp/SHA256SUMS"; then
                version=$v
                break
            fi
            say "v$v has no release files yet; trying the one before it"
        done
        [ -n "$version" ] ||
            die "found no release with its files in $feed; set KULE_VERSION"
    fi
    base="$files/v$version"
    archive="kule-$version-$platform.tar.gz"

    say "downloading $archive"
    download "$base/$archive" "$tmp/$archive"
    want=$(grep "  $archive\$" "$tmp/SHA256SUMS" | cut -d ' ' -f 1 || true)
    [ -n "$want" ] || die "SHA256SUMS lists no $archive; nothing installed"
    got=$(sha256 "$tmp/$archive")
    [ "$got" = "$want" ] ||
        die "$archive does not match its SHA256SUMS (got $got, want $want); nothing installed"
    say "checksum matches SHA256SUMS"

    # The checksum comes from the same release as the archive; the build
    # provenance says the release workflow made it, from the release's tag.
    verify="gh attestation verify $archive --repo $REPO --signer-workflow $REPO/.github/workflows/release.yml --source-ref refs/tags/v$version"
    if has gh && gh auth status >/dev/null 2>&1; then
        gh attestation verify "$tmp/$archive" --repo "$REPO" \
            --signer-workflow "$REPO/.github/workflows/release.yml" \
            --source-ref "refs/tags/v$version" >/dev/null 2>&1 ||
            die "$archive has no build provenance from $REPO's release workflow at v$version; nothing installed"
        say "build provenance checked: built by $REPO's release workflow from v$version"
    elif [ "${KULE_REQUIRE_ATTESTATION:-}" = 1 ]; then
        die "KULE_REQUIRE_ATTESTATION=1, and the build provenance cannot be checked without the GitHub CLI signed in (gh auth login); nothing installed"
    else
        say "build provenance NOT checked (it takes the GitHub CLI, signed in); to check it yourself, download the archive and run:"
        say "  $verify"
    fi

    tar -xzf "$tmp/$archive" -C "$tmp" kule || die "$archive holds no kule"
    chmod 755 "$tmp/kule"
    # Run where it was unpacked first: one that cannot run here (an older
    # system library) is never put on PATH.
    new=$("$tmp/kule" --version 2>/dev/null) ||
        die "the downloaded kule does not run on this system; nothing installed. Build it from source: $DOCS#build-from-source"

    mkdir -p "$dir"
    # Next to the old one, then moved over it: a running kule keeps its
    # file, and no half-written one is ever on PATH.
    cp "$tmp/kule" "$dir/.kule.new"
    chmod 755 "$dir/.kule.new"
    mv -f "$dir/.kule.new" "$dir/kule"
    say "installed $new at $dir/kule"

    case ":$PATH:" in
        *":$dir:"*)
            first=$(command -v kule || true)
            if [ -n "$first" ] && [ "$first" != "$dir/kule" ]; then
                say "note: $first comes first on PATH; remove it or put $dir before it"
            fi
            ;;
        *)
            if [ "${KULE_NO_MODIFY_PATH:-}" = 1 ]; then
                say "$dir is not on PATH; add it to use kule by name"
            else
                spec=$(path_line "$dir")
                rc=${spec%%|*}
                line=${spec#*|}
                if ! grep -qsF "$line" "$rc"; then
                    mkdir -p "${rc%/*}"
                    printf '\n# kule\n%s\n' "$line" >>"$rc"
                    say "added $dir to PATH in $rc"
                fi
                say "open a new terminal, or run: $line"
            fi
            ;;
    esac

    if [ "$platform" = linux-x86_64 ]; then
        say "Kule's Linux sandbox needs unprivileged user namespaces: run 'kule isolation' to see what this computer gives"
    fi
    say "the desktop app (.dmg, .deb, .rpm) and the next steps: $DOCS"
}

main "$@"
