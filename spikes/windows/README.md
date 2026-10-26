# Windows measurement spike

A small tool and a few scripts used to measure, on real processes, which
Windows primitives can hold Kule's boundaries (sandbox, network, process
end, copy, daemon socket) before any product code is written. Not product
code and not part of the workspace; it informs the Windows port plan.

- `src/main.rs`: starts a command inside an AppContainer profile or under a
  restricted token, optionally inside a kill-on-close job object, and has
  small probes (token, security descriptors, named pipes, block cloning).
- `scripts/`: the PowerShell drivers used on the test VM. They assume the
  built tool at `C:\kule-setup\ws.exe` and are run as a standard user.

Build on Windows with `cargo build` (MSVC Build Tools and the Windows SDK
are needed). The measurements and the decisions they led to are recorded
in the design notes, not here.
