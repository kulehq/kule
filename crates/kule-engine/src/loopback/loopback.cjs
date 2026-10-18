// Kule: a server that asks for every address listens on this computer
// only, so nothing on the network reaches what runs in a task. Loaded into
// every Node process of a task through NODE_OPTIONS. A server given an
// address of its own, a pipe or a handle is left as it is.
"use strict";
const net = require("net");
const ANY = new Set([undefined, null, "", "0.0.0.0", "::", "::0", "0:0:0:0:0:0:0:0"]);
const LOOPBACK = "127.0.0.1";
const listen = net.Server.prototype.listen;
net.Server.prototype.listen = function (...args) {
  const first = args[0];
  if (first !== null && typeof first === "object") {
    // listen(options[, callback])
    const own = first.path !== undefined || first.handle !== undefined || first._handle !== undefined || first.fd !== undefined;
    if (!own && ANY.has(first.host)) args[0] = { ...first, host: LOOPBACK };
  } else if (typeof first === "number" || (typeof first === "string" && /^\d+$/.test(first))) {
    // listen(port[, host][, backlog][, callback])
    const host = args[1];
    if (host === undefined || typeof host === "function" || typeof host === "number") args.splice(1, 0, LOOPBACK);
    else if (ANY.has(host)) args[1] = LOOPBACK;
  } else if (first === undefined || typeof first === "function") {
    // listen([callback]): a free port, on every address
    args.splice(0, first === undefined ? 1 : 0, 0, LOOPBACK);
  }
  return listen.apply(this, args);
};
