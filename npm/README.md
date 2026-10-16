# kule

Kule is a control room for coding agents. Agents do the work; people watch,
approve and steer. Every command an agent runs is a transaction in a
sandboxed copy-on-write copy of the workspace, and a policy decides what
reaches the real workspace.

The command-line tool is written in Rust and installed with Cargo:

```sh
cargo install kule
```

This npm package will carry the desktop app and its tooling. Development
happens at https://github.com/kulehq/kule.
