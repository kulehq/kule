//! A profile: the person's choices in one file, to take to another
//! computer or keep. It carries what shapes how Kule looks and works for
//! them, each part only when they ask for it: the themes, the shortcuts,
//! Kule's policy they chose, the daily budget, the limits on every task's
//! runs and their own agents.
//!
//! What would let something through without asking never travels: no
//! secret's value, no remembered approval, no policy they wrote themselves.
//! An agent from a profile is saved like one written by hand, so it runs
//! only once approved here.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Result, shortcuts, themes};

/// The version of the file format.
pub const FORMAT: u32 = 1;

/// The largest profile file read.
pub const MAX_FILE: u64 = 256 * 1024;

const MAX_AGENTS: usize = 64;

/// The parts a profile can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Appearance,
    Shortcuts,
    Policy,
    Budget,
    Limits,
    Agents,
}

/// The daily budget, or none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub daily: Option<kule_engine::Money>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub kule_profile: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<themes::Themes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcuts: Option<shortcuts::Shortcuts>,
    /// Balanced or careful; a policy of the person's own is not carried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<kule_engine::PolicyChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
    /// The limits of every task that sets none of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<kule_engine::Limits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<kule_acp::Profile>>,
}

/// The file with `parts` of what is kept under `home`.
pub fn export(home: &Path, parts: &[Part]) -> Result<String> {
    let mut file = ProfileFile {
        kule_profile: FORMAT,
        ..Default::default()
    };
    for part in parts {
        match part {
            Part::Appearance => file.appearance = Some(themes::load(home)?),
            Part::Shortcuts => file.shortcuts = Some(shortcuts::load(home)?),
            Part::Policy => {
                let choice = kule_engine::policy_choice(home).map_err(|e| e.to_string())?;
                if choice == kule_engine::PolicyChoice::Own {
                    return Err(
                        "your policy is one you wrote; a profile carries only Kule's balanced or careful policy, so leave Policy out".to_owned(),
                    );
                }
                file.policy = Some(choice);
            }
            Part::Budget => {
                file.budget = Some(Budget {
                    daily: kule_engine::daily_budget(home).map_err(|e| e.to_string())?,
                })
            }
            Part::Limits => {
                file.limits = Some(kule_engine::limits(home).map_err(|e| e.to_string())?)
            }
            Part::Agents => {
                let agents = kule_acp::Agents::load(home).map_err(|e| e.to_string())?;
                file.agents = Some(
                    agents
                        .listed()
                        .iter()
                        .filter(|l| l.origin == kule_acp::Origin::Person)
                        .map(|l| l.profile.clone())
                        .collect(),
                )
            }
        }
    }
    serde_json::to_string_pretty(&file).map_err(|e| e.to_string())
}

/// A profile file's contents, read and checked part by part; refused whole
/// when any part is not what it should be.
pub fn read(text: &str) -> Result<ProfileFile> {
    if text.len() as u64 > MAX_FILE {
        return Err(format!("a profile is at most {} KiB", MAX_FILE / 1024));
    }
    let file: ProfileFile =
        serde_json::from_str(text).map_err(|e| format!("not a Kule profile: {e}"))?;
    if file.kule_profile != FORMAT {
        return Err(format!(
            "a profile of format {}, and this Kule reads format {FORMAT}",
            file.kule_profile
        ));
    }
    let appearance = match &file.appearance {
        Some(t) => Some(themes::checked(t).map_err(|e| format!("appearance: {e}"))?),
        None => None,
    };
    if let Some(s) = &file.shortcuts {
        shortcuts::check(s).map_err(|e| format!("shortcuts: {e}"))?;
    }
    if file.policy == Some(kule_engine::PolicyChoice::Own) {
        return Err("policy: a profile carries only Kule's balanced or careful policy".to_owned());
    }
    if let Some(Budget { daily: Some(m) }) = &file.budget {
        let currency_ok =
            m.currency.len() == 3 && m.currency.chars().all(|c| c.is_ascii_uppercase());
        if !(m.amount.is_finite() && m.amount > 0.0) || !currency_ok {
            return Err(format!(
                "budget: {} {} is not a budget",
                m.amount, m.currency
            ));
        }
    }
    if let Some(limits) = &file.limits {
        limits.check().map_err(|e| format!("limits: {e}"))?;
    }
    if let Some(agents) = &file.agents
        && agents.len() > MAX_AGENTS
    {
        return Err(format!("agents: at most {MAX_AGENTS}"));
    }
    Ok(ProfileFile { appearance, ..file })
}

