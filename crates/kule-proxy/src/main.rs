//! Kule's task proxy as a program of its own: everything it does is in
//! `kule_engine::proxy_program`, so that the proxy Kule serves in its own
//! process on macOS and Linux and the one it runs apart on Windows are the
//! same code.

fn main() -> std::process::ExitCode {
    kule_engine::proxy_program::main(std::env::args_os().skip(1))
}
