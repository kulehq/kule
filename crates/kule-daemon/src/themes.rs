//! The app's themes: which one is in use, and the ones the person made,
//! kept in Kule's home. Kule's own themes live in the app; the daemon keeps
//! only the person's and the choice.
//!
//! A theme is a set of colours for the window's named parts and nothing
//! else: every colour is `#rrggbb` and every name one the window knows, so
//! a theme from a file can change how Kule looks but carries no CSS, no
//! URLs and no script into it.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;

/// Where they are kept, under Kule's home.
pub const THEMES_FILE: &str = "themes.json";

/// The parts of the window a theme colours.
pub const TOKENS: &[&str] = &[
    "bg",
    "surface",
    "sunken",
    "border",
    "text",
    "muted",
    "faint",
    "accent",
    "accent-text",
    "accent-soft",
    "ok",
    "ok-soft",
    "ask",
    "ask-soft",
    "deny",
    "deny-soft",
];

/// The largest theme file read, far more than a theme needs.
pub const MAX_FILE: u64 = 64 * 1024;

const MAX_CUSTOM: usize = 64;
const MAX_NAME: usize = 40;

/// The version of the file format below.
pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Light,
    Dark,
}

/// A theme the person made, or took in from a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub scheme: Scheme,
    /// Part → `#rrggbb`; a part left out keeps the scheme's own colour.
    pub colors: BTreeMap<String, String>,
}

/// Whether the window's parts move as they change: as the computer's
/// "reduce motion" says, or always, or never.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    #[default]
    System,
    On,
    Off,
}

/// What is kept: the theme in use (`system`, one of Kule's, or one of
/// `custom`), the person's own, the motion, and whether text the person
/// selects with the mouse is copied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Themes {
    pub current: String,
    #[serde(default)]
    pub custom: Vec<Theme>,
    #[serde(default)]
    pub motion: Motion,
    /// Text selected with the mouse goes to the clipboard at once.
    #[serde(default = "copied_by_default")]
    pub copy_on_select: bool,
    /// The fonts the window uses, and their sizes.
    #[serde(default)]
    pub fonts: Fonts,
}

/// The window's fonts: one for its text, one for code, the agent chat and
/// terminals. A family is a name the computer has, which the window puts
/// in front of Kule's own; `None` keeps Kule's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fonts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// In pixels, [`TEXT_SIZES`].
    #[serde(default = "text_size")]
    pub text_size: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// In pixels, [`CODE_SIZES`].
    #[serde(default = "code_size")]
    pub code_size: u8,
}

/// The sizes the window's text may take, and Kule's own.
pub const TEXT_SIZES: std::ops::RangeInclusive<u8> = 11..=20;
pub const CODE_SIZES: std::ops::RangeInclusive<u8> = 10..=22;
const TEXT_SIZE: u8 = 14;
const CODE_SIZE: u8 = 13;
/// The longest font family name kept.
const MAX_FAMILY: usize = 64;

fn text_size() -> u8 {
    TEXT_SIZE
}

fn code_size() -> u8 {
    CODE_SIZE
}

impl Default for Fonts {
    fn default() -> Fonts {
        Fonts {
            text: None,
            text_size: TEXT_SIZE,
            code: None,
            code_size: CODE_SIZE,
        }
    }
}

fn copied_by_default() -> bool {
    true
}

impl Default for Themes {
    fn default() -> Themes {
        Themes {
            current: "system".to_owned(),
            custom: Vec::new(),
            motion: Motion::System,
            copy_on_select: true,
            fonts: Fonts::default(),
        }
    }
}

/// A theme as a file carries it, from one person's Kule to another's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    pub kule_theme: u32,
    pub name: String,
    pub scheme: Scheme,
    pub colors: BTreeMap<String, String>,
}

