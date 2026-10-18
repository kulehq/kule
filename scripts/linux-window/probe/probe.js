// What an extension's frame can and cannot do, kept with kule.storage for
// check.sh to read. A module, so it also shows modules run (with an import).
import { word } from "./dep.js";

const attempt = (f) => {
  try {
    const v = f();
    return v === undefined ? "undefined" : String(v);
  } catch {
    return "blocked";
  }
};

await kule.ready;
const seen = {
  module: word,
  rtc: typeof window.RTCPeerConnection,
  tauri: typeof window.__TAURI_INTERNALS__,
  parent: attempt(() => typeof parent.document.title),
  localStorage: attempt(() => localStorage.length),
  fetch: await fetch("http://127.0.0.1:8766/fetch").then(
    () => "allowed",
    () => "blocked"
  ),
};
await kule.storage.set(seen);
document.getElementById("out").textContent = JSON.stringify(seen, null, 2);
// Last: try to take the frame to another server on the computer. The
// listener must not hear of it.
setTimeout(() => {
  location.href = "http://127.0.0.1:8766/nav?seen=" + encodeURIComponent(JSON.stringify(seen));
}, 1500);
