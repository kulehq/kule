//! Which changed paths the workspace's own ignore rules exclude: its
//! `.gitignore` files and `.git/info/exclude`. A commit leaves them in the
//! copy unless asked to take them, so build output, caches and files such as
//! `.env` do not reach the workspace by default.
//!
//! The rules are read from the workspace, never from the copy: an agent
//! cannot decide what is ignored by editing `.gitignore`. Like the rest of
//! Kule's git reading, the repository is opened without global
//! configuration, so a global excludes file does not count. A workspace that
//! is not the root of a repository has no ignored paths.

use std::path::Path;

use gix::index::entry::Mode;
use gix::worktree::stack::state::ignore::Source;

use crate::effects::Effect;
use crate::error::{Error, Result};
use crate::git;
use crate::manifest::Kind;

/// Sets [`Effect::ignored`] on each of `effects` the workspace's rules
/// exclude. Tracked files are never ignored, as in git.
pub(crate) fn mark(work: &Path, effects: &mut [Effect]) -> Result<()> {
    if !work.join(".git").is_dir() || effects.is_empty() {
        return Ok(());
    }
    let fail = |e: &dyn std::fmt::Display| Error::Git {
        path: work.to_path_buf(),
        message: e.to_string(),
    };
    let repo = git::open(work)?;
    let index = repo.index_or_empty().map_err(|e| fail(&e))?;
    let mut stack = repo
        .excludes(&index, None, Source::WorktreeThenIdMappingIfNotSkipped)
        .map_err(|e| fail(&e))?;
    for effect in effects {
        if git::classify(&effect.path).is_some()
            || index.entry_by_path(effect.path.as_str().into()).is_some()
        {
            continue;
        }
        // The stack checks the parent directories on the way down, so a
        // path under an ignored directory is excluded too.
        let mode = if effect.kind == Kind::Dir {
            Mode::DIR
        } else {
            Mode::FILE
        };
        effect.ignored = stack
            .at_entry(effect.path.as_str(), Some(mode))
            .map_err(|e| fail(&e))?
            .is_excluded();
    }
    Ok(())
}
