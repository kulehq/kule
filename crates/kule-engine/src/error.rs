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
    #[error("the workspace changed outside Kule at: {}", .0.join(", "))]
    Conflict(Vec<String>),
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
