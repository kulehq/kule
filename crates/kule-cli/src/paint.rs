//! Colour and the busy line, for a person at a terminal. Output that goes to
//! a pipe or a file stays plain text, as do runs under `NO_COLOR` or
//! `TERM=dumb`.

use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

static ON: OnceLock<bool> = OnceLock::new();

/// Whether standard output is a terminal that takes colour.
pub fn on() -> bool {
    *ON.get_or_init(|| {
        std::io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
            && std::env::var("TERM").map_or(true, |t| t != "dumb")
    })
}

fn wrap(on: bool, code: &str, text: &str) -> String {
    if on {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

pub fn green(text: &str) -> String {
    wrap(on(), "32", text)
}

pub fn red(text: &str) -> String {
    wrap(on(), "31", text)
}

pub fn yellow(text: &str) -> String {
    wrap(on(), "33", text)
}

pub fn cyan(text: &str) -> String {
    wrap(on(), "36", text)
}

pub fn dim(text: &str) -> String {
    wrap(on(), "2", text)
}

pub fn bold(text: &str) -> String {
    wrap(on(), "1", text)
}

/// `5m 26s`, `42s`, `1h 03m`.
pub fn took(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        _ => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
    }
}

/// What the busy line says: what is going on, since when.
struct Busy {
    started: Instant,
    doing: String,
    /// The agent's answer is mid-line on the terminal: no busy line now.
    mid_line: bool,
    drawn: bool,
}

/// A line on standard error that says the agent is at work, with the time
/// and what it is doing, redrawn until the run ends; nothing unless both
/// outputs are a terminal. Whatever is printed meanwhile goes through
/// [`Spinner::print`], which takes the line away first.
#[derive(Clone)]
pub struct Spinner {
    busy: Option<Arc<Mutex<Busy>>>,
}

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl Spinner {
    pub fn new(agent: &str) -> Spinner {
        let shown = on() && std::io::stderr().is_terminal();
        let busy = shown.then(|| {
            Arc::new(Mutex::new(Busy {
                started: Instant::now(),
                doing: "thinking".to_owned(),
                mid_line: false,
                drawn: false,
            }))
        });
        if let Some(busy) = &busy {
            let busy = Arc::downgrade(busy);
            let agent = agent.to_owned();
            std::thread::spawn(move || {
                let mut frame = 0;
                // Ends once the spinner is dropped.
                while let Some(busy) = busy.upgrade() {
                    {
                        let mut b = busy.lock().unwrap();
                        if !b.mid_line {
                            let line = format!(
                                "{} {agent} is working · {} · {}",
                                FRAMES[frame % FRAMES.len()],
                                took(b.started.elapsed()),
                                b.doing
                            );
                            let mut err = std::io::stderr().lock();
                            let _ = write!(err, "\r\x1b[2K{}", wrap(true, "36", &cut(&line, 100)));
                            let _ = err.flush();
                            b.drawn = true;
                        }
                    }
                    drop(busy);
                    frame += 1;
                    std::thread::sleep(Duration::from_millis(120));
                }
            });
        }
        Spinner { busy }
    }

    /// Says what the agent is doing now.
    pub fn doing(&self, what: &str) {
        if let Some(busy) = &self.busy {
            busy.lock().unwrap().doing = what.to_owned();
        }
    }

    /// Prints `text` on standard output with the busy line out of its way;
    /// `ends_line` says whether the line is complete after it.
    pub fn print(&self, text: &str, ends_line: bool) {
        let mut busy = self.busy.as_ref().map(|b| b.lock().unwrap());
        if let Some(b) = busy.as_deref_mut() {
            if b.drawn {
                let mut err = std::io::stderr().lock();
                let _ = write!(err, "\r\x1b[2K");
                let _ = err.flush();
                b.drawn = false;
            }
            b.mid_line = !ends_line;
        }
        let mut out = std::io::stdout().lock();
        let _ = write!(out, "{text}");
        let _ = out.flush();
    }

    /// Takes the busy line away for good.
    pub fn stop(self) {
        if let Some(busy) = &self.busy {
            let mut b = busy.lock().unwrap();
            if b.drawn {
                eprint!("\r\x1b[2K");
                b.drawn = false;
            }
            b.mid_line = true;
        }
    }
}

/// `text` cut to `max` characters, with an ellipsis when it was longer.
fn cut(text: &str, max: usize) -> String {
    let one_line = text.replace('\n', " ");
    if one_line.chars().count() <= max {
        return one_line;
    }
    let mut short: String = one_line.chars().take(max - 1).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_only_where_it_is_asked_for() {
        assert_eq!(wrap(true, "32", "ok"), "\x1b[32mok\x1b[0m");
        assert_eq!(wrap(false, "32", "ok"), "ok");
    }

    #[test]
    fn times_read_as_a_person_says_them() {
        assert_eq!(took(Duration::from_secs(42)), "42s");
        assert_eq!(took(Duration::from_secs(326)), "5m 26s");
        assert_eq!(took(Duration::from_secs(3780)), "1h 03m");
    }

    #[test]
    fn a_long_title_is_cut_to_one_line() {
        assert_eq!(cut("ls -la", 10), "ls -la");
        assert_eq!(cut("printf 'a\nb'", 100), "printf 'a b'");
        assert_eq!(cut("abcdefghijk", 5), "abcd…");
    }
}