/// What is kept; Kule's defaults when nothing was ever chosen. A file that
/// cannot be read is reported rather than taken as empty, so a save never
/// writes over themes that are still there.
pub fn load(home: &Path) -> Result<Themes> {
    let path = home.join(THEMES_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Themes::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Keeps `themes` in place of what was there, whole or not at all; the
/// colours are kept in lower case.
pub fn save(home: &Path, themes: &Themes) -> Result<Themes> {
    let themes = checked(themes)?;
    let path = home.join(THEMES_FILE);
    let tmp = home.join(format!(".{THEMES_FILE}.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&themes).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })?;
    Ok(themes)
}

/// A theme file's contents, read and checked; refused whole when anything
/// in it is not a theme.
pub fn read_file(bytes: &[u8]) -> Result<ThemeFile> {
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("a theme file is at most {} KiB", MAX_FILE / 1024));
    }
    let file: ThemeFile =
        serde_json::from_slice(bytes).map_err(|e| format!("not a Kule theme: {e}"))?;
    if file.kule_theme != FORMAT {
        return Err(format!(
            "a theme of format {}, and this Kule reads format {FORMAT}",
            file.kule_theme
        ));
    }
    Ok(ThemeFile {
        name: name(&file.name)?,
        colors: colors(&file.colors)?,
        ..file
    })
}

/// The file `theme` is shared as.
pub fn to_file(theme: &ThemeFile) -> Result<String> {
    let checked = ThemeFile {
        kule_theme: FORMAT,
        name: name(&theme.name)?,
        scheme: theme.scheme,
        colors: colors(&theme.colors)?,
    };
    serde_json::to_string_pretty(&checked).map_err(|e| e.to_string())
}

pub(crate) fn checked(themes: &Themes) -> Result<Themes> {
    if !id_ok(&themes.current) {
        return Err(format!("{:?} is not a theme", themes.current));
    }
    if themes.custom.len() > MAX_CUSTOM {
        return Err(format!("at most {MAX_CUSTOM} themes of your own"));
    }
    let mut custom = Vec::with_capacity(themes.custom.len());
    for theme in &themes.custom {
        if !id_ok(&theme.id) || custom.iter().any(|t: &Theme| t.id == theme.id) {
            return Err(format!(
                "{:?} is not a theme's id, or it is there twice",
                theme.id
            ));
        }
        custom.push(Theme {
            id: theme.id.clone(),
            name: name(&theme.name)?,
            scheme: theme.scheme,
            colors: colors(&theme.colors)?,
        });
    }
    Ok(Themes {
        current: themes.current.clone(),
        custom,
        motion: themes.motion,
        copy_on_select: themes.copy_on_select,
        fonts: fonts(&themes.fonts)?,
    })
}

fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `raw` with each family a font name and each size in its range. A family
/// goes into the window's style as a quoted name, so it holds letters,
/// digits, spaces and `- _ . & + ' ( )` only: no quote or backslash to end
/// the name, no `;` or `{}` to start a rule, no `url(` to fetch anything.
fn fonts(raw: &Fonts) -> Result<Fonts> {
    let family = |raw: &Option<String>| -> Result<Option<String>> {
        let Some(name) = raw.as_deref().map(str::trim).filter(|n| !n.is_empty()) else {
            return Ok(None);
        };
        let allowed = |c: char| c.is_alphanumeric() || " -_.&+'()".contains(c);
        if name.chars().count() > MAX_FAMILY || !name.chars().all(allowed) || name.contains("url(")
        {
            return Err(format!(
                "{name:?} is not a font name Kule takes: up to {MAX_FAMILY} letters, digits, spaces and - _ . & + ' ( )"
            ));
        }
        Ok(Some(name.to_owned()))
    };
    let size = |size: u8, range: std::ops::RangeInclusive<u8>, what: &str| -> Result<u8> {
        if range.contains(&size) {
            Ok(size)
        } else {
            Err(format!(
                "{what} size is {} to {} pixels",
                range.start(),
                range.end()
            ))
        }
    };
    Ok(Fonts {
        text: family(&raw.text)?,
        text_size: size(raw.text_size, TEXT_SIZES, "the text")?,
        code: family(&raw.code)?,
        code_size: size(raw.code_size, CODE_SIZES, "the code")?,
    })
}

/// A theme's name, trimmed: something to read, not something that can
/// turn the text around it.
fn name(raw: &str) -> Result<String> {
    let name = raw.trim();
    let turns = |c: char| matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(format!("a theme's name is 1 to {MAX_NAME} characters"));
    }
    if name.chars().any(|c| c.is_control() || turns(c)) {
        return Err("a theme's name cannot hold control or text-direction characters".to_owned());
    }
    Ok(name.to_owned())
}

fn colors(raw: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
    let mut colors = BTreeMap::new();
    for (part, value) in raw {
        if !TOKENS.contains(&part.as_str()) {
            return Err(format!(
                "{part:?} is not a part of the window a theme colours"
            ));
        }
        let hex = value.len() == 7
            && value.starts_with('#')
            && value[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !hex {
            return Err(format!("{part}: {value:?} is not a colour like #1b1f27"));
        }
        colors.insert(part.clone(), value.to_ascii_lowercase());
    }
    Ok(colors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(id: &str, name: &str, colors: &[(&str, &str)]) -> Theme {
        Theme {
            id: id.into(),
            name: name.into(),
            scheme: Scheme::Dark,
            colors: colors
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
        }
    }

    #[test]
    fn themes_are_kept_and_read_back_with_the_choice() {
        let home = tempfile::TempDir::new().unwrap();
        assert_eq!(load(home.path()).unwrap(), Themes::default());

        let mine = Themes {
            current: "custom-1".into(),
            custom: vec![theme(
                "custom-1",
                " Harbour ",
                &[("bg", "#0B1A2A"), ("accent", "#ff5a3c")],
            )],
            motion: Motion::Off,
            copy_on_select: false,
            fonts: Fonts {
                text: Some(" Inter ".into()),
                text_size: 16,
                code: Some("JetBrains Mono".into()),
                code_size: 15,
            },
        };
        let kept = save(home.path(), &mine).unwrap();
        assert_eq!(kept.custom[0].name, "Harbour");
        assert_eq!(kept.custom[0].colors["bg"], "#0b1a2a");
        assert_eq!(kept.motion, Motion::Off);
        assert!(!kept.copy_on_select);
        assert_eq!(kept.fonts.text.as_deref(), Some("Inter"));
        assert_eq!((kept.fonts.text_size, kept.fonts.code_size), (16, 15));
        assert_eq!(load(home.path()).unwrap(), kept);
    }

    #[test]
    fn a_font_is_a_name_and_a_size_in_range_and_nothing_more() {
        let home = tempfile::TempDir::new().unwrap();
        let with = |fonts: Fonts| Themes {
            fonts,
            ..Themes::default()
        };
        let named = |name: &str| Fonts {
            text: Some(name.into()),
            ..Fonts::default()
        };
        for good in [
            "Inter",
            "Fira Code",
            "Noto Sans CJK JP",
            "ヒラギノ角ゴシック",
            "Source Code Pro (Beta)",
        ] {
            assert!(save(home.path(), &with(named(good))).is_ok(), "{good}");
        }
        // Each would end the quoted name, start a rule, or fetch something.
        for bad in [
            "x\"; } body { display: none",
            "x\\\"",
            "a;b",
            "a{b}",
            "x url(https:",
            "tab\there",
            "a/b",
            &"n".repeat(MAX_FAMILY + 1),
        ] {
            assert!(save(home.path(), &with(named(bad))).is_err(), "{bad:?}");
            let code = Fonts {
                code: Some(bad.into()),
                ..Fonts::default()
            };
            assert!(save(home.path(), &with(code)).is_err(), "{bad:?}");
        }
        for (text_size, code_size) in [(10, 13), (21, 13), (14, 9), (14, 23)] {
            let sized = Fonts {
                text_size,
                code_size,
                ..Fonts::default()
            };
            assert!(
                save(home.path(), &with(sized)).is_err(),
                "{text_size} {code_size}"
            );
        }
        // A blank name is no name: Kule's own font.
        let kept = save(home.path(), &with(named("  "))).unwrap();
        assert_eq!(kept.fonts.text, None);
    }

    #[test]
    fn a_file_from_before_the_motion_choice_follows_the_system() {
        let home = tempfile::TempDir::new().unwrap();
        std::fs::write(
            home.path().join(THEMES_FILE),
            r#"{ "current": "system", "custom": [] }"#,
        )
        .unwrap();
        assert_eq!(load(home.path()).unwrap().motion, Motion::System);
        // Nor the copy choice: text selected is copied.
        assert!(load(home.path()).unwrap().copy_on_select);
        // Nor fonts: Kule's own, at its sizes.
        assert_eq!(load(home.path()).unwrap().fonts, Fonts::default());

        let chosen = r#"{ "current": "system", "motion": "on" }"#;
        let read: Themes = serde_json::from_str(chosen).unwrap();
        assert_eq!(read.motion, Motion::On);
        assert!(
            serde_json::from_str::<Themes>(r#"{ "current": "system", "motion": "fast" }"#).is_err()
        );
    }

    #[test]
    fn only_colours_for_known_parts_get_in() {
        let home = tempfile::TempDir::new().unwrap();
        let refused = [
            theme("t", "x", &[("bg", "red")]),
            theme("t", "x", &[("bg", "url(https://evil.example/x.png)")]),
            theme("t", "x", &[("bg", "#fff; background: url(x)")]),
            theme("t", "x", &[("font", "#ffffff")]),
            theme("t", "x", &[("bg", "#12345")]),
            theme("T!", "x", &[]),
            theme("t", "", &[]),
            theme("t", "abc\u{202E}txt", &[]),
            theme("t", &"n".repeat(MAX_NAME + 1), &[]),
        ];
        for bad in refused {
            let themes = Themes {
                custom: vec![bad.clone()],
                ..Themes::default()
            };
            assert!(save(home.path(), &themes).is_err(), "{bad:?}");
        }
        let twice = Themes {
            custom: vec![theme("t", "a", &[]), theme("t", "b", &[])],
            ..Themes::default()
        };
        assert!(save(home.path(), &twice).is_err());
        assert_eq!(load(home.path()).unwrap(), Themes::default());
    }

    #[test]
    fn a_theme_file_goes_out_and_comes_back_the_same() {
        let file = ThemeFile {
            kule_theme: FORMAT,
            name: "Harbour".into(),
            scheme: Scheme::Light,
            colors: [("text".to_owned(), "#1B1F27".to_owned())].into(),
        };
        let text = to_file(&file).unwrap();
        let back = read_file(text.as_bytes()).unwrap();
        assert_eq!(back.colors["text"], "#1b1f27");
        assert_eq!(
            (back.name.as_str(), back.scheme),
            ("Harbour", Scheme::Light)
        );

        // Anything else in it, another format, too much of it: refused.
        let extra = text.replacen("\"name\"", "\"script\": \"x\", \"name\"", 1);
        assert!(read_file(extra.as_bytes()).is_err());
        let newer = text.replacen("\"kule_theme\": 1", "\"kule_theme\": 2", 1);
        assert!(
            read_file(newer.as_bytes())
                .unwrap_err()
                .contains("format 2")
        );
        assert!(read_file(&vec![b' '; MAX_FILE as usize + 1]).is_err());
        assert!(read_file(b"not json").is_err());
    }
}
