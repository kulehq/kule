//! `kule-extension.json`: who the extension is, what it may do, and what it
//! adds to Kule. Every field is checked; an unknown field or permission is
//! refused, never ignored.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, invalid};
use crate::package::plain_path;
use crate::text::{host_ok, slug, version_ok, words};

/// The manifest's name inside a package or a folder.
pub const MANIFEST: &str = "kule-extension.json";
/// The largest manifest read.
const MAX_MANIFEST: usize = 64 * 1024;
const MAX_HOSTS: usize = 16;
const MAX_COMMANDS: usize = 50;
const MAX_PANELS: usize = 8;
const MAX_FIELDS: usize = 32;
const MAX_THEMES: usize = 8;
const MAX_MCP: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    /// The manifest format; 1.
    #[serde(rename = "kule_extension")]
    pub kule_extension: u32,
    /// `publisher.name`, each part 1 to 20 of `a-z0-9-`.
    pub id: String,
    pub name: String,
    pub version: String,
    /// The id's first part. Nobody vouches for it: Kule shows it as unverified.
    pub publisher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The script that runs while the app is open, in a hidden frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<Permission>,
    /// The only hosts `net` may reach.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
}

/// What an extension may ask Kule for. The list is closed: anything else is
/// refused when the extension is read. No permission reaches secrets,
/// answers to questions, commit, rollback, undo, policies, rules, budgets,
/// agent approvals or the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    #[serde(rename = "tasks.read")]
    TasksRead,
    #[serde(rename = "events.read")]
    EventsRead,
    #[serde(rename = "questions.read")]
    QuestionsRead,
    #[serde(rename = "review.read")]
    ReviewRead,
    #[serde(rename = "review.annotate")]
    ReviewAnnotate,
    #[serde(rename = "files.read")]
    FilesRead,
    #[serde(rename = "storage")]
    Storage,
    #[serde(rename = "notify")]
    Notify,
    #[serde(rename = "net")]
    Net,
}

