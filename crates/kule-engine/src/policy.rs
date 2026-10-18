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
    Authorizer, Context, Decision, Entities, Entity, EntityId, EntityTypeName, EntityUid, PolicyId,
    PolicySet, Request, RestrictedExpression,
};
use serde::{Deserialize, Serialize};

use crate::effects::{Change, EffectSet};
use crate::error::{Error, Result};
use crate::git::RefChange;
use crate::remembered::{Allowance, Remembered};

pub const DEFAULT_POLICY: &str = include_str!("default_policy.cedar");

/// The built-in policy with every change to a file or to git waiting for a
/// person.
pub const CAREFUL_POLICY: &str = concat!(
    include_str!("default_policy.cedar"),
    include_str!("careful_policy.cedar")
);

/// Where a repository keeps its own rules, relative to the workspace.
pub const REPO_POLICY: &str = ".kule/policy.cedar";

/// Where a person's policy lives, under Kule's home; the built-in one
/// holds while there is none.
pub const USER_POLICY: &str = "policy.cedar";

/// Which policy holds: one of Kule's two, or one the person wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyChoice {
    /// The built-in policy: changes to files pass, what decides what gets
    /// installed, built or run waits.
    Balanced,
    /// [`CAREFUL_POLICY`].
    Careful,
    /// `<home>/policy.cedar` as the person wrote it.
    Own,
}

/// Which policy holds under `home`.
pub fn policy_choice(home: &Path) -> Result<PolicyChoice> {
    let path = home.join(USER_POLICY);
    if !path.is_file() {
        return Ok(PolicyChoice::Balanced);
    }
    Ok(match read(&path)?.as_str() {
        DEFAULT_POLICY => PolicyChoice::Balanced,
        CAREFUL_POLICY => PolicyChoice::Careful,
        _ => PolicyChoice::Own,
    })
}

