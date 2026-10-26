//! Terminals inside the window, on Windows: not available yet. The shell
//! inside a task's sandbox comes with a later release, through a helper
//! that opens a pseudo console inside the container; until then opening
//! one is refused, and never started outside the sandbox.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Runtime};

/// Nothing to hold yet: no terminal opens on Windows.
#[derive(Default)]
pub struct Terminals {
    _none: (),
}

#[derive(Serialize)]
pub struct Opened {
    pub id: u64,
    pub shell: String,
    /// What Kule withheld from the shell, for the window to say first.
    pub notes: Vec<String>,
}

/// The name of the shell a terminal would start.
pub fn shell_name() -> String {
    Path::new(&kule_daemon::osfs::default_shell())
        .file_stem()
        .map_or_else(|| "shell".to_owned(), |s| s.to_string_lossy().into_owned())
}

const NOT_YET: &str = "the terminal inside the task's sandbox is not available on Windows yet; it comes with a later release";

impl Terminals {
    pub fn open<R: Runtime>(
        &self,
        _app: AppHandle<R>,
        _home: &Path,
        _task: &str,
        _cols: u16,
        _rows: u16,
        _key: u64,
    ) -> Result<(u64, Vec<String>), String> {
        Err(NOT_YET.to_owned())
    }

    pub fn write(&self, _id: u64, _data: &str) -> Result<(), String> {
        Err(NOT_YET.to_owned())
    }

    pub fn resize(&self, _id: u64, _cols: u16, _rows: u16) -> Result<(), String> {
        Err(NOT_YET.to_owned())
    }

    pub fn any(&self) -> bool {
        false
    }

    pub fn end_all(&self, _within: Duration) {}

    pub fn close(&self, _id: u64) {}
}
