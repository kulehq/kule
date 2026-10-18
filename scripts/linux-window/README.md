# Kule's window on Linux

`check.sh` opens Kule's desktop app on Linux (WebKitGTK) with a probe
extension and checks, in the real window, what an extension's frame can and
cannot do. These are the same things checked by hand on macOS:

- its files are served, and its ES modules run with their imports;
- WebRTC is off;
- Tauri's internals are not in the frame;
- the window is out of reach;
- browser storage is refused;
- `fetch` is refused;
- the frame cannot navigate to another server on the computer.

The probe keeps what it saw with `kule.storage`. A local listener on port
8766 shows whether anything got out. The script prints one line per check
and exits 1 if any fails.

From macOS, in a container:

```sh
scripts/linux-window/run.sh      # screenshot: target/linux-window/linux-window.png
```

On a Linux machine, run `scripts/linux-window/check.sh` directly. It needs
the WebKitGTK development packages, `dbus-run-session`, `python3` and
ImageMagick, and uses Xvfb when there is no display.
