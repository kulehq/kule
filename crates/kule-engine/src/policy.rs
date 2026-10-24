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
use crate::git::{Carried, CarriedEntry, RefChange};
use crate::manifest::Kind;
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

/// Where the choice of Kule's careful policy is kept, under Kule's home: a
/// choice, never the text, so it always means this version's rules.
const POLICY_CHOICE: &str = "policy-choice";

/// The blake3 of every balanced and careful text earlier versions wrote to
/// [`USER_POLICY`] or a person copied there, from 0.2.0-preview.1 to
/// 0.2.0-preview.11: such a file is that choice, judged by today's rules,
/// never a policy of the person's own. Choices are no longer kept as text,
/// so the list does not grow.
const EARLIER_TEXTS: [(&str, PolicyChoice); 6] = [
    (
        "09832ff8257708d5147645cd0ed832e35744f7db9c5274037261e7744db295ef",
        PolicyChoice::Balanced,
    ),
    (
        "54b7d8bcaf14f8315892266a2ab77c9f947da7933e41938fb3595a1d70c2e431",
        PolicyChoice::Careful,
    ),
    (
        "7364d81e982d84ee86f65ca4ca9f41d43123376e744d056b0220a4e26ee5f94f",
        PolicyChoice::Balanced,
    ),
    (
        "df0c87af894168f287f7e150185d595d2e06196cae14ba941dd8c538230b5907",
        PolicyChoice::Careful,
    ),
    (
        "b38b5e97d82ed1e7f76cb76c025b40248cc7df3ba4947003368fd435549fc2bb",
        PolicyChoice::Balanced,
    ),
    (
        "14757aef6b0547e99cfd4d89c0c4aab237a0b5ba766e0eb8b8563d070ee4bccd",
        PolicyChoice::Careful,
    ),
];

/// Which policy holds: one of Kule's two, or one the person wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyChoice {
    /// The built-in policy: source, docs and data, and kinds of file the
    /// project already tracks, pass; other kinds, hidden paths, and what
    /// decides what gets installed, built or run wait.
    Balanced,
    /// [`CAREFUL_POLICY`].
    Careful,
    /// `<home>/policy.cedar` as the person wrote it.
    Own,
}