impl Permission {
    pub const ALL: [Permission; 9] = [
        Permission::TasksRead,
        Permission::EventsRead,
        Permission::QuestionsRead,
        Permission::ReviewRead,
        Permission::ReviewAnnotate,
        Permission::FilesRead,
        Permission::Storage,
        Permission::Notify,
        Permission::Net,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Permission::TasksRead => "tasks.read",
            Permission::EventsRead => "events.read",
            Permission::QuestionsRead => "questions.read",
            Permission::ReviewRead => "review.read",
            Permission::ReviewAnnotate => "review.annotate",
            Permission::FilesRead => "files.read",
            Permission::Storage => "storage",
            Permission::Notify => "notify",
            Permission::Net => "net",
        }
    }

    /// What it lets the extension do, for the person approving it.
    pub fn describe(self) -> &'static str {
        match self {
            Permission::TasksRead => "See your tasks: their names, folders and state",
            Permission::EventsRead => {
                "Read what agents say and do while they run, and when tasks are committed or undone"
            }
            Permission::QuestionsRead => {
                "See the questions waiting for you (it cannot answer them)"
            }
            Permission::ReviewRead => "Read a task's changes and their diffs",
            Permission::ReviewAnnotate => {
                "Add notes to a task's review (notes only: it cannot approve or refuse)"
            }
            Permission::FilesRead => "Read files in a task's copy",
            Permission::Storage => "Keep a little data of its own",
            Permission::Notify => "Show notifications",
            Permission::Net => "Send and receive data over the internet, to the hosts listed",
        }
    }

    /// Whether it reads what the person or their agents wrote.
    pub fn reads_work(self) -> bool {
        matches!(
            self,
            Permission::EventsRead | Permission::ReviewRead | Permission::FilesRead
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Contributes {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<Command>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<Panel>,
    /// Settings Kule draws for the extension and keeps for it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configuration: Vec<Field>,
    /// `.kule-theme.json` files inside the extension.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<String>,
    /// An `agents.toml` inside the extension; each profile is approved on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<String>,
    /// MCP servers offered to agents; each is approved on its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServer>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Command {
    pub id: String,
    pub title: String,
    /// A suggested shortcut, such as `Mod+Shift+L`; dropped when Kule uses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Panel {
    pub id: String,
    pub title: String,
    /// The page shown in the panel.
    pub entry: String,
    /// Whether the panel shows something about the chosen task.
    #[serde(default)]
    pub needs_task: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Field {
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    /// The choices of an `enum`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    String,
    Boolean,
    Number,
    Enum,
}

/// The longest string a setting holds.
const MAX_SETTING: usize = 500;

impl Field {
    /// Whether `value` fits this setting.
    pub fn accepts(&self, value: &Value) -> bool {
        match (self.kind, value) {
            (FieldKind::String, Value::String(s)) => {
                s.chars().count() <= MAX_SETTING && !s.chars().any(char::is_control)
            }
            (FieldKind::Boolean, Value::Bool(_)) => true,
            (FieldKind::Number, Value::Number(n)) => n.as_f64().is_some_and(f64::is_finite),
            (FieldKind::Enum, Value::String(s)) => self.values.contains(s),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpServer {
    pub name: String,
    /// `npx` or `uvx`, with an exact version in the arguments.
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl Manifest {
    /// Reads and checks a manifest.
    pub fn parse(bytes: &[u8]) -> Result<Manifest> {
        if bytes.len() > MAX_MANIFEST {
            return Err(invalid(format!(
                "{MANIFEST} is larger than {} KiB",
                MAX_MANIFEST / 1024
            )));
        }
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|e| invalid(format!("{MANIFEST}: {e}")))?;
        manifest.check()?;
        Ok(manifest)
    }

    /// The id Kule's commands, shortcuts and layout know a contribution by.
    pub fn qualified(&self, local: &str) -> String {
        format!("ext.{}.{local}", self.id)
    }

    /// Every file the manifest names, so a package can show it holds them.
    pub fn files(&self) -> Vec<&str> {
        let c = &self.contributes;
        self.main
            .iter()
            .map(String::as_str)
            .chain(c.panels.iter().map(|p| p.entry.as_str()))
            .chain(c.themes.iter().map(String::as_str))
            .chain(c.agents.iter().map(String::as_str))
            .collect()
    }

    pub fn has(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }

    /// The setting `key`, if declared.
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.contributes.configuration.iter().find(|f| f.key == key)
    }

    fn check(&self) -> Result<()> {
        if self.kule_extension != 1 {
            return Err(invalid(format!(
                "{MANIFEST} is format {}; this Kule reads format 1",
                self.kule_extension
            )));
        }
        let (publisher, name) = self
            .id
            .split_once('.')
            .ok_or_else(|| invalid(format!("id {:?} must be publisher.name", self.id)))?;
        slug("the id's publisher", publisher, 20)?;
        slug("the id's name", name, 20)?;
        if publisher != self.publisher {
            return Err(invalid(format!(
                "id {:?} must start with the publisher {:?}",
                self.id, self.publisher
            )));
        }
        words("name", &self.name, 40)?;
        if !version_ok(&self.version) {
            return Err(invalid(format!(
                "version {:?} must be X.Y.Z (with an optional -pre or +build)",
                self.version
            )));
        }
        if let Some(description) = &self.description {
            words("description", description, 300)?;
        }
        if let Some(main) = &self.main {
            file("main", main, &[".js"])?;
        }
        let mut seen = BTreeSet::new();
        for p in &self.permissions {
            if !seen.insert(*p) {
                return Err(invalid(format!(
                    "permission {} is listed twice",
                    p.as_str()
                )));
            }
        }
        if self.hosts.len() > MAX_HOSTS {
            return Err(invalid(format!("more than {MAX_HOSTS} hosts")));
        }
        for host in &self.hosts {
            if !host_ok(host) {
                return Err(invalid(format!(
                    "host {host:?} must be a plain name such as hooks.example.com"
                )));
            }
        }
        unique("host", self.hosts.iter().map(String::as_str))?;
        match (self.has(Permission::Net), self.hosts.is_empty()) {
            (true, true) => return Err(invalid("net needs at least one host")),
            (false, false) => return Err(invalid("hosts are listed without the net permission")),
            _ => {}
        }
        self.contributes.check(self.main.is_some())
    }
}

impl Contributes {
    fn check(&self, has_main: bool) -> Result<()> {
        if self.commands.len() > MAX_COMMANDS {
            return Err(invalid(format!("more than {MAX_COMMANDS} commands")));
        }
        if !self.commands.is_empty() && !has_main {
            return Err(invalid("commands need a main script to run them"));
        }
        for c in &self.commands {
            slug("command id", &c.id, 18)?;
            words("command title", &c.title, 60)?;
            if let Some(keys) = &c.keys {
                words("command keys", keys, 64)?;
            }
        }
        unique("command", self.commands.iter().map(|c| c.id.as_str()))?;
        if self.panels.len() > MAX_PANELS {
            return Err(invalid(format!("more than {MAX_PANELS} panels")));
        }
        for p in &self.panels {
            slug("panel id", &p.id, 18)?;
            words("panel title", &p.title, 30)?;
            file("panel entry", &p.entry, &[".html"])?;
        }
        unique("panel", self.panels.iter().map(|p| p.id.as_str()))?;
        // A panel's id is also the id of the command that shows it.
        if let Some(p) = self
            .panels
            .iter()
            .find(|p| self.commands.iter().any(|c| c.id == p.id))
        {
            return Err(invalid(format!("{:?} is both a command and a panel", p.id)));
        }
        if self.configuration.len() > MAX_FIELDS {
            return Err(invalid(format!("more than {MAX_FIELDS} settings")));
        }
        for f in &self.configuration {
            f.check()?;
        }
        unique("setting", self.configuration.iter().map(|f| f.key.as_str()))?;
        if self.themes.len() > MAX_THEMES {
            return Err(invalid(format!("more than {MAX_THEMES} themes")));
        }
        for t in &self.themes {
            file("theme", t, &[".json"])?;
        }
        if let Some(agents) = &self.agents {
            file("agents", agents, &[".toml"])?;
        }
        if self.mcp_servers.len() > MAX_MCP {
            return Err(invalid(format!("more than {MAX_MCP} MCP servers")));
        }
        for m in &self.mcp_servers {
            m.check()?;
        }
        unique(
            "MCP server",
            self.mcp_servers.iter().map(|m| m.name.as_str()),
        )
    }
}

impl Field {
    fn check(&self) -> Result<()> {
        slug("setting key", &self.key, 32)?;
        words("setting title", &self.title, 60)?;
        if let Some(d) = &self.description {
            words("setting description", d, 200)?;
        }
        match self.kind {
            FieldKind::Enum => {
                if self.values.is_empty() || self.values.len() > 20 {
                    return Err(invalid(format!(
                        "setting {} needs 1 to 20 values",
                        self.key
                    )));
                }
                for v in &self.values {
                    words("setting value", v, 40)?;
                }
                unique("setting value", self.values.iter().map(String::as_str))?;
            }
            _ if !self.values.is_empty() => {
                return Err(invalid(format!(
                    "setting {} lists values but is not an enum",
                    self.key
                )));
            }
            _ => {}
        }
        if let Some(default) = &self.default
            && !self.accepts(default)
        {
            return Err(invalid(format!(
                "setting {}'s default does not fit its type",
                self.key
            )));
        }
        Ok(())
    }
}

impl McpServer {
    fn check(&self) -> Result<()> {
        slug("MCP server name", &self.name, 32)?;
        if !matches!(self.command.as_str(), "npx" | "uvx") {
            return Err(invalid(format!(
                "MCP server {} must start with npx or uvx",
                self.name
            )));
        }
        if self.args.len() > 32 {
            return Err(invalid(format!(
                "MCP server {} has over 32 arguments",
                self.name
            )));
        }
        for a in &self.args {
            words("MCP server argument", a, 200)?;
        }
        if self.env.len() > 16 {
            return Err(invalid(format!(
                "MCP server {} sets over 16 variables",
                self.name
            )));
        }
        for (name, value) in &self.env {
            let plain = !name.is_empty()
                && name.len() <= 64
                && name.starts_with(|c: char| c.is_ascii_uppercase())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
            let risky = name == "PATH"
                || name == "HOME"
                || name.starts_with("LD_")
                || name.starts_with("DYLD_")
                || name.starts_with("NODE_")
                || name.ends_with("PROXY");
            if !plain || risky {
                return Err(invalid(format!(
                    "MCP server {} may not set {name:?}",
                    self.name
                )));
            }
            if value.chars().count() > 500 || value.chars().any(char::is_control) {
                return Err(invalid(format!(
                    "MCP server {}'s {name} is too long or holds control characters",
                    self.name
                )));
            }
        }
        Ok(())
    }
}

fn file(what: &str, path: &str, endings: &[&str]) -> Result<()> {
    if !plain_path(path) || !endings.iter().any(|e| path.ends_with(e)) {
        return Err(invalid(format!(
            "{what} {path:?} must be a plain path inside the extension ending in {}",
            endings.join(" or ")
        )));
    }
    Ok(())
}

fn unique<'a>(what: &str, names: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(invalid(format!("{what} {name:?} is listed twice")));
        }
    }
    Ok(())
}
