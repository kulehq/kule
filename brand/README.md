# Kule's mark

The tower as three shapes: the beacon, the cab, the stem. White on coral
(`#ff5a3c`).

| File | What it is |
| --- | --- |
| `kule-icon.svg` | The macOS app icon, on Apple's grid: an 824 × 824 rounded square inside a 1024 × 1024 canvas, with its shadow. |
| `kule-icon-square.svg` | The app icon for Windows and Linux: a plate that fills the canvas, no shadow. |
| `kule-mark.svg` | The glyph alone, in `currentColor`, for anywhere the plate is not wanted. |

The app's icons are made from these with the Tauri CLI:

```sh
rsvg-convert -w 1024 -h 1024 brand/kule-icon.svg -o /tmp/kule-mac.png
rsvg-convert -w 1024 -h 1024 brand/kule-icon-square.svg -o /tmp/kule-square.png
cargo tauri icon /tmp/kule-mac.png -o /tmp/mac       # icons/*.png, icons/icon.icns
cargo tauri icon /tmp/kule-square.png -o /tmp/square # icons/linux/*.png, icons/windows/icon.ico
```

`tauri.conf.json` lists the macOS set; `tauri.linux.conf.json` and
`tauri.windows.conf.json` replace it on those systems.
