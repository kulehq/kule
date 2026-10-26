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
- `scripts/`: the measurement rounds (`e1`–`e12`). `e8` onwards need an
  interactive desktop session: `run-interactive.ps1` starts a script in the
  caller's own console session through a scheduled task, `run-as-user.ps1`
  (run by an administrator) starts one in another user's console session,
  which is how the standard-user rounds were taken.

Build on Windows with `cargo build` (MSVC Build Tools and the Windows SDK are
needed). The measurements and the decisions they led to are recorded in the
design notes, not here.
