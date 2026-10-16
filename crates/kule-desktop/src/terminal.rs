//! Opens the person's own terminal in a task's copy: outside the sandbox,
//! with their shell and rights. What they change there becomes part of the
//! task, like the agent's work, and goes through review the same way.

use std::path::Path;
use std::process::{Command, Stdio};

/// Terminal apps `TERM_PROGRAM` names, and the macOS app for each.
const MACOS_APPS: &[(&str, &str)] = &[
    ("iTerm.app", "iTerm"),
    ("WarpTerminal", "Warp"),
    ("ghostty", "Ghostty"),
    ("WezTerm", "WezTerm"),
    ("Apple_Terminal", "Terminal"),
];

/// Commands tried in turn on Linux; each starts in its working directory.
const LINUX_TERMINALS: &[&str] = &[
    "x-terminal-emulator",
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "alacritty",
    "kitty",
    "xterm",
];

/// The command that opens a terminal in `dir`. `chosen` is `KULE_TERMINAL`
/// (an app name on macOS, a command on Linux); `term_program` is the
/// `TERM_PROGRAM` the app was started with; `exists` says whether a Linux
/// command is on the path.
pub fn command_for(
    macos: bool,
    dir: &Path,
    chosen: Option<&str>,
    term_program: Option<&str>,
    exists: impl Fn(&str) -> bool,
) -> Option<Command> {
    if macos {
        let app = chosen
            .or_else(|| {
                let program = term_program?;
                MACOS_APPS
                    .iter()
                    .find(|(name, _)| *name == program)
                    .map(|(_, app)| *app)
            })
            .unwrap_or("Terminal");
        let mut command = Command::new("open");
        command.arg("-a").arg(app).arg(dir);
        return Some(command);
    }
    let program = match chosen {
        Some(chosen) => chosen,
        None => LINUX_TERMINALS.iter().copied().find(|c| exists(c))?,
    };
    let mut command = Command::new(program);
    command.current_dir(dir);
    Some(command)
}

/// Opens a terminal in `dir`, detached from the app.
pub fn open(dir: &Path) -> Result<(), String> {
    let chosen = std::env::var("KULE_TERMINAL").ok();
    let term_program = std::env::var("TERM_PROGRAM").ok();
    let on_path = |name: &str| {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(name).is_file()))
    };
    let mut command = command_for(
        cfg!(target_os = "macos"),
        dir,
        chosen.as_deref().filter(|c| !c.is_empty()),
        term_program.as_deref(),
        on_path,
    )
    .ok_or("no terminal found; set KULE_TERMINAL to the one you use")?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open a terminal: {e}"))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn parts(command: &Command) -> (String, Vec<String>, Option<String>) {
        (
            command.get_program().to_string_lossy().into_owned(),
            command
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            command
                .get_current_dir()
                .map(|d| d.to_string_lossy().into_owned()),
        )
    }

    #[test]
    fn macos_opens_the_terminal_the_app_was_started_from() {
        let dir = Path::new("/tasks/abc/copy");
        let open =
            |chosen, program| parts(&command_for(true, dir, chosen, program, |_| false).unwrap());
        let args = |app: &str| {
            vec![
                "-a".to_owned(),
                app.to_owned(),
                "/tasks/abc/copy".to_owned(),
            ]
        };
        assert_eq!(
            open(None, Some("iTerm.app")),
            ("open".into(), args("iTerm"), None)
        );
        assert_eq!(open(None, None), ("open".into(), args("Terminal"), None));
        assert_eq!(
            open(None, Some("vscode")),
            ("open".into(), args("Terminal"), None)
        );
        assert_eq!(
            open(Some("Warp"), Some("iTerm.app")),
            ("open".into(), args("Warp"), None)
        );
    }

    #[test]
    fn linux_starts_the_first_terminal_on_the_path_in_the_copy() {
        let dir = Path::new("/tasks/abc/copy");
        let command = command_for(false, dir, None, None, |c| c == "konsole" || c == "xterm");
        let command = command.unwrap();
        assert_eq!(command.get_program(), OsStr::new("konsole"));
        assert_eq!(command.get_current_dir(), Some(dir));

        let chosen = command_for(false, dir, Some("foot"), None, |_| false).unwrap();
        assert_eq!(chosen.get_program(), OsStr::new("foot"));
        assert!(command_for(false, dir, None, None, |_| false).is_none());
    }
}
