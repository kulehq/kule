//! Pictures the window may show: only what a header proves to be a PNG,
//! JPEG, GIF or WebP picture of a sane size, so the window never decodes a
//! file that claims to be an image, nor one that would expand to gigabytes.

use imagesize::ImageType;

/// Larger files are not shown.
pub const MAX_IMAGE: usize = 8 << 20;
/// Pictures with more pixels than this are not shown: decoded, they would
/// take more memory than a look at them is worth.
const MAX_PIXELS: u64 = 40_000_000;

/// The names shown as pictures, by extension; the bytes decide in the end.
const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

/// What a picture's header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    pub mime: &'static str,
    pub width: usize,
    pub height: usize,
}

/// Why a file is not shown as a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotShown {
    TooLarge,
    NotAPicture,
}

impl NotShown {
    pub fn reason(self) -> &'static str {
        match self {
            NotShown::TooLarge => "too large to preview",
            NotShown::NotAPicture => "not a picture Kule shows",
        }
    }
}

/// Whether a file named `path` is meant to be a picture.
pub fn named_like_one(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// What `bytes` are, when they are a picture Kule shows.
pub fn picture(bytes: &[u8]) -> Result<Picture, NotShown> {
    if bytes.len() > MAX_IMAGE {
        return Err(NotShown::TooLarge);
    }
    let mime = match imagesize::image_type(bytes) {
        Ok(ImageType::Png) => "image/png",
        Ok(ImageType::Jpeg) => "image/jpeg",
        Ok(ImageType::Gif) => "image/gif",
        Ok(ImageType::Webp) => "image/webp",
        _ => return Err(NotShown::NotAPicture),
    };
    let size = imagesize::blob_size(bytes).map_err(|_| NotShown::NotAPicture)?;
    let pixels = (size.width as u64).saturating_mul(size.height as u64);
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err(NotShown::TooLarge);
    }
    Ok(Picture {
        mime,
        width: size.width,
        height: size.height,
    })
}

#[cfg(test)]
pub(crate) mod tests {
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
    fn a_picture_is_known_by_its_header() {
        assert_eq!(
            picture(&png(3, 2)),
            Ok(Picture {
                mime: "image/png",
                width: 3,
                height: 2
            })
        );
        assert_eq!(picture(&png(50_000, 50_000)), Err(NotShown::TooLarge));
        assert_eq!(picture(&png(0, 5)), Err(NotShown::TooLarge));
        assert_eq!(
            picture(b"<svg onload=alert(1)>"),
            Err(NotShown::NotAPicture)
        );
        let mut big = png(3, 2);
        big.resize(MAX_IMAGE + 1, 0);
        assert_eq!(picture(&big), Err(NotShown::TooLarge));
    }

    #[test]
    fn names_say_what_is_meant_to_be_a_picture() {
        assert!(named_like_one("a/b/Logo.PNG"));
        assert!(named_like_one("x.webp"));
        assert!(!named_like_one("x.svg"));
        assert!(!named_like_one("png"));
    }
}
