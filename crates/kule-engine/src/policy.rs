//! Policy: which effects pass on their own, which wait for a person, and
//! which never pass.
//!
//! Policies are Cedar. Cedar only answers allow or deny, so each effect is
//! asked twice: may it pass on its own (`Kule::Action::"auto"`), and if not,
//! may it be put to a person (`Kule::Action::"ask"`). The first yes decides;
//! two noes deny. A transaction gets the strictest verdict of its effects.
//! Evaluation errors deny.
//!
//! The built-in policy below is the default. A user policy replaces it (a
//! `forbid` can never be overridden, so replacing is the only way to loosen
//! it). A repository policy is read from the real workspace, never from the
//! copy an agent can edit, and may only add `forbid` rules.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;

use cedar_policy::{
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid,
    PolicySet, Request, RestrictedExpression,
};
use serde::{Deserialize, Serialize};

use crate::effects::{Change, EffectSet};
use crate::error::{Error, Result};
use crate::git::RefChange;

pub const DEFAULT_POLICY: &str = include_str!("default_policy.cedar");

/// Where a repository keeps its own rules, relative to the workspace.
pub const REPO_POLICY: &str = ".kule/policy.cedar";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Passes on its own.
    Allow,
    /// Waits for a person.
    Ask,
    /// Never passes.
    Deny,
}

/// The verdict on one thing the transaction does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judgement {
    /// A workspace path, or for `.git`, what changed there (`git ref refs/heads/main`).
    pub subject: String,
    pub verdict: Verdict,
    /// The `@id` of the rules behind the verdict.
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ruling {
    pub verdict: Verdict,
    pub judgements: Vec<Judgement>,
}

impl Ruling {
    /// The judgements that keep the transaction from passing on its own.
    pub fn holding(&self) -> impl Iterator<Item = &Judgement> {
        self.judgements
            .iter()
            .filter(|j| j.verdict != Verdict::Allow)
    }
}

pub struct Policy {
    set: PolicySet,
}

impl Policy {
    /// The built-in default alone.
    pub fn builtin() -> Policy {
        Policy::from_texts(DEFAULT_POLICY, None).expect("the built-in policy parses")
    }

    /// The user's policy (or the built-in one when `user` is absent) plus
    /// the repository's `forbid` rules from `workspace`, when it has any.
    pub fn load(user: Option<&Path>, workspace: &Path) -> Result<Policy> {
        let base = match user.filter(|p| p.is_file()) {
            Some(path) => read(path)?,
            None => DEFAULT_POLICY.to_owned(),
        };
        let repo_path = workspace.join(REPO_POLICY);
        let repo = if repo_path.is_file() {
            Some(read(&repo_path)?)
        } else {
            None
        };
        Policy::from_texts(&base, repo.as_deref())
    }

    /// `base` with `repo`'s rules added; `repo` may hold only `forbid` rules.
    pub fn from_texts(base: &str, repo: Option<&str>) -> Result<Policy> {
        let invalid =
            |what: &str, e: &dyn std::fmt::Display| Error::Invalid(format!("{what}: {e}"));
        PolicySet::from_str(base).map_err(|e| invalid("policy", &e))?;
        let mut text = base.to_owned();
        if let Some(repo) = repo {
            let repo_set = PolicySet::from_str(repo).map_err(|e| invalid(REPO_POLICY, &e))?;
            if repo_set.templates().next().is_some()
                || repo_set
                    .policies()
                    .any(|p| p.effect() != cedar_policy::Effect::Forbid)
            {
                return Err(Error::Invalid(format!(
                    "{REPO_POLICY} may only contain forbid rules: a repository can tighten the policy, never loosen it"
                )));
            }
            text.push('\n');
            text.push_str(repo);
        }
        let set = PolicySet::from_str(&text).map_err(|e| invalid("policy", &e))?;
        Ok(Policy { set })
    }

