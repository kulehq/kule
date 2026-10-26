//! `kule-pty-helper`: the person's shell on a pseudo console, inside the
//! task's container (see the library's notes). Kule starts it; nothing
//! else is meant to.

#[cfg(windows)]
mod conpty;

fn main() {
    let args = match kule_pty_helper::Args::parse(std::env::args_os()) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("kule-pty-helper: {e}");
            std::process::exit(2);
        }
    };
    #[cfg(windows)]
    match conpty::run(&args) {
        Ok(code) => std::process::exit(code as i32),
        Err(e) => {
            eprintln!("kule-pty-helper: {e}");
            std::process::exit(2);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        eprintln!(
            "kule-pty-helper: only Windows needs it; elsewhere Kule gives a shell its own terminal"
        );
        std::process::exit(2);
    }
}
