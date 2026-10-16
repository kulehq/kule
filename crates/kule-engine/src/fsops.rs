//! Filesystem moves shared by task creation, commit, rollback and undo.

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use filetime::FileTime;
use serde::Serialize;
use serde::de::DeserializeOwned;
use walkdir::WalkDir;

use crate::error::{Error, IoContext, Result};
use crate::manifest::{Kind, Stamp, relative};

/// Clones the tree under `src` into `dst`, which must not exist yet. Files are
/// reflinked where the filesystem allows it and copied otherwise; modes and
/// modification times are kept so incremental builds see the same tree.
pub fn clone_tree(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir(dst).at(dst)?;
    let mut dirs = Vec::new();
    for entry in WalkDir::new(src)
        .min_depth(1)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(|e| Error::Io {
            path: e.path().unwrap_or(src).to_path_buf(),
            source: e.into(),
        })?;
        let Some(stamp) = Stamp::of(entry.path())? else {
            continue;
        };
        let to = dst.join(relative(src, entry.path())?);
        if stamp.kind == Kind::Dir {
            fs::create_dir(&to).at(&to)?;
            dirs.push((to, stamp));
        } else {
            place(entry.path(), &to, &stamp)?;
        }
    }
    for (dir, stamp) in dirs.iter().rev() {
        finish(dir, stamp)?;
    }
    if let Some(root) = Stamp::of(src)? {
        finish(dst, &root)?;
    }
    Ok(())
}

/// Makes each of `paths` under `dst` what it is under `src`; a path missing
/// under `src` is removed from `dst`. `paths` must be sorted so that parents
/// come before their children, and their parents must exist under `dst`.
/// With `atomic`, files and symlinks arrive through a rename, so nobody
/// reading `dst` sees half a file.
pub fn mirror(src: &Path, dst: &Path, paths: &[String], atomic: bool) -> Result<()> {
    let stamps = paths
        .iter()
        .map(|p| Ok((Stamp::of(&src.join(p))?, Stamp::of(&dst.join(p))?)))
        .collect::<Result<Vec<_>>>()?;

    // 1. Clear what has to go, children before parents.
    for (p, (s, d)) in paths.iter().zip(&stamps).rev() {
        let Some(d) = d else { continue };
        let keep = match s {
            None => false,
            Some(s) if s.kind == Kind::Dir || d.kind == Kind::Dir => s.kind == d.kind,
            Some(_) => atomic,
        };
        if !keep {
            remove(&dst.join(p))?;
        }
    }

    // 2. Bring in what `src` has, parents before children.
    for (p, (s, _)) in paths.iter().zip(&stamps) {
        let Some(s) = s else { continue };
        let (from, to) = (src.join(p), dst.join(p));
        if s.kind == Kind::Dir {
            if Stamp::of(&to)?.is_none() {
                fs::create_dir(&to).at(&to)?;
            }
        } else if atomic {
            let tmp = temp_beside(&to);
            let placed = place(&from, &tmp, s).and_then(|()| fs::rename(&tmp, &to).at(&to));
            if placed.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            placed?;
        } else {
            place(&from, &to, s)?;
        }
    }

    // 3. Directory modes and times last: adding a child moves a directory's mtime.
    for (p, (s, _)) in paths.iter().zip(&stamps).rev() {
        if let Some(s) = s.as_ref().filter(|s| s.kind == Kind::Dir) {
            finish(&dst.join(p), s)?;
        }
    }
    Ok(())
}

/// Puts the file or symlink described by `stamp` (read from `from`) at `to`,
/// which must not exist.
fn place(from: &Path, to: &Path, stamp: &Stamp) -> Result<()> {
    match stamp.kind {
        Kind::File => {
            reflink_copy::reflink_or_copy(from, to).at(from)?;
            finish(to, stamp)
        }
        Kind::Symlink => symlink(stamp.link.as_deref().unwrap_or_default(), to).at(to),
        Kind::Dir => fs::create_dir(to).at(to),
    }
}

fn finish(path: &Path, stamp: &Stamp) -> Result<()> {
    let mtime = FileTime::from_unix_time(stamp.mtime.0, stamp.mtime.1 as u32);
    filetime::set_file_mtime(path, mtime).at(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(stamp.mode)).at(path)
}

fn temp_beside(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".kule-tmp-{}-{n}", std::process::id()))
}

/// Removes a file, symlink or whole directory; a missing path is not an error.
pub fn remove(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).at(path),
        Ok(meta) if meta.is_dir() => remove_dir_tree(path),
        Ok(_) => fs::remove_file(path).at(path),
    }
}

/// `remove_dir_all` that also gets through read-only directories.
pub fn remove_dir_tree(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            for entry in WalkDir::new(path).follow_links(false).into_iter().flatten() {
                if entry.file_type().is_dir() {
                    let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o700));
                }
            }
            fs::remove_dir_all(path).at(path)
        }
        other => other.at(path),
    }
}

pub fn same_bytes(a: &Path, b: &Path) -> Result<bool> {
    let (mut fa, mut fb) = (File::open(a).at(a)?, File::open(b).at(b)?);
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let na = read_full(&mut fa, &mut ba).at(a)?;
        let nb = read_full(&mut fb, &mut bb).at(b)?;
        if ba[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

fn read_full(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read(path).at(path)?;
    serde_json::from_slice(&text).map_err(|source| Error::State {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes through a rename, so a crash leaves the old file or the new one.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_vec_pretty(value).map_err(|source| Error::State {
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = temp_beside(path);
    fs::write(&tmp, text).at(&tmp)?;
    fs::rename(&tmp, path).at(path)
}