/// Puts one of Kule's policies in place. A policy the person wrote is
/// never replaced: they move it away first.
pub fn choose_policy(home: &Path, choice: PolicyChoice) -> Result<()> {
    let path = home.join(USER_POLICY);
    if policy_choice(home)? == PolicyChoice::Own {
        return Err(Error::Invalid(format!(
            "{} is a policy of your own; Kule does not replace it",
            path.display()
        )));
    }
    match choice {
        PolicyChoice::Balanced => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(Error::Io { path, source: e })
            }
            _ => Ok(()),
        },
        PolicyChoice::Careful => {
            std::fs::create_dir_all(home).map_err(|source| Error::Io {
                path: home.to_path_buf(),
                source,
            })?;
            std::fs::write(&path, CAREFUL_POLICY).map_err(|source| Error::Io { path, source })
        }
        PolicyChoice::Own => Err(Error::Invalid(format!(
            "a policy of your own is written to {}",
            path.display()
        ))),
    }
}

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
    /// Files a person said not to be asked about again, with the rule that
    /// asked: (rule, path).
    files: Vec<(String, String)>,
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
        Policy::assemble(&base, workspace, user.and_then(Path::parent))
    }

    /// The policy `task` is judged by: its group's, when the group chose
    /// one, else the one for every task (`<home>/policy.cedar` or the
    /// built-in one); then the workspace's forbids and what the person
    /// remembered, as [`Policy::load`] adds them.
    pub fn for_task(task: &crate::Task) -> Result<Policy> {
        let home = task.home();
        let base = match crate::groups::base_policy(&home, task)? {
            Some(text) => text,
            None => {
                let user = home.join(USER_POLICY);
                if user.is_file() {
                    read(&user)?
                } else {
                    DEFAULT_POLICY.to_owned()
                }
            }
        };
        Policy::assemble(&base, task.workspace(), Some(&home))
    }

    /// `base`, the forbids of the repository at `workspace`, and what was
    /// remembered in `home`.
    fn assemble(base: &str, workspace: &Path, home: Option<&Path>) -> Result<Policy> {
        let repo_path = workspace.join(REPO_POLICY);
        let repo = if repo_path.is_file() {
            Some(read(&repo_path)?)
        } else {
            None
        };
        let policy = Policy::from_texts(base, repo.as_deref())?;
        // What the person remembered lives next to their policy.
        match home {
            Some(home) => policy.remembering(&crate::remembered::load(home)?, workspace),
            None => Ok(policy),
        }
    }

    /// The policy with what a person remembered for `workspace` (see
    /// [`crate::remembered`]): a connection as a `permit`, a file as an
    /// exception to the rules that ask about it.
    pub fn remembering(mut self, remembered: &[Remembered], workspace: &Path) -> Result<Policy> {
        let invalid = |e: &dyn std::fmt::Display| Error::Invalid(format!("remembered: {e}"));
        // Kept with the folder's real path.
        let workspace = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_owned());
        for kept in remembered.iter().filter(|r| r.holds_for(&workspace)) {
            match &kept.allowance {
                Allowance::Connect { host, port } => {
                    // Checked when it was kept: a plain host name.
                    if !host
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                    {
                        return Err(invalid(&host));
                    }
                    let text = format!(
                        "@id(\"remembered-{}\") permit (principal, action == Kule::Action::\"auto\", resource) \
                         when {{ resource.kind == \"net.connect\" && resource.host == \"{host}\" && resource.port == {port} }};",
                        kept.id
                    );
                    let policy = cedar_policy::Policy::parse(
                        Some(PolicyId::new(format!("remembered-{}", kept.id))),
                        text,
                    )
                    .map_err(|e| invalid(&e))?;
                    self.set.add(policy).map_err(|e| invalid(&e))?;
                }
                Allowance::File { path, rule } => self.files.push((rule.clone(), path.clone())),
            }
        }
        Ok(self)
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
        Ok(Policy {
            set,
            files: Vec::new(),
        })
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

    /// Lets HTTPS connections to `hosts` pass on their own, under the rule
    /// `id`. Agent profiles use it for the endpoints an agent needs to log in
    /// and reach its model. A `forbid` (from a repository, say) still wins.
    pub fn permit_hosts(mut self, id: &str, hosts: &[&str]) -> Result<Policy> {
        let is_name = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        };
        if !is_name(id) || !hosts.iter().all(|h| is_name(h)) {
            return Err(Error::Invalid(format!("bad host rule {id}: {hosts:?}")));
        }
        let list = hosts
            .iter()
            .map(|h| format!("\"{h}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let text = format!(
            "@id(\"{id}\") permit (principal, action == Kule::Action::\"auto\", resource) \
             when {{ resource.kind == \"net.connect\" && resource.port == 443 && [{list}].contains(resource.host) }};"
        );
        let invalid = |e: &dyn std::fmt::Display| Error::Invalid(format!("policy: {e}"));
        let policy = cedar_policy::Policy::parse(Some(PolicyId::new(format!("kule-{id}"))), text)
            .map_err(|e| invalid(&e))?;
        self.set.add(policy).map_err(|e| invalid(&e))?;
        Ok(self)
    }

    /// Judges a network connection a command asks the proxy for. It happens
    /// now, so nobody can be asked yet: the caller lets through only
    /// [`Verdict::Allow`].
    pub fn judge_connection(&self, host: &str, port: u16) -> Judgement {
        let subject = Subject::connection(host, port);
        self.judge_one(format!("{host}:{port}"), &subject, &Facts::default())
    }

    /// Judges everything the effect set does.
    pub fn judge(&self, set: &EffectSet) -> Ruling {
        // A move is not a deletion: the file lives on under its new name.
        let deletes = set.deleted().count();
        let files = set.files().count();
        let facts = Facts {
            deletes,
            files,
            repository: set.repository,
        };
        let mut judgements = Vec::new();
        for effect in set.files() {
            let kind = match effect.change {
                Change::Created => "fs.create",
                Change::Modified => "fs.modify",
                Change::Deleted => "fs.delete",
            };
            let subject = Subject::file(kind, &effect.path, effect.change);
            let mut judgement = self.judge_one(effect.path.clone(), &subject, &facts);
            // Asked about only by rules a person answered for good here.
            let remembered = judgement.verdict == Verdict::Ask
                && !judgement.rules.is_empty()
                && judgement.rules.iter().all(|rule| {
                    self.files
                        .iter()
                        .any(|(r, path)| r == rule && *path == effect.path)
                });
            if remembered {
                judgement.verdict = Verdict::Allow;
                judgement.rules = vec!["remembered".to_owned()];
            }
            judgements.push(judgement);
        }
        if let Some(git) = &set.git {
            for change in &git.refs {
                let subject = Subject::git_ref(change);
                judgements.push(self.judge_one(
                    format!("git ref {}", change.name),
                    &subject,
                    &facts,
                ));
            }
            if git.config {
                let subject = Subject::git("git.config", "config");
                judgements.push(self.judge_one("git config".into(), &subject, &facts));
            }
            for hook in &git.hooks {
                let subject = Subject::git("git.hook", hook);
                judgements.push(self.judge_one(format!("git hook {hook}"), &subject, &facts));
            }
            for path in &git.staged {
                let subject = Subject::git("git.staged", path);
                judgements.push(self.judge_one(format!("git staged {path}"), &subject, &facts));
            }
            for path in &git.other {
                let subject = Subject::git("git.other", path);
                judgements.push(self.judge_one(path.clone(), &subject, &facts));
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

    fn judge_one(&self, label: String, subject: &Subject, facts: &Facts) -> Judgement {
        let judgement = |verdict, rules| Judgement {
            subject: label.clone(),
            verdict,
            rules,
        };
        let auto = match self.ask(subject, "auto", facts) {
            Ok(answer) => answer,
            Err(error) => return judgement(Verdict::Deny, vec![format!("policy-error: {error}")]),
        };
        if auto.allowed {
            return judgement(Verdict::Allow, auto.rules);
        }
        match self.ask(subject, "ask", facts) {
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
        facts: &Facts,
    ) -> std::result::Result<Answer, String> {
        let resource = uid("Kule::Effect", "effect");
        let entity = Entity::new(resource.clone(), subject.attrs(), HashSet::new())
            .map_err(|e| e.to_string())?;
        let entities = Entities::from_entities([entity], None).map_err(|e| e.to_string())?;
        let context = Context::from_pairs([
            (
                "deletes".to_owned(),
                RestrictedExpression::new_long(facts.deletes as i64),
            ),
            (
                "files".to_owned(),
                RestrictedExpression::new_long(facts.files as i64),
            ),
            (
                "repository".to_owned(),
                RestrictedExpression::new_bool(facts.repository),
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

/// What the policy's context says about the whole transaction.
#[derive(Default)]
struct Facts {
    /// Paths deleted for good: moves left out.
    deletes: usize,
    files: usize,
    repository: bool,
}

struct Answer {
    allowed: bool,
    /// The rules that decided: the permits on allow, the forbids on deny.
    rules: Vec<String>,
}

/// What a policy sees of one effect, as attributes of `Kule::Effect`. Every
/// attribute is always present, so rules can test any of them.
#[derive(Default)]
struct Subject {
    kind: &'static str,
    path: String,
    change: &'static str,
    ref_name: String,
    fast_forward: bool,
    commits: i64,
    host: String,
    port: i64,
}

impl Subject {
    fn file(kind: &'static str, path: &str, change: Change) -> Subject {
        Subject {
            kind,
            path: path.to_owned(),
            change: change_name(change),
            ..Subject::default()
        }
    }

    fn git(kind: &'static str, path: &str) -> Subject {
        Subject {
            kind,
            path: path.to_owned(),
            change: "modified",
            ..Subject::default()
        }
    }

    fn git_ref(change: &RefChange) -> Subject {
        Subject {
            kind: "git.ref",
            change: match (&change.old, &change.new) {
                (None, _) => "created",
                (_, None) => "deleted",
                _ => "modified",
            },
            ref_name: change.name.clone(),
            fast_forward: change.fast_forward == Some(true),
            commits: change.commits.map_or(0, |n| n as i64),
            ..Subject::default()
        }
    }

    fn connection(host: &str, port: u16) -> Subject {
        Subject {
            kind: "net.connect",
            change: "created",
            host: host.to_owned(),
            port: i64::from(port),
            ..Subject::default()
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
            (
                "host".into(),
                RestrictedExpression::new_string(self.host.clone()),
            ),
            ("port".into(), RestrictedExpression::new_long(self.port)),
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
