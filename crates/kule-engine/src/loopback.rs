//! Servers a task starts listen on this computer only. Where a server asks
//! for every address (Node's `listen(3000)`, Python's `http.server`, a Puma
//! on `0.0.0.0`), small hooks loaded into Node, Python and Ruby give it the
//! loopback address instead, so nothing on the network reaches what an
//! agent runs, and nothing it runs answers the network past the proxy.
//!
//! The hooks travel through the environment (`NODE_OPTIONS`, `PYTHONPATH`,
//! `RUBYOPT`), which passes through every shell on the way, unlike the
//! `DYLD_*` variables macOS strips at the first system binary. Seatbelt
//! cannot do this itself: its rules see a listening port, but not the
//! address. On Linux the network namespace, or where there is none the
//! bind filter (see `bind_filter`), holds every program to loopback; these
//! hooks keep the servers working there too.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use crate::error::{IoContext, Result};

const NODE: &str = include_str!("loopback/loopback.cjs");
const PYTHON: &str = include_str!("loopback/sitecustomize.py");
const RUBY: &str = include_str!("loopback/loopback.rb");

/// Where the hooks are written, inside the task's temporary directory.
pub(crate) const DIR: &str = ".kule-loopback";

/// Writes the hooks under `tmp` and returns the variables that load them,
/// on top of `current` (what the command's environment already holds).
pub(crate) fn hooks(
    tmp: &Path,
    current: impl Fn(&str) -> Option<OsString>,
) -> Result<Vec<(String, OsString)>> {
    let dir = tmp.join(DIR);
    fs::create_dir_all(&dir).at(&dir)?;
    for (name, text) in [
        ("loopback.cjs", NODE),
        ("sitecustomize.py", PYTHON),
        ("loopback.rb", RUBY),
    ] {
        let path = dir.join(name);
        // Written again only when changed: a run's processes may be reading it.
        if fs::read_to_string(&path).ok().as_deref() != Some(text) {
            fs::write(&path, text).at(&path)?;
        }
    }
    let Some(dir_text) = dir.to_str() else {
        return Ok(Vec::new());
    };
    let mut vars = Vec::new();
    // NODE_OPTIONS takes quotes around a path with spaces.
    let mut node = format!("--require \"{dir_text}/loopback.cjs\"");
    if let Some(more) = current("NODE_OPTIONS").and_then(|v| v.into_string().ok()) {
        node = format!("{node} {more}");
    }
    vars.push(("NODE_OPTIONS".to_owned(), node.into()));
    let mut python = OsString::from(dir_text);
    if let Some(more) = current("PYTHONPATH") {
        python.push(":");
        python.push(more);
    }
    vars.push(("PYTHONPATH".to_owned(), python));
    // RUBYOPT splits on spaces and takes no quotes.
    if !dir_text.contains(char::is_whitespace) {
        let mut ruby = format!("-r{dir_text}/loopback.rb");
        if let Some(more) = current("RUBYOPT").and_then(|v| v.into_string().ok()) {
            ruby = format!("{ruby} {more}");
        }
        vars.push(("RUBYOPT".to_owned(), ruby.into()));
    }
    Ok(vars)
}
