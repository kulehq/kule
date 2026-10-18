#!/usr/bin/env bash
# Runs check.sh from macOS in a Linux container (podman): Debian with
# WebKitGTK, Xvfb and a copy of the repository (mounted read-only). Builds go to a
# volume (kule-window-target) that later runs reuse. The screenshot lands in
# target/linux-window/. Starts the podman machine if it is stopped, and
# stops it again afterwards (it holds about 9 GB of memory).
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
out="$repo/target/linux-window"
mkdir -p "$out"
# rust:1.92-slim, pinned.
image=docker.io/library/rust@sha256:628183ec0eb88ecc4f1f0c5299ab59565ba6ec732bc5bc52842f2720f32d34c2

started=0
if [ "$(podman machine inspect --format '{{.State}}' 2>/dev/null)" != "running" ]; then
  podman machine start
  started=1
fi
trap '[ "$started" = 1 ] && podman machine stop >/dev/null' EXIT

podman volume create kule-window-target >/dev/null 2>&1 || true
podman run --rm --init --privileged \
  -v "$repo:/src:ro" -v kule-window-target:/target -v "$out:/out" \
  "$image" bash -euo pipefail -c '
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends \
      libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
      xvfb dbus dbus-x11 imagemagick python3 ca-certificates >/dev/null
    useradd -m dev
    chown dev /target /out
    # A writable copy: Tauri'"'"'s build script writes beside its crate.
    mkdir /home/dev/src
    tar -C /src --exclude=./target --exclude=./.git -cf - . | tar -C /home/dev/src -xf -
    chown -R dev /home/dev/src
    # No GPU in the container, and no nested sandbox for WebKit'"'"'s own
    # processes there; neither touches what an extension frame may do.
    su dev -c "
      export PATH=/usr/local/cargo/bin:\$PATH RUSTUP_HOME=/usr/local/rustup
      export CARGO_HOME=/target/cargo-home CARGO_TARGET_DIR=/target/build OUT=/out
      export LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1
      export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
      /home/dev/src/scripts/linux-window/check.sh
    "
  '
echo "screenshot: $out/linux-window.png"