/// Applies `parts` of the profile in `text` to `home`; says what each did.
/// Themes join the person's own; shortcuts, the policy and the budget take
/// the profile's; an agent is saved as written and waits for approval.
pub fn apply(home: &Path, text: &str, parts: &[Part]) -> Result<Vec<String>> {
    let file = read(text)?;
    let mut said = Vec::new();
    for part in parts {
        match part {
            Part::Appearance => {
                let Some(theirs) = &file.appearance else {
                    continue;
                };
                let mut mine = themes::load(home)?;
                let mut current = theirs.current.clone();
                for theme in &theirs.custom {
                    let mut theme = theme.clone();
                    if let Some(same) = mine.custom.iter().find(|t| t.id == theme.id) {
                        if *same == theme {
                            continue;
                        }
                        // Another theme under the same id: this one gets its own.
                        let id = format!(
                            "{}-{:x}",
                            theme.id,
                            std::process::id() ^ mine.custom.len() as u32
                        );
                        if current == theme.id {
                            current = id.clone();
                        }
                        theme.id = id;
                    }
                    mine.custom.push(theme);
                }
                mine.current = current;
                mine.motion = theirs.motion;
                let kept = themes::save(home, &mine)?;
                said.push(format!(
                    "Appearance: {} theme(s) of yours now, {} in use",
                    kept.custom.len(),
                    kept.current
                ));
            }
            Part::Shortcuts => {
                let Some(s) = &file.shortcuts else { continue };
                let kept = shortcuts::from_profile(s, &shortcuts::load(home)?);
                shortcuts::save(home, &kept)?;
                said.push(format!("Shortcuts: {} of your own", kept.len()));
                if s.keys()
                    .any(|id| shortcuts::DECIDING.contains(&id.as_str()))
                {
                    said.push(
                        "Shortcuts: the keys that approve, commit or change the policy stay as you set them".to_owned(),
                    );
                }
            }
            Part::Policy => {
                let Some(choice) = file.policy else { continue };
                kule_engine::choose_policy(home, choice).map_err(|e| format!("policy: {e}"))?;
                said.push(format!(
                    "Policy: {}",
                    if choice == kule_engine::PolicyChoice::Careful {
                        "careful"
                    } else {
                        "balanced"
                    }
                ));
            }
            Part::Budget => {
                let Some(b) = &file.budget else { continue };
                kule_engine::set_daily_budget(home, b.daily.clone())
                    .map_err(|e| format!("budget: {e}"))?;
                said.push(match &b.daily {
                    Some(m) => format!("Daily budget: {:.2} {}", m.amount, m.currency),
                    None => "Daily budget: none".to_owned(),
                });
            }
            Part::Limits => {
                let Some(limits) = &file.limits else { continue };
                kule_engine::set_limits(home, limits).map_err(|e| format!("limits: {e}"))?;
                said.push("Limits: every task's, from the profile".to_owned());
            }
            Part::Agents => {
                let Some(agents) = &file.agents else { continue };
                for agent in agents {
                    match kule_acp::Agents::save(home, agent) {
                        Ok(()) => {
                            let held = kule_acp::Agents::load(home)
                                .ok()
                                .and_then(|a| {
                                    a.listed()
                                        .iter()
                                        .find(|l| l.profile.name == agent.name)
                                        .map(|l| l.held.is_some())
                                })
                                .unwrap_or(true);
                            said.push(if held {
                                format!("Agent {}: saved; it runs once you approve it", agent.name)
                            } else {
                                format!("Agent {}: saved, as you approved it before", agent.name)
                            });
                        }
                        Err(e) => said.push(format!("Agent {}: not saved: {e}", agent.name)),
                    }
                }
            }
        }
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENCODE: &str = r#"
[[agent]]
name = "opencode"
command = ["npx", "-y", "opencode-ai@1.2.3", "acp"]
state = [".local/share/opencode"]
login = [".local/share/opencode/auth.json"]
"#;

    fn chosen(home: &Path) {
        let mut s = shortcuts::Shortcuts::new();
        s.insert("task.new".into(), "Mod+Shift+N".into());
        shortcuts::save(home, &s).unwrap();
        themes::save(
            home,
            &themes::Themes {
                current: "custom-a".into(),
                custom: vec![themes::Theme {
                    id: "custom-a".into(),
                    name: "Harbour".into(),
                    scheme: themes::Scheme::Dark,
                    colors: [("bg".to_owned(), "#0b1a2a".to_owned())].into(),
                }],
                motion: themes::Motion::Off,
            },
        )
        .unwrap();
        kule_engine::choose_policy(home, kule_engine::PolicyChoice::Careful).unwrap();
        kule_engine::set_daily_budget(
            home,
            Some(kule_engine::Money {
                amount: 20.0,
                currency: "USD".into(),
            }),
        )
        .unwrap();
        kule_engine::set_limits(
            home,
            &kule_engine::Limits {
                memory_mb: Some(4096),
                gpu: false,
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(home.join(kule_acp::AGENTS_FILE), OPENCODE).unwrap();
    }

    #[test]
    fn a_profile_carries_the_parts_asked_for_to_another_home() {
        let (one, two) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        chosen(one.path());
        let all = [
            Part::Appearance,
            Part::Shortcuts,
            Part::Policy,
            Part::Budget,
            Part::Limits,
            Part::Agents,
        ];
        let text = export(one.path(), &all).unwrap();
        let file = read(&text).unwrap();
        assert_eq!(file.agents.as_ref().unwrap()[0].name, "opencode");

        // Only what is asked for is applied.
        apply(two.path(), &text, &[Part::Shortcuts, Part::Budget]).unwrap();
        assert_eq!(
            shortcuts::load(two.path()).unwrap()["task.new"],
            "Mod+Shift+N"
        );
        assert_eq!(
            kule_engine::daily_budget(two.path())
                .unwrap()
                .unwrap()
                .amount,
            20.0
        );
        assert_eq!(themes::load(two.path()).unwrap(), themes::Themes::default());
        assert_eq!(
            kule_engine::policy_choice(two.path()).unwrap(),
            kule_engine::PolicyChoice::Balanced
        );

        assert_eq!(
            kule_engine::limits(two.path()).unwrap(),
            kule_engine::Limits::default()
        );

        let said = apply(two.path(), &text, &all).unwrap();
        let limits = kule_engine::limits(two.path()).unwrap();
        assert_eq!((limits.memory_mb, limits.gpu), (Some(4096), false));
        assert_eq!(themes::load(two.path()).unwrap().current, "custom-a");
        assert_eq!(
            themes::load(two.path()).unwrap().motion,
            themes::Motion::Off
        );
        assert_eq!(
            kule_engine::policy_choice(two.path()).unwrap(),
            kule_engine::PolicyChoice::Careful
        );
        // The agent arrives, and waits to be approved here.
        let agents = kule_acp::Agents::load(two.path()).unwrap();
        let opencode = agents
            .listed()
            .iter()
            .find(|l| l.profile.name == "opencode")
            .unwrap();
        assert!(opencode.held.is_some(), "{said:?}");
        assert!(
            said.iter().any(|s| s.contains("once you approve")),
            "{said:?}"
        );

        // A part asked for but left out of the file changes nothing.
        let only_budget = export(one.path(), &[Part::Budget]).unwrap();
        apply(two.path(), &only_budget, &[Part::Shortcuts]).unwrap();
        assert_eq!(
            shortcuts::load(two.path()).unwrap()["task.new"],
            "Mod+Shift+N"
        );
    }

    #[test]
    fn a_profile_that_would_let_things_through_or_carries_more_is_refused() {
        let home = tempfile::TempDir::new().unwrap();
        // A policy of the person's own does not travel.
        std::fs::write(home.path().join(kule_engine::USER_POLICY), "// mine\n").unwrap();
        assert!(
            export(home.path(), &[Part::Policy])
                .unwrap_err()
                .contains("leave Policy out")
        );
        assert!(export(home.path(), &[Part::Shortcuts]).is_ok());

        for bad in [
            r#"{"kule_profile":1,"policy":"own"}"#,
            r#"{"kule_profile":1,"remembered":[{"host":"evil.example"}]}"#,
            r#"{"kule_profile":1,"secrets":{"KEY":"x"}}"#,
            r#"{"kule_profile":2}"#,
            r##"{"kule_profile":1,"appearance":{"current":"x","custom":[{"id":"x","name":"n","scheme":"dark","colors":{"bg":"url(x)"}}]}}"##,
            r#"{"kule_profile":1,"shortcuts":{"Bad Id":"Mod+K"}}"#,
            r#"{"kule_profile":1,"budget":{"daily":{"amount":-1,"currency":"USD"}}}"#,
            r#"{"kule_profile":1,"limits":{"idle_minutes":0}}"#,
        ] {
            assert!(read(bad).is_err(), "{bad}");
        }
        assert!(read(&" ".repeat(MAX_FILE as usize + 1)).is_err());
    }

    #[test]
    fn a_profile_never_moves_the_keys_that_decide() {
        let home = tempfile::TempDir::new().unwrap();
        let mut mine = shortcuts::Shortcuts::new();
        mine.insert("question.deny".into(), "Mod+Alt+N".into());
        mine.insert("task.new".into(), "Mod+Shift+N".into());
        shortcuts::save(home.path(), &mine).unwrap();

        // Send the prompt with nothing, and approve-and-remember with the
        // keys that used to send it.
        let text = r#"{"kule_profile":1,"shortcuts":{
            "prompt.run":"",
            "question.remember":"Mod+Enter",
            "question.approve":"Mod+Shift+K",
            "review.commit":"Mod+L",
            "policy.balanced":"Mod+1",
            "question.deny":"",
            "view.live":"F5"}}"#;
        let said = apply(home.path(), text, &[Part::Shortcuts]).unwrap();
        let kept = shortcuts::load(home.path()).unwrap();
        for id in shortcuts::DECIDING {
            assert_eq!(kept.get(*id), mine.get(*id), "{id}");
        }
        // The rest is the profile's.
        assert_eq!(kept["prompt.run"], "");
        assert_eq!(kept["view.live"], "F5");
        assert!(!kept.contains_key("task.new"));
        assert!(
            said.iter().any(|s| s.contains("stay as you set them")),
            "{said:?}"
        );
    }
}
