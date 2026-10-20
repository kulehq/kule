//! The font families installed on this computer, for Settings →
//! Appearance: macOS's from CoreText, Linux's from fontconfig.

/// The most families listed; a computer with more lists these first.
const MAX_FAMILIES: usize = 4000;

/// The font families the window can use, sorted, each once: only names the
/// daemon keeps (letters, digits, spaces and `- _ . & + ' ( )`), so every
/// one listed can be chosen.
#[tauri::command(async)]
pub fn system_fonts() -> Vec<String> {
    let mut families: Vec<String> = installed()
        .into_iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| keepable(name))
        .collect();
    families.sort_by_key(|name| name.to_lowercase());
    families.dedup();
    families.truncate(MAX_FAMILIES);
    families
}

/// Takes out of `fonts` each family `installed` lacks, so a profile from
/// another computer falls back to Kule's own font here; a line for each.
/// With nothing listed (no fontconfig) nothing can be told, and nothing
/// is taken out: a family the window lacks falls back by itself anyway.
pub fn without_missing(
    fonts: &mut kule_daemon::themes::Fonts,
    installed: &[String],
) -> Vec<String> {
    if installed.is_empty() {
        return Vec::new();
    }
    let has = |name: &str| installed.iter().any(|i| i.eq_ignore_ascii_case(name));
    let mut said = Vec::new();
    for (what, family) in [("text", &mut fonts.text), ("code", &mut fonts.code)] {
        if let Some(name) = family.as_deref()
            && !has(name)
        {
            said.push(format!(
                "Fonts: {name} (for {what}) is not on this computer, so Kule's own is used"
            ));
            *family = None;
        }
    }
    said
}

/// A name the daemon keeps as a font family (see `kule_daemon::themes`),
/// and not one of the system's hidden ones (`.AppleSystemUIFont`).
fn keepable(name: &str) -> bool {
    let allowed = |c: char| c.is_alphanumeric() || " -_.&+'()".contains(c);
    !name.is_empty()
        && !name.starts_with('.')
        && name.chars().count() <= 64
        && name.chars().all(allowed)
        && !name.contains("url(")
}

#[cfg(target_os = "macos")]
fn installed() -> Vec<String> {
    use objc2_core_foundation::{CFArray, CFString};
    use objc2_core_text::CTFontManagerCopyAvailableFontFamilyNames;

    // SAFETY: CoreText returns an array of the visible family names, each
    // a CFString, as its documentation says.
    let names = unsafe { CTFontManagerCopyAvailableFontFamilyNames() };
    let names: &CFArray<CFString> = unsafe { names.cast_unchecked() };
    names.iter().map(|name| name.to_string()).collect()
}

/// fontconfig's families, one line per font: `Family,Localized name`.
#[cfg(not(target_os = "macos"))]
fn installed() -> Vec<String> {
    let program = ["/usr/bin/fc-list", "/bin/fc-list"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap_or("fc-list");
    let Ok(out) = std::process::Command::new(program)
        .args([":", "family"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split(',').next())
        .map(|name| name.replace('\\', ""))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_names_the_daemon_keeps_are_listed() {
        for good in [
            "Inter",
            "Fira Code",
            "Source Code Pro (Beta)",
            "ヒラギノ角ゴシック",
        ] {
            assert!(keepable(good), "{good}");
        }
        for bad in [".AppleSystemUIFont", "", "a;b", "x\"y", "a\\b", "x url(y"] {
            assert!(!keepable(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_profile_s_font_this_computer_lacks_falls_back_to_kule_s() {
        let installed = ["Menlo".to_owned(), "Inter".to_owned()];
        let mut fonts = kule_daemon::themes::Fonts {
            text: Some("inter".into()),
            code: Some("Fira Code".into()),
            code_size: 15,
            ..Default::default()
        };
        let said = without_missing(&mut fonts, &installed);
        assert_eq!(fonts.text.as_deref(), Some("inter"));
        assert_eq!(fonts.code, None);
        assert_eq!(fonts.code_size, 15, "the size stays");
        assert_eq!(said.len(), 1);
        assert!(said[0].contains("Fira Code"), "{said:?}");

        // Nothing listed: nothing can be told, nothing changes.
        let mut kept = kule_daemon::themes::Fonts {
            code: Some("Fira Code".into()),
            ..Default::default()
        };
        assert!(without_missing(&mut kept, &[]).is_empty());
        assert_eq!(kept.code.as_deref(), Some("Fira Code"));
    }

    /// Every computer Kule runs on has fonts; none listed means the call
    /// broke.
    #[test]
    fn this_computer_lists_its_fonts() {
        let fonts = system_fonts();
        if cfg!(target_os = "macos") {
            assert!(fonts.iter().any(|f| f == "Menlo"), "{fonts:?}");
        }
        assert!(
            fonts
                .windows(2)
                .all(|w| w[0].to_lowercase() <= w[1].to_lowercase())
        );
    }
}