    /// A check for [`Task::commit_if`](crate::Task::commit_if): passes what
    /// the policy allows, and what it would ask about when a person
    /// `approved` it; holds everything else with [`Error::Held`].
    pub fn gate(&self, approved: bool) -> impl FnOnce(&EffectSet) -> Result<()> + '_ {
        move |set| {
            let ruling = self.judge(set);
            match ruling.verdict {
                Verdict::Allow => Ok(()),
                Verdict::Ask if approved => Ok(()),
                _ => Err(Error::Held(ruling)),
            }
        }
    }

    /// Judges everything the effect set does.
    pub fn judge(&self, set: &EffectSet) -> Ruling {
        let deletes = set.files().filter(|e| e.change == Change::Deleted).count();
        let files = set.files().count();
        let mut judgements = Vec::new();
        for effect in set.files() {
            let kind = match effect.change {
                Change::Created => "fs.create",
                Change::Modified => "fs.modify",
                Change::Deleted => "fs.delete",
            };
            let subject = Subject::file(kind, &effect.path, effect.change);
            judgements.push(self.judge_one(effect.path.clone(), &subject, deletes, files));
        }
        if let Some(git) = &set.git {
            for change in &git.refs {
                let subject = Subject::git_ref(change);
                judgements.push(self.judge_one(
                    format!("git ref {}", change.name),
                    &subject,
                    deletes,
                    files,
                ));
            }
            if git.config {
                let subject = Subject::git("git.config", "config");
                judgements.push(self.judge_one("git config".into(), &subject, deletes, files));
            }
            for hook in &git.hooks {
                let subject = Subject::git("git.hook", hook);
                judgements.push(self.judge_one(
                    format!("git hook {hook}"),
                    &subject,
                    deletes,
                    files,
                ));
            }
            for path in &git.other {
                let subject = Subject::git("git.other", path);
                judgements.push(self.judge_one(path.clone(), &subject, deletes, files));
            }
        }
        let verdict = judgements
            .iter()
            .map(|j| j.verdict)
            .max()
            .unwrap_or(Verdict::Allow);
        Ruling {
            verdict,
            judgements,
        }
    }

    fn judge_one(
        &self,
        label: String,
        subject: &Subject,
        deletes: usize,
        files: usize,
    ) -> Judgement {
        let judgement = |verdict, rules| Judgement {
            subject: label.clone(),
            verdict,
            rules,
        };
        let auto = match self.ask(subject, "auto", deletes, files) {
            Ok(answer) => answer,
            Err(error) => return judgement(Verdict::Deny, vec![format!("policy-error: {error}")]),
        };
        if auto.allowed {
            return judgement(Verdict::Allow, auto.rules);
        }
        match self.ask(subject, "ask", deletes, files) {
            Err(error) => judgement(Verdict::Deny, vec![format!("policy-error: {error}")]),
            // Why it does not pass on its own: the forbids that matched, or
            // the lack of any rule letting it through.
            Ok(ask) if ask.allowed => judgement(
                Verdict::Ask,
                if auto.rules.is_empty() {
                    vec!["no-rule-lets-it-pass".into()]
                } else {
                    auto.rules
                },
            ),
            Ok(ask) => judgement(
                Verdict::Deny,
                if ask.rules.is_empty() {
                    vec!["no-rule-allows-asking".into()]
                } else {
                    ask.rules
                },
            ),
        }
    }

    fn ask(
        &self,
        subject: &Subject,
        action: &str,
        deletes: usize,
        files: usize,
    ) -> std::result::Result<Answer, String> {
        let resource = uid("Kule::Effect", "effect");
        let entity = Entity::new(resource.clone(), subject.attrs(), HashSet::new())
            .map_err(|e| e.to_string())?;
        let entities = Entities::from_entities([entity], None).map_err(|e| e.to_string())?;
        let context = Context::from_pairs([
            (
                "deletes".to_owned(),
                RestrictedExpression::new_long(deletes as i64),
            ),
            (
                "files".to_owned(),
                RestrictedExpression::new_long(files as i64),
            ),
        ])
        .map_err(|e| e.to_string())?;
        let request = Request::new(
            uid("Kule::Agent", "agent"),
            uid("Kule::Action", action),
            resource,
            context,
            None,
        )
        .map_err(|e| e.to_string())?;
        let response = Authorizer::new().is_authorized(&request, &self.set, &entities);
        if let Some(error) = response.diagnostics().errors().next() {
            return Err(error.to_string());
        }
        let mut rules: Vec<String> = response
            .diagnostics()
            .reason()
            .map(|id| {
                self.set
                    .annotation(id, "id")
                    .map_or_else(|| id.to_string(), str::to_owned)
            })
            .collect();
        rules.sort();
        Ok(Answer {
            allowed: response.decision() == Decision::Allow,
            rules,
        })
    }
}

struct Answer {
    allowed: bool,
    /// The rules that decided: the permits on allow, the forbids on deny.
    rules: Vec<String>,
}

/// What a policy sees of one effect, as attributes of `Kule::Effect`. Every
/// attribute is always present, so rules can test any of them.
struct Subject {
    kind: &'static str,
    path: String,
    change: &'static str,
    ref_name: String,
    fast_forward: bool,
    commits: i64,
}

impl Subject {
    fn file(kind: &'static str, path: &str, change: Change) -> Subject {
        Subject {
            kind,
            path: path.to_owned(),
            change: change_name(change),
            ref_name: String::new(),
            fast_forward: false,
            commits: 0,
        }
    }

    fn git(kind: &'static str, path: &str) -> Subject {
        Subject {
            kind,
            path: path.to_owned(),
            change: "modified",
            ref_name: String::new(),
            fast_forward: false,
            commits: 0,
        }
    }

    fn git_ref(change: &RefChange) -> Subject {
        Subject {
            kind: "git.ref",
            path: String::new(),
            change: match (&change.old, &change.new) {
                (None, _) => "created",
                (_, None) => "deleted",
                _ => "modified",
            },
            ref_name: change.name.clone(),
            fast_forward: change.fast_forward == Some(true),
            commits: change.commits.map_or(0, |n| n as i64),
        }
    }

    fn attrs(&self) -> HashMap<String, RestrictedExpression> {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        HashMap::from([
            (
                "kind".into(),
                RestrictedExpression::new_string(self.kind.into()),
            ),
            (
                "path".into(),
                RestrictedExpression::new_string(self.path.clone()),
            ),
            ("name".into(), RestrictedExpression::new_string(name.into())),
            (
                "change".into(),
                RestrictedExpression::new_string(self.change.into()),
            ),
            (
                "ref".into(),
                RestrictedExpression::new_string(self.ref_name.clone()),
            ),
            (
                "fast_forward".into(),
                RestrictedExpression::new_bool(self.fast_forward),
            ),
            (
                "commits".into(),
                RestrictedExpression::new_long(self.commits),
            ),
        ])
    }
}

fn change_name(change: Change) -> &'static str {
    match change {
        Change::Created => "created",
        Change::Modified => "modified",
        Change::Deleted => "deleted",
    }
}

fn uid(kind: &str, id: &str) -> EntityUid {
    EntityUid::from_type_name_and_id(
        EntityTypeName::from_str(kind).expect("a valid type name"),
        EntityId::new(id),
    )
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}
