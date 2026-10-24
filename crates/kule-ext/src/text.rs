//! Checks for the words and names an extension shows the person.

use crate::error::{Result, invalid};

/// Characters that show nothing yet change what text means or how it
/// reads: control characters, the marks that turn text around, zero-width
/// ones, the soft hyphen, line and paragraph separators, and the tag
/// characters text can hide words in. The window's own set (`UNSEEN` in
/// the app's highlight.js); the app's tests keep the two the same.
pub fn unseen(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{2069}'
                | '\u{FEFF}'
                | '\u{E0000}'..='\u{E007F}'
        )
}

/// Text shown to the person: not empty, at most `max` characters, no control
/// or direction characters, no space at either end.
pub(crate) fn words(what: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{what} is empty")));
    }
    if value.trim() != value {
        return Err(invalid(format!("{what} starts or ends with a space")));
    }
    if value.chars().count() > max {
        return Err(invalid(format!("{what} is longer than {max} characters")));
    }
    if value.chars().any(unseen) {
        return Err(invalid(format!(
            "{what} holds a control, text-direction or invisible character"
        )));
    }
    Ok(())
}

/// A name made of lowercase letters, digits and `-`, from 1 to `max` long,
/// not starting or ending with `-`.
pub(crate) fn slug(what: &str, value: &str, max: usize) -> Result<()> {
    let ok = !value.is_empty()
        && value.len() <= max
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(invalid(format!(
            "{what} {value:?} must be 1 to {max} lowercase letters, digits or '-'"
        )))
    }
}

/// A plain host name: lowercase labels of letters, digits and `-`, at least
/// two of them, a top-level label that is not a number; no scheme, port,
/// path or wildcard.
pub fn host_ok(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && !labels
            .last()
            .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()))
}

/// `X.Y.Z`, optionally followed by `-pre` or `+build` of letters, digits, `.` and `-`.
pub(crate) fn version_ok(version: &str) -> bool {
    let (core, rest) = match version.find(['-', '+']) {
        Some(at) => (&version[..at], &version[at + 1..]),
        None => (version, ""),
    };
    let parts: Vec<&str> = core.split('.').collect();
    version.len() <= 40
        && parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
        && (version.len() == core.len()
            || (!rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_plain_names() {
        for ok in ["hooks.slack.com", "api.example.io", "a-b.example.com"] {
            assert!(host_ok(ok), "{ok}");
        }
        for bad in [
            "localhost",
            "https://x.com",
            "x.com:443",
            "*.x.com",
            "X.com",
            "10.0.0.1",
            "x..com",
            "-x.com",
            "x.com/",
            "",
        ] {
            assert!(!host_ok(bad), "{bad}");
        }
    }

    #[test]
    fn versions_have_three_numbers() {
        for ok in ["1.0.0", "0.12.3-beta.1", "2.0.0+build-5"] {
            assert!(version_ok(ok), "{ok}");
        }
        for bad in ["1.0", "1.0.0.0", "v1.0.0", "1.0.0-", "1.x.0", "1.0.0-a b"] {
            assert!(!version_ok(bad), "{bad}");
        }
    }

    #[test]
    fn what_shows_nothing_is_refused_in_words() {
        for c in [
            '\u{200B}',
            '\u{2028}',
            '\u{FEFF}',
            '\u{E0041}',
            '\u{00AD}',
            '\u{202E}',
        ] {
            assert!(unseen(c), "U+{:04X}", c as u32);
            assert!(
                words("name", &format!("a{c}b"), 40).is_err(),
                "U+{:04X}",
                c as u32
            );
        }
    }

    #[test]
    fn shown_words_hide_nothing() {
        assert!(words("name", "Lockfile notes", 40).is_ok());
        assert!(words("name", "Pay\u{202E}lacol", 40).is_err());
        assert!(words("name", "a\nb", 40).is_err());
        assert!(words("name", " a", 40).is_err());
        assert!(words("name", "", 40).is_err());
        assert!(words("name", &"x".repeat(41), 40).is_err());
    }
}
