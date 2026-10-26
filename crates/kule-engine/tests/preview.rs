//! Pictures in a review: what a changed image was and is.

use std::fs;

use kule_engine::{Picture, Task};
use tempfile::TempDir;

/// A PNG header claiming `width` × `height`.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&width.to_be_bytes());
    png.extend_from_slice(&height.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    png
}

fn size(picture: &Picture) -> Option<(usize, usize)> {
    match picture {
        Picture::Image { width, height, .. } => Some((*width, *height)),
        _ => None,
    }
}

#[test]
fn a_changed_picture_shows_what_it_was_and_is() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(ws.path().join("logo.png"), png(4, 4)).unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    fs::write(task.copy().join("logo.png"), png(8, 2)).unwrap();
    fs::write(task.copy().join("new.png"), png(1, 1)).unwrap();

    let preview = task.preview("logo.png").unwrap();
    assert_eq!(size(&preview.before), Some((4, 4)));
    assert_eq!(size(&preview.after), Some((8, 2)));
    let added = task.preview("new.png").unwrap();
    assert_eq!(added.before, Picture::Absent);
    assert_eq!(size(&added.after), Some((1, 1)));

    // The folder's picture changed meanwhile: what it was is not known.
    fs::write(ws.path().join("logo.png"), png(5, 5)).unwrap();
    assert!(matches!(
        task.preview("logo.png").unwrap().before,
        Picture::Skipped { .. }
    ));
}

#[cfg(unix)]
#[test]
fn only_pictures_inside_the_task_are_shown() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(ws.path().join("notes.md"), "# a\n").unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret.png"), png(2, 2)).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.png"), task.copy().join("a.png"))
        .unwrap();

    assert!(task.preview("notes.md").is_err());
    assert_eq!(task.preview("a.png").unwrap().after, Picture::Absent);
    assert_eq!(
        task.preview("../secret.png").unwrap().after,
        Picture::Absent
    );
}
