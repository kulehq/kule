//! Where the window's panels are: which region each sits in, which one is
//! showing, how wide or tall each region is and whether it is open. Kept
//! in Kule's home so the window opens the way it was left.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;

/// Where it is kept, under Kule's home.
pub const LAYOUT_FILE: &str = "layout.json";

/// The panels the window has.
pub const PANELS: &[&str] = &[
    "tasks", "live", "review", "timeline", "files", "preview", "terminal",
];

/// An extension's panel: `ext.<publisher>.<name>.<panel>`, each part 1 to
/// 20 (the panel 18) of `a-z0-9-`. Whether the extension is still there is
/// the window's business; it drops what it does not know.
fn extension_panel(panel: &str) -> bool {
    let part = |s: &str, max: usize| {
        !s.is_empty()
            && s.len() <= max
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let parts: Vec<&str> = panel.split('.').collect();
    matches!(parts.as_slice(), ["ext", publisher, name, local]
        if part(publisher, 20) && part(name, 20) && part(local, 18))
}

/// The regions they can sit in.
pub const REGIONS: &[&str] = &["left", "main", "right", "bottom"];

const MAX_SIZE: u32 = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub panels: Vec<String>,
    pub active: Option<String>,
    pub open: bool,
    /// Width of a side region, height of the bottom one, in pixels.
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub version: u32,
    pub regions: BTreeMap<String, Region>,
    /// The height of the box the agent's prompt is written in, in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<u32>,
}

/// What is kept; `None` when the window never changed its layout.
pub fn load(home: &Path) -> Result<Option<Layout>> {
    let path = home.join(LAYOUT_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Keeps `layout`, whole or not at all; `None` takes it away, so the
/// window starts from its own.
pub fn save(home: &Path, layout: Option<&Layout>) -> Result<()> {
    let path = home.join(LAYOUT_FILE);
    let Some(layout) = layout else {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", path.display()))
            }
            _ => Ok(()),
        };
    };
    check(layout)?;
    let tmp = home.join(format!(".{LAYOUT_FILE}.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(layout).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn check(layout: &Layout) -> Result<()> {
    if !(1..=2).contains(&layout.version) {
        return Err(format!("a layout of version {}", layout.version));
    }
    if layout.composer.is_some_and(|h| h > MAX_SIZE) {
        return Err("the prompt is taller than a screen".to_owned());
    }
    let names: HashSet<&str> = layout.regions.keys().map(String::as_str).collect();
    if names != REGIONS.iter().copied().collect() {
        return Err(
            "a layout names the left, main, right and bottom regions, once each".to_owned(),
        );
    }
    let mut seen = HashSet::new();
    for (name, region) in &layout.regions {
        for panel in &region.panels {
            if !PANELS.contains(&panel.as_str()) && !extension_panel(panel) {
                return Err(format!("{name}: {panel:?} is not a panel"));
            }
            if !seen.insert(panel.as_str()) {
                return Err(format!("{panel} is in the layout twice"));
            }
        }
        if let Some(active) = &region.active
            && !region.panels.contains(active)
        {
            return Err(format!("{name}: {active} shows but is not there"));
        }
        if region.size > MAX_SIZE {
            return Err(format!(
                "{name}: {} pixels is more than a screen",
                region.size
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window's panels are the ones the daemon keeps: a panel the
    /// window has and the daemon does not know makes every layout the
    /// window saves a refused one.
    #[test]
    fn the_window_and_the_daemon_know_the_same_panels() {
        let app = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../kule-desktop/ui/app.js"),
        )
        .unwrap();
        let start = app.find("const PANELS = {").expect("PANELS in app.js");
        let block = &app[start..start + app[start..].find("};").unwrap()];
        let mut window: Vec<&str> = block
            .lines()
            .skip(1)
            .filter_map(|l| l.trim().split_once(':').map(|(k, _)| k))
            .collect();
        let mut daemon = PANELS.to_vec();
        window.sort();
        daemon.sort();
        assert_eq!(window, daemon);
    }

    fn layout() -> Layout {
        let region = |panels: &[&str], open: bool, size: u32| Region {
            panels: panels.iter().map(|p| (*p).to_owned()).collect(),
            active: panels.first().map(|p| (*p).to_owned()),
            open,
            size,
        };
        Layout {
            version: 1,
            regions: [
                ("left".to_owned(), region(&["tasks", "files"], true, 272)),
                ("main".to_owned(), region(&["live", "review"], true, 0)),
                ("right".to_owned(), region(&["timeline"], true, 360)),
                ("bottom".to_owned(), region(&[], false, 240)),
            ]
            .into(),
            composer: Some(96),
        }
    }

    #[test]
    fn a_layout_is_kept_and_read_back_and_can_be_taken_away() {
        let home = tempfile::TempDir::new().unwrap();
        assert_eq!(load(home.path()).unwrap(), None);
        save(home.path(), Some(&layout())).unwrap();
        assert_eq!(load(home.path()).unwrap(), Some(layout()));
        save(home.path(), None).unwrap();
        assert_eq!(load(home.path()).unwrap(), None);
    }

    #[test]
    fn only_known_panels_in_known_regions_once_each() {
        let home = tempfile::TempDir::new().unwrap();
        let mut unknown = layout();
        unknown
            .regions
            .get_mut("right")
            .unwrap()
            .panels
            .push("browser".into());
        let mut twice = layout();
        twice
            .regions
            .get_mut("bottom")
            .unwrap()
            .panels
            .push("live".into());
        let mut region = layout();
        let r = region.regions.remove("bottom").unwrap();
        region.regions.insert("top".into(), r);
        let mut active = layout();
        active.regions.get_mut("bottom").unwrap().active = Some("files".into());
        let mut huge = layout();
        huge.regions.get_mut("left").unwrap().size = 99_999;
        let mut tall = layout();
        tall.composer = Some(99_999);
        for bad in [unknown, twice, region, active, huge, tall] {
            assert!(save(home.path(), Some(&bad)).is_err(), "{bad:?}");
        }
        assert_eq!(load(home.path()).unwrap(), None);
    }

    #[test]
    fn an_extensions_panel_has_the_shape_of_one() {
        let home = tempfile::TempDir::new().unwrap();
        let with = |panel: &str| {
            let mut l = layout();
            l.regions
                .get_mut("right")
                .unwrap()
                .panels
                .push(panel.into());
            l
        };
        assert!(save(home.path(), Some(&with("ext.acme.notes.notes"))).is_ok());
        for bad in [
            "ext.acme.notes",
            "ext.acme.notes.notes.more",
            "ext.Acme.notes.notes",
            "ext.acme..notes",
            "ext.acme.notes.n/../x",
            "ext.acme.notes.this-panel-name-is-too-long",
            "other.acme.notes.notes",
        ] {
            assert!(save(home.path(), Some(&with(bad))).is_err(), "{bad}");
        }
    }
}
