# Windows measurement spike, round two: mxc-sdk

A small tool used to measure, on a real Windows 11 machine, what Microsoft's
`mxc-sdk` ProcessContainer holds before Kule's Windows port depends on it:
host support, environment requirements, filesystem precedence, the proxy
path, listeners, process end and ConPTY inside a container. Not product code
and not part of the workspace.

- `src/main.rs`: `probe` prints host facts; `run` starts a command in a
  container with a filesystem, network and environment policy and relays its
  output; `conpty-host` runs a command under a pseudo console (started inside
  a container to test terminals); `http-sink` is a stand-in proxy listener.

Build on Windows with `cargo build` (MSVC Build Tools and the Windows SDK are
needed). The measurements and the decisions they led to are recorded in the
design notes, not here.
