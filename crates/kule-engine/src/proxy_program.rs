//! What the `kule-proxy` program does: it serves a task's proxy in a
//! process of its own, as [`crate::proxy_pipe`] describes, for the
//! platforms where the proxy runs apart from Kule (Windows, inside an app
//! container). The program is the one piece of Kule that process is, so
//! its code stays here, in the engine, next to the proxy it serves.

use std::ffi::OsString;
use std::process::ExitCode;

/// Runs the program with its arguments (after its own name). Without any
/// it serves on its standard pipes. Two more are for Kule's tests of the
/// process it runs in: `--probe PATH` reads `PATH` and exits with 0 when
/// it could, 2 when it could not; `--hold SECONDS` sleeps that long,
/// touching nothing, not even its pipes.
pub fn main(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args: Vec<OsString> = args.into_iter().collect();
    match args.as_slice() {
        [] => match crate::proxy_pipe::run_child(std::io::stdin(), std::io::stdout()) {
            0 => ExitCode::SUCCESS,
            code => ExitCode::from(code as u8),
        },
        [flag, path] if flag == "--probe" => match std::fs::read(path) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("kule-proxy: cannot read {}: {e}", path.to_string_lossy());
                ExitCode::from(2)
            }
        },
        [flag, seconds] if flag == "--hold" => {
            let seconds: u64 = seconds
                .to_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60)
                .min(3600);
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: kule-proxy [--probe PATH | --hold SECONDS]");
            ExitCode::from(64)
        }
    }
}