/// Which policy holds under `home`. A [`USER_POLICY`] holding one of Kule's
/// texts, today's or an earlier version's, is that choice.
pub fn policy_choice(home: &Path) -> Result<PolicyChoice> {
    let path = home.join(USER_POLICY);
    if path.is_file() {
        let text = read(&path)?;
        return Ok(kules_text(&text).unwrap_or(PolicyChoice::Own));
    }
    let marker = home.join(POLICY_CHOICE);
    Ok(match std::fs::read_to_string(&marker) {
        Ok(choice) if choice.trim() == "careful" => PolicyChoice::Careful,
        Ok(choice) => {
            return Err(Error::Invalid(format!(
                "{}: {:?} is not a policy Kule knows; choose balanced or careful again",
                marker.display(),
                choice.trim()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PolicyChoice::Balanced,
        Err(source) => {
            return Err(Error::Io {
                path: marker,
                source,
            });
        }
    })
}

/// Which of Kule's policies `text` is, today's or an earlier version's.
fn kules_text(text: &str) -> Option<PolicyChoice> {
    match text {
        DEFAULT_POLICY => Some(PolicyChoice::Balanced),
        CAREFUL_POLICY => Some(PolicyChoice::Careful),
        _ => {
            let hash = blake3::hash(text.as_bytes()).to_hex();
            EARLIER_TEXTS
                .iter()
                .find(|(earlier, _)| *earlier == hash.as_str())
                .map(|(_, choice)| *choice)
        }
    }
}

/// The text every task is judged by under `home` unless its group chose
/// another: Kule's balanced or careful policy as this version has it, or
/// the person's own.
pub fn policy_text(home: &Path) -> Result<String> {
    Ok(match policy_choice(home)? {
        PolicyChoice::Balanced => DEFAULT_POLICY.to_owned(),
        PolicyChoice::Careful => CAREFUL_POLICY.to_owned(),
        PolicyChoice::Own => read(&home.join(USER_POLICY))?,
    })
}

/// Puts one of Kule's policies in place. A policy the person wrote is
/// never replaced: they move it away first.
pub fn choose_policy(home: &Path, choice: PolicyChoice) -> Result<()> {
    let path = home.join(USER_POLICY);
    // Only the person's own text is kept from being replaced: a choice
    // Kule cannot read is put right by choosing again.
    if path.is_file() && kules_text(&read(&path)?).is_none() {
        return Err(Error::Invalid(format!(
            "{} is a policy of your own; Kule does not replace it",
            path.display()
        )));
    }
    let marker = home.join(POLICY_CHOICE);
    let remove = |path: &Path| match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Io {
            path: path.to_path_buf(),
            source: e,
        }),
        _ => Ok(()),
    };
    match choice {
        PolicyChoice::Balanced => {
            remove(&path)?;
            remove(&marker)
        }
        PolicyChoice::Careful => {
            std::fs::create_dir_all(home).map_err(|source| Error::Io {
                path: home.to_path_buf(),
                source,
            })?;
            std::fs::write(&marker, "careful\n").map_err(|source| Error::Io {
                path: marker.clone(),
                source,
            })?;
            // An earlier version's text goes: the choice says it now.
            remove(&path)
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

    /// [`Ruling::holding`] put together by why: the same rules, and for
    /// what waits only for its kind, the same kind. Groups come in the
    /// order their first subject does.
    pub fn held(&self) -> Vec<Held> {
        let mut groups: Vec<Held> = Vec::new();
        for judgement in self.holding() {
            let kind = (judgement.rules == [crate::remembered::UNFAMILIAR_KINDS])
                .then(|| file_kind(&judgement.subject));
            match groups.iter_mut().find(|g| {
                g.rules == judgement.rules && g.kind == kind && g.verdict == judgement.verdict
            }) {
                Some(group) => group.subjects.push(judgement.subject.clone()),
                None => groups.push(Held {
                    verdict: judgement.verdict,
                    reasons: judgement.rules.iter().map(|r| reason(r)).collect(),
                    rules: judgement.rules.clone(),
                    kind,
                    subjects: vec![judgement.subject.clone()],
                }),
            }
        }
        groups
    }
}

/// Subjects that wait, or are denied, for the same reasons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub verdict: Verdict,
    /// The `@id`s of the rules that hold them.
    pub rules: Vec<String>,
    /// Why, in a person's words: one for each of `rules` ([`reason`]).
    pub reasons: Vec<String>,
    /// What kind of file they all are ([`file_kind`]), when only their
    /// kind holds them, and a person may answer the kind for good.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub subjects: Vec<String>,
}

/// Why rule `rule` holds what it holds, in a person's words.
pub fn reason(rule: &str) -> String {
    if rule.starts_with("repo:") {
        return "the repository's own rule".to_owned();
    }
    if rule.starts_with("policy-error") {
        return "the policy could not judge it".to_owned();
    }
    match rule {
        "unfamiliar-kinds" => "a kind of file this project has none of",
        "dot-paths" => "a hidden file or folder, where tools find their settings",
        "automation" => "runs code when a tool or an agent opens the folder",
        "other-vcs" => "another version control system's files, which run on its next command",
        "nested-repository" => "a repository's own files, which git reads and runs",
        "nested-git-hooks" | "git-hooks" => "a git hook, which runs on its own",
        "manifests-and-lockfiles" => "decides what gets installed",
        "project-files" => "decides what gets built",
        "permissions" => "made executable, or writable by anyone",
        "extended-attributes" => "carries attributes no diff shows",
        "links" => "a link, which leads wherever it points",
        "mass-delete" => "one of more than 200 deletions",
        "deletes-outside-git" => "one of more than 20 deletions outside git",
        "careful" => "the careful policy asks about every change",
        "no-rule-lets-it-pass" => "no rule lets it pass on its own",
        _ => "a rule of the policy",
    }
    .to_owned()
}

/// The start of the ids a repository's own rules are added under.
const REPO_RULE: &str = "kule-repository-rule-";

pub struct Policy {
    set: PolicySet,
    /// Files a person said not to be asked about again, with the rule that
    /// asked: (rule, path).
    files: Vec<(String, String)>,
    /// Kinds of file a person said not to be asked about again for their
    /// kind ([`crate::remembered::UNFAMILIAR_KINDS`]).
    kinds: Vec<String>,
}

impl Policy {
    /// The built-in default alone.
    pub fn builtin() -> Policy {
        Policy::from_texts(DEFAULT_POLICY, None).expect("the built-in policy parses")
    }

    /// The policy for every task under `home` ([`policy_text`]; the
    /// built-in one when `home` is absent) plus the repository's `forbid`
    /// rules from `workspace`, when it has any, and what was remembered.
    pub fn load(home: Option<&Path>, workspace: &Path) -> Result<Policy> {
        let base = match home {
            Some(home) => policy_text(home)?,
            None => DEFAULT_POLICY.to_owned(),
        };
        Policy::assemble(&base, workspace, home)
    }

    /// The policy `task` is judged by: its group's, when the group chose
    /// one, else the one for every task (`<home>/policy.cedar` or the
    /// built-in one); then the workspace's forbids and what the person
    /// remembered, as [`Policy::load`] adds them.
    pub fn for_task(task: &crate::Task) -> Result<Policy> {
        Policy::for_task_in(task, task.group()?)
    }

    /// The policy `task` would be judged by in group `group` (`None` for
    /// none), as [`Policy::for_task`] puts it together.
    pub fn for_task_in(task: &crate::Task, group: Option<u64>) -> Result<Policy> {
        let base = Policy::base_text(task, group)?;
        Policy::assemble(&base, task.workspace(), Some(&task.home()))
    }

    /// The rules `task` would be judged by in group `group`, before the
    /// workspace's forbids and what was remembered: two groups with the same
    /// text judge alike.
    pub fn base_text(task: &crate::Task, group: Option<u64>) -> Result<String> {
        let home = task.home();
        match crate::groups::base_policy(&home, group)? {
            Some(text) => Ok(text),
            None => policy_text(&home),
        }
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
                // Kept by an earlier version for what is now always asked.
                Allowance::File { .. } if !kept.applies() => {}
                Allowance::File { path, rule } => self.files.push((rule.clone(), path.clone())),
                Allowance::Kind { kind } => self.kinds.push(kind.clone()),
            }
        }
        Ok(self)
    }

    /// `base` with `repo`'s rules added; `repo` may hold only `forbid` rules,
    /// told as `repo:<@id>`.
    pub fn from_texts(base: &str, repo: Option<&str>) -> Result<Policy> {
        let invalid =
            |what: &str, e: &dyn std::fmt::Display| Error::Invalid(format!("{what}: {e}"));
        let mut set = PolicySet::from_str(base).map_err(|e| invalid("policy", &e))?;
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
            // Under ids of their own: a repository's rule is told as the
            // repository's (`repo:` before its `@id`), and never shares a
            // name with a rule a person may answer for good.
            for (i, policy) in repo_set.policies().enumerate() {
                let policy = policy.new_id(PolicyId::new(format!("{REPO_RULE}{i}")));
                set.add(policy).map_err(|e| invalid(REPO_POLICY, &e))?;
            }
        }
        Ok(Policy {
            set,
            files: Vec::new(),
            kinds: Vec::new(),
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
            let mut subject = Subject::file(kind, &effect.path, effect.change, effect.kind);
            subject.gains = set.gains.get(&effect.path).cloned().unwrap_or_default();
            subject.xattrs_changed = set.xattrs.contains_key(&effect.path);
            subject.kind_tracked = set.tracked_kinds.contains(&subject.file_kind);
            subject.in_git_dir = set.git_dirs.iter().any(|d| {
                effect
                    .path
                    .strip_prefix(d.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            });
            let mut judgement = self.judge_one(effect.path.clone(), &subject, &facts);
            // Asked about only by rules a person answered for good here.
            let remembered = judgement.verdict == Verdict::Ask
                && !judgement.rules.is_empty()
                && judgement.rules.iter().all(|rule| {
                    self.files
                        .iter()
                        .any(|(r, path)| r == rule && *path == effect.path)
                        || (rule == crate::remembered::UNFAMILIAR_KINDS
                            && self.kinds.contains(&subject.file_kind))
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
                // What its new commits carry, judged as files are: a file
                // committed and gone from the folder still goes with it.
                match &change.paths {
                    Some(paths) => {
                        // Each as what the commit did there, and to what; a
                        // record from before they were kept, as files changed.
                        let carried: Vec<Carried> = if change.carried.is_empty() {
                            paths
                                .iter()
                                .map(|path| Carried {
                                    path: path.clone(),
                                    change: Change::Modified,
                                    entry: CarriedEntry::File,
                                })
                                .collect()
                        } else {
                            change.carried.clone()
                        };
                        for c in &carried {
                            let kind = match c.change {
                                Change::Created => "fs.create",
                                Change::Modified => "fs.modify",
                                Change::Deleted => "fs.delete",
                            };
                            let mut subject = match c.entry {
                                CarriedEntry::File => {
                                    Subject::file(kind, &c.path, c.change, Kind::File)
                                }
                                CarriedEntry::Symlink => {
                                    Subject::file(kind, &c.path, c.change, Kind::Symlink)
                                }
                                // What another repository holds: a person's.
                                CarriedEntry::Submodule => Subject::git("git.other", &c.path),
                            };
                            subject.kind_tracked = set.tracked_kinds.contains(&subject.file_kind);
                            judgements.push(self.judge_one(
                                format!("git ref {}: {}", change.name, c.path),
                                &subject,
                                &facts,
                            ));
                        }
                    }
                    None => {
                        let subject = Subject::git("git.other", &change.name);
                        judgements.push(self.judge_one(
                            format!("git ref {}: more commits than Kule reads", change.name),
                            &subject,
                            &facts,
                        ));
                    }
                }
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
                let name = self
                    .set
                    .annotation(id, "id")
                    .map_or_else(|| id.to_string(), str::to_owned);
                if id.to_string().starts_with(REPO_RULE) {
                    format!("repo:{name}")
                } else {
                    name
                }
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
    /// What the path is: "file", "dir" or "symlink"; "" for git and network.
    entry: &'static str,
    path: String,
    /// The path lies in a repository's own files (see
    /// [`EffectSet::git_dirs`]): judged as if under a `.git`.
    in_git_dir: bool,
    change: &'static str,
    ref_name: String,
    fast_forward: bool,
    commits: i64,
    host: String,
    port: i64,
    /// See [`EffectSet::gains`].
    gains: Vec<String>,
    /// See [`EffectSet::xattrs`].
    xattrs_changed: bool,
    /// See [`file_kind`]; "" for git and network.
    file_kind: String,
    /// See [`EffectSet::tracked_kinds`].
    kind_tracked: bool,
}

impl Subject {
    fn file(kind: &'static str, path: &str, change: Change, entry: Kind) -> Subject {
        Subject {
            kind,
            entry: match entry {
                Kind::File => "file",
                Kind::Dir => "dir",
                Kind::Symlink => "symlink",
            },
            path: path.to_owned(),
            change: change_name(change),
            file_kind: file_kind(path),
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
        // Folded as a case-insensitive disk (macOS's, by default) and the
        // tools reading it see the names; each of the path's components,
        // to match a folder at any depth.
        let folded = fold(&self.path);
        let parts = folded
            .split('/')
            .filter(|p| !p.is_empty())
            .chain(self.in_git_dir.then_some(".git"))
            .map(|p| RestrictedExpression::new_string(p.to_owned()));
        HashMap::from([
            (
                "folded_path".into(),
                RestrictedExpression::new_string(folded.clone()),
            ),
            (
                "folded_name".into(),
                RestrictedExpression::new_string(fold(name)),
            ),
            ("parts".into(), RestrictedExpression::new_set(parts)),
            (
                "entry".into(),
                RestrictedExpression::new_string(self.entry.into()),
            ),
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
            (
                "gains".into(),
                RestrictedExpression::new_set(
                    self.gains
                        .iter()
                        .map(|g| RestrictedExpression::new_string(g.clone())),
                ),
            ),
            (
                "xattrs_changed".into(),
                RestrictedExpression::new_bool(self.xattrs_changed),
            ),
            (
                "file_kind".into(),
                RestrictedExpression::new_string(self.file_kind.clone()),
            ),
            (
                "kind_tracked".into(),
                RestrictedExpression::new_bool(self.kind_tracked),
            ),
            (
                "dotted".into(),
                RestrictedExpression::new_bool(
                    !self.entry.is_empty() && (self.in_git_dir || dotted(&self.path)),
                ),
            ),
        ])
    }
}

/// What kind of file `path` is, as the policy's `file_kind` sees it: the
/// extension of its name in lower case (`vue`), or the whole name where it
/// has none (`makefile`, `.envrc`). Not folded: a name that only folds to
/// one with an extension (`x．md`) has none, and is a kind of its own.
pub fn file_kind(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_owned(),
        _ => name,
    }
}

/// Whether a component of `path`, folded, starts with a dot: a hidden
/// file or folder, where tools look for their settings.
pub fn dotted(path: &str) -> bool {
    fold(path).split('/').any(|part| part.starts_with('.'))
}

/// `text` as a disk that ignores case and Unicode forms may read it, and
/// further: compatibility forms taken apart (`ſ` is `s`, `ﬆ` is `st`, the
/// Kelvin sign is `k`), accents dropped, lower case. A name that folds to
/// a listed one is judged as that one; folding too far only asks more.
fn fold(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::is_combining_mark;
    text.nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
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
