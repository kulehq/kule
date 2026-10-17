# xterm.js, vendored

The terminal panel draws with xterm.js. The window's content security
policy loads scripts only from the app itself, so the files are kept here,
unchanged, from the npm packages:

| File | Package |
| --- | --- |
| `xterm.js`, `xterm.css` | `@xterm/xterm` 6.0.0 |
| `addon-fit.js` | `@xterm/addon-fit` 0.11.0 |
| `addon-search.js` | `@xterm/addon-search` 0.16.0 |
| `addon-web-links.js` | `@xterm/addon-web-links` 0.12.0 |

All are MIT; `LICENSE` is xterm.js's. To update, `npm pack` the new
versions and copy the same files.
