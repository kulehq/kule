use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("path is not valid UTF-8: {}", .0.display())]
    NonUtf8Path(PathBuf),
    /// The workspace no longer looks the way the operation expects, so
    /// applying it would overwrite changes Kule did not make.
    #[error("the workspace changed since Kule last saw it, at: {}", .0.join(", "))]
    Conflict(Vec<String>),
    /// The policy did not let the transaction through: it waits for a
    /// person (`Ask`) or may never pass (`Deny`).
    #[error("the policy holds this transaction ({:?})", .0.verdict)]
    Held(crate::policy::Ruling),
    #[error("task {0} is busy: another kule command holds its lock")]
    Busy(String),
    #[error("state file {}: {source}", path.display())]
    State {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("git repository at {}: {message}", path.display())]
    Git { path: PathBuf, message: String },
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) trait IoContext<T> {
    fn at(self, path: &Path) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: &Path) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}
