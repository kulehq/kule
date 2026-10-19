//! The daemon's wire format: one JSON object per line, each way. A client
//! sends a [`Request`] and reads one [`Response`]; after
//! [`Call::Subscribe`] the connection carries [`Notice`](crate::Notice)s
//! instead, one per line, until either side closes it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Changes whenever a call, a result or a notice changes shape; a client
/// and a daemon that disagree on it cannot talk.
pub const PROTOCOL: u32 = 28;

/// What a daemon says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    /// The daemon's token (see `token_path`); requests without it are
    /// refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(flatten)]
    pub call: Call,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Call {
    /// The daemon's [`Hello`]; every client asks first.
    Hello,
    /// Stops the daemon, unless an agent is at work or a question waits.
    Shutdown,
    Agents,
    Tasks,
    CreateTask {
        workspace: PathBuf,
    },
    Review {
        task: String,
        include_ignored: bool,
    },
    /// What a changed picture was and is (see `Task::preview`).
    Preview {
        task: String,
        path: String,
    },
    Commit {
        task: String,
        approve: bool,
        include_ignored: bool,
        /// The digest of the review the person saw; refused if it changed.
        #[serde(default)]
        reviewed: Option<String>,
        /// With an approval: files whose question is not to be asked again
        /// on this folder, once the commit is in.
        #[serde(default)]
        remember: Vec<String>,
    },
    Rollback {
        task: String,
    },
    Undo {
        task: String,
    },
    /// Puts the copy back as it was after a step (`0`: before the first).
    BackToStep {
        task: String,
        step: u32,
    },
    /// The servers working in the task's copy: processes there that listen
    /// on a TCP port.
    Ports {
        task: String,
    },
    /// A new task on the same folder from this one's copy: now, or as it
    /// was after a step. Returns the new task.
    ForkTask {
        task: String,
        #[serde(default)]
        step: Option<u32>,
    },
    Timeline {
        task: String,
    },
    RemoveTask {
        task: String,
    },
    CopyOf {
        task: String,
    },
    /// The copy, for the person's terminal; refused while the copy's git
    /// config or hooks hold changes (see `Task::terminal_dir`).
    TerminalDir {
        task: String,
    },
    StartRun {
        task: String,
        agent: String,
        prompt: String,
        /// Places in the task's files the prompt is about.
        #[serde(default)]
        refs: Vec<kule_view::refs::Reference>,
    },
    /// The latest events of the task's runs.
    Stream {
        task: String,
    },
    Questions,
    Answer {
        id: u64,
        approve: bool,
        /// With an approval: let this host and port through on the task's
        /// folder from now on.
        #[serde(default)]
        remember: bool,
    },
    SetApprovalTimeout {
        task: String,
        minutes: u64,
    },
    /// What the task's runs may cost together; `None` for no limit.
    SetBudget {
        task: String,
        budget: Option<kule_engine::Money>,
    },
    /// A name for the task, shown with its folder's; `None` or blank clears
    /// it.
    SetLabel {
        task: String,
        label: Option<String>,
    },
    /// `None` for no colour.
    SetColor {
        task: String,
        color: Option<kule_engine::TaskColor>,
    },
    /// The order the task list comes in, as task ids; tasks left out come
    /// first.
    SetOrder {
        tasks: Vec<String>,
    },
    /// The groups tasks are put in, in the person's order.
    Groups,
    /// A new group, last, with the policy for every task.
    CreateGroup {
        name: String,
        color: Option<kule_engine::TaskColor>,
    },
    /// Renames, recolours, folds or unfolds a group.
    UpdateGroup {
        group: u64,
        name: String,
        color: Option<kule_engine::TaskColor>,
        #[serde(default)]
        collapsed: bool,
    },
    /// The policy a group's tasks are judged by; `text` writes the group's
    /// own, which goes with `own`.
    SetGroupPolicy {
        group: u64,
        policy: kule_engine::GroupPolicy,
        #[serde(default)]
        text: Option<String>,
    },
    /// The policy written for a group, if one was.
    GroupPolicyText {
        group: u64,
    },
    /// Every group's id, in the order the list shows them.
    SetGroupOrder {
        groups: Vec<u64>,
    },
    /// Deletes a group; its tasks stay, in no group.
    DeleteGroup {
        group: u64,
    },
    /// What putting a task in a group would change; nothing changes yet.
    GroupImpact {
        task: String,
        group: Option<u64>,
    },
    /// Puts a task in a group; `None` takes it out.
    SetTaskGroup {
        task: String,
        group: Option<u64>,
    },
    /// Which policy holds: Kule's balanced or careful one, or the person's.
    Policy,
    /// Puts one of Kule's policies in place; never over one of the
    /// person's.
    ChoosePolicy {
        choice: kule_engine::PolicyChoice,
    },
    /// A profile file with these parts of the person's choices.
    ExportProfile {
        parts: Vec<crate::settings_profile::Part>,
    },
    /// A profile file's contents, checked, for the person to look at.
    ReadProfile {
        text: String,
    },
    /// Applies these parts of a profile file; says what each did.
    ApplyProfile {
        text: String,
        parts: Vec<crate::settings_profile::Part>,
    },
    /// Where the window's panels are; `null` until it changed them.
    Layout,
    /// Keeps the window's layout; `None` goes back to the window's own.
    SetLayout {
        layout: Option<crate::layout::Layout>,
    },
    /// One directory of the task's copy, for the file explorer.
    ListDir {
        task: String,
        path: String,
    },
    /// A file of the task's copy, as text when it is.
    ReadFile {
        task: String,
        path: String,
    },
    /// A file of the task's copy as the Preview shows it: a
    /// [`kule_view::View`].
    View {
        task: String,
        path: String,
    },
    /// The theme in use and the person's own themes.
    Themes,
    /// Keeps these in place of the ones before; returns them as kept.
    SetThemes {
        themes: crate::themes::Themes,
    },
    /// The keyboard shortcuts the person chose in the app.
    Shortcuts,
    /// Keeps these in place of the ones chosen before.
    SetShortcuts {
        shortcuts: crate::shortcuts::Shortcuts,
    },
    /// Stops the agent at work in the task.
    StopRun {
        task: String,
    },
    /// What this computer cannot give a sandboxed command, and whether
    /// the person accepted running agents with less.
    Isolation,
    SetIsolation {
        accept_weaker: bool,
    },
    /// The limits on the runs of every task that sets none of its own.
    Limits,
    SetLimits {
        limits: kule_engine::Limits,
    },
    /// The limits the task sets itself, field by field.
    SetTaskLimits {
        task: String,
        limits: kule_engine::TaskLimits,
    },
    /// What every task may spend together in 24 hours, and what they did.
    DailyBudget,
    /// `None` for no limit.
    SetDailyBudget {
        budget: Option<kule_engine::Money>,
    },
    /// Every agent profile, the ones that cannot run included.
    AgentList,
    /// The ACP registry, fresh or as last read.
    AgentRegistry,
    /// Writes one of the person's profiles; it then waits for approval.
    SaveAgent {
        profile: kule_acp::Profile,
    },
    RemoveAgent {
        name: String,
    },
    /// Approves a profile, only if it still has the digest the person saw.
    ApproveAgent {
        name: String,
        digest: String,
    },
    RevokeAgent {
        name: String,
    },
    /// Runs a draft profile once in an empty folder; what it was refused.
    TryAgent {
        profile: kule_acp::Profile,
    },
    /// The approvals a person told Kule to remember.
    Rules,
    ForgetRule {
        id: u64,
    },
    /// What an allowance would have let through so far, before it is kept.
    RuleImpact {
        allowance: kule_engine::Allowance,
        #[serde(default)]
        workspace: Option<PathBuf>,
    },
    /// Takes the task's home changes at `paths` to the person's home.
    ApplyHome {
        task: String,
        paths: Vec<String>,
    },
    DiscardHome {
        task: String,
        paths: Vec<String>,
    },
    /// What the home change at `path` would do, line by line.
    HomeDiff {
        task: String,
        path: String,
    },
    /// Takes the task's last apply of home changes back.
    UndoHome {
        task: String,
    },
    /// The person's secrets, without their values.
    Secrets,
    /// Who keeps the key that seals the secrets' values: the system, or a
    /// file; `null` before the first secret.
    SecretsKey,
    /// Keeps a secret for `hosts`, on `workspace` or every folder.
    AddSecret {
        name: String,
        value: Redacted,
        hosts: Vec<String>,
        #[serde(default)]
        workspace: Option<PathBuf>,
    },
    RemoveSecret {
        name: String,
        #[serde(default)]
        workspace: Option<PathBuf>,
    },
    /// The installed extensions, and why any that cannot run cannot.
    Extensions,
    /// Installs or updates the `.kulext` at `path`; it waits for approval.
    InstallExtension {
        path: PathBuf,
    },
    /// Installs a folder under development, read live.
    InstallDevExtension {
        path: PathBuf,
    },
    /// Approves an extension as the person saw it: refused if its digest
    /// is no longer `digest`.
    ApproveExtension {
        id: String,
        digest: String,
    },
    SetExtensionEnabled {
        id: String,
        enabled: bool,
    },
    RemoveExtension {
        id: String,
    },
    /// What an extension kept.
    ExtensionStorage {
        id: String,
    },
    SetExtensionStorage {
        id: String,
        value: Value,
    },
    /// An extension's settings, with defaults for those not chosen.
    ExtensionConfig {
        id: String,
    },
    /// Sets one setting; null puts back its default.
    SetExtensionConfig {
        id: String,
        key: String,
        value: Value,
    },
    /// The themes extensions that may run bring.
    ExtensionThemes,
    /// The MCP servers extensions that may run offer, approved or not.
    McpServers,
    /// Lets agents use an offered MCP server, as the person saw it.
    ApproveMcpServer {
        extension: String,
        name: String,
        digest: String,
    },
    RevokeMcpServer {
        extension: String,
        name: String,
    },
    /// Turns the connection into a stream of notices.
    Subscribe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A secret's value on its way to Kule's home: sent as it is, never shown
/// when a call is printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Redacted(pub String);

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<secret>")
    }
}

#[cfg(test)]
mod redacted {
    use super::*;

    #[test]
    fn a_printed_call_never_shows_a_secret() {
        let call = Call::AddSecret {
            name: "STRIPE_KEY".into(),
            value: Redacted("sk_live_real".into()),
            hosts: vec!["api.stripe.com".into()],
            workspace: None,
        };
        assert!(!format!("{call:?}").contains("sk_live_real"));
        let sent = serde_json::to_string(&call).unwrap();
        assert!(sent.contains("\"value\":\"sk_live_real\""), "{sent}");
    }
}
