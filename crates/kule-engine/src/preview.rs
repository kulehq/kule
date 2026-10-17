//! Images a person can look at in a review: what a changed picture was and
//! is. Both sides are read the way outlines read them (beneath the root, no
//! link followed), and only what a header proves to be a PNG, JPEG, GIF or
//! WebP picture of a sane size is passed on, so the window never decodes a
//! file that claims to be an image, nor one that would expand to gigabytes.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use imagesize::ImageType;
use serde::{Deserialize, Serialize};

use crate::entities::Skip;
use crate::outline::Side;

/// Larger files are not previewed.
pub const MAX_IMAGE: usize = 8 << 20;
/// Pictures with more pixels than this are not previewed: decoded, they
/// would take more memory than a review is worth.
const MAX_PIXELS: u64 = 40_000_000;

/// The names previewed, by extension; the bytes decide in the end.
const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

/// One side of a previewed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "side", rename_all = "snake_case")]
pub enum Picture {
    Image {
        mime: String,
        width: usize,
        height: usize,
        /// The file, base64.
        data: String,
    },
    Absent,
    /// Why there is nothing to show: too large, not a picture Kule shows,
    /// or what it was is no longer known.
    Skipped {
        reason: String,
    },
}

/// A changed picture: what it was and what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub path: String,
    pub before: Picture,
    pub after: Picture,
}

/// Whether files at `path` are previewed at all, by their name.
pub fn previewable(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

pub(crate) fn picture(side: Side) -> Picture {
    let skipped = |reason: &str| Picture::Skipped {
        reason: reason.to_owned(),
    };
    let bytes = match side {
        Side::Bytes(bytes) => bytes,
        Side::Absent => return Picture::Absent,
        Side::Skipped(Skip::BeforeUnknown) => return skipped("changed in the folder too"),
        Side::Skipped(Skip::TooLarge) => return skipped("too large to preview"),
        Side::Skipped(_) => return skipped("not previewed"),
    };
    let mime = match imagesize::image_type(&bytes) {
        Ok(ImageType::Png) => "image/png",
        Ok(ImageType::Jpeg) => "image/jpeg",
        Ok(ImageType::Gif) => "image/gif",
        Ok(ImageType::Webp) => "image/webp",
        _ => return skipped("not a picture Kule shows"),
    };
    let Ok(size) = imagesize::blob_size(&bytes) else {
        return skipped("not a picture Kule shows");
    };
    let pixels = (size.width as u64).saturating_mul(size.height as u64);
    if pixels == 0 || pixels > MAX_PIXELS {
        return skipped("too large to preview");
    }
    Picture::Image {
        mime: mime.to_owned(),
        width: size.width,
        height: size.height,
        data: STANDARD.encode(&bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG header claiming `width` × `height`: what a size check sees.
    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        png
    }

    #[test]
    fn a_picture_is_passed_on_with_its_size() {
        let Picture::Image {
            mime,
            width,
            height,
            data,
        } = picture(Side::Bytes(png(3, 2)))
        else {
            panic!("not an image");
        };
        assert_eq!((mime.as_str(), width, height), ("image/png", 3, 2));
        assert_eq!(STANDARD.decode(data).unwrap(), png(3, 2));
    }

    #[test]
    fn a_picture_that_would_expand_too_far_is_not() {
        assert_eq!(
            picture(Side::Bytes(png(50_000, 50_000))),
            Picture::Skipped {
                reason: "too large to preview".into()
            }
        );
    }

    #[test]
    fn a_file_named_like_a_picture_is_judged_by_its_bytes() {
        assert_eq!(
            picture(Side::Bytes(b"<svg onload=alert(1)>".to_vec())),
            Picture::Skipped {
                reason: "not a picture Kule shows".into()
            }
        );
        // A BMP, which the window could show but Kule does not pass on.
        let mut bmp = b"BM".to_vec();
        bmp.extend([0; 12]);
        bmp.extend(40u32.to_le_bytes());
        bmp.extend(3i32.to_le_bytes());
        bmp.extend(2i32.to_le_bytes());
        assert_eq!(
            picture(Side::Bytes(bmp)),
            Picture::Skipped {
                reason: "not a picture Kule shows".into()
            }
        );
        assert!(previewable("a/B.JPG"));
        assert!(!previewable("logo.svg"));
    }
}
