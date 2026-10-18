use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The manifest, the package or a value is not acceptable; the text says why.
    #[error("{0}")]
    Invalid(String),
    #[error("no extension {0}")]
    NotFound(String),
    /// The extension may not run until the person approves it.
    #[error("extension {id} is held: {why}")]
    Held { id: String, why: String },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(text: impl Into<String>) -> Error {
    Error::Invalid(text.into())
}

pub(crate) trait At<T> {
    fn at(self, path: &std::path::Path) -> Result<T>;
}

impl<T> At<T> for std::io::Result<T> {
    fn at(self, path: &std::path::Path) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}
