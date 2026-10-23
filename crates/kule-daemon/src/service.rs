//! What the daemon does for its clients: tasks, reviews, commits, agent runs
//! and the questions they wait on. The server only carries calls to it over
//! the socket, so its tests run on real tasks without one.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use kule_engine::entities;
use kule_engine::{
    Approvals, Checkpoint, Connection, Denial, Effect, EffectSet, Error, FileOutline, Kind, Policy,
    Preview, Proxy, Question, Ruling, Step, Task, Touched, Verdict, changed_entities, connections,
    denials, meet, record_denials,
};
use serde::{Deserialize, Serialize};

/// Errors reach clients as text.
pub type Result<T> = std::result::Result<T, String>;

/// The most changed files a question before opening the copy outside the
/// sandbox names.
const MAX_SHOWN: usize = 20;

/// What opening a task's copy outside the sandbox comes to (see
/// `Call::OpenOutside`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Outside {
    /// Open it: the copy.
    Open { dir: PathBuf },
    /// The copy holds changes not committed, which a terminal or an editor
    /// there may run: these first (`more` besides), and open only once the
    /// person says so.
    Ask { changed: Vec<String>, more: usize },
}

fn text(e: Error) -> String {
    match e {
        Error::Conflict(paths) => format!(
            "the folder changed since Kule last saw it, at {}; roll back, or start a new \
             task on the folder as it is now",
            paths.join(", ")
        ),
        e => e.to_string(),
    }
}

/// What putting a task in another group would change: whether its policy
/// would, and the verdicts on its pending changes that would.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupImpact {
    /// The group's policy is the one that judges the task now.
    pub same: bool,
    /// Its pending changes whose verdict would change, with the verdict now
    /// and then.
    pub changes: Vec<VerdictChange>,
    /// An agent is at work in the task: its connections keep the policy it
    /// started with until the run ends.
    pub running: bool,
    /// The limits on its runs now, and in the other group.
    pub limits_now: kule_engine::Limits,
    pub limits_then: kule_engine::Limits,
    /// The budgets over its runs now, and in the other group.
    pub budgets_now: Vec<kule_engine::BudgetLeft>,
    pub budgets_then: Vec<kule_engine::BudgetLeft>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerdictChange {
    pub subject: String,
    pub now: Verdict,
    pub then: Verdict,
}

/// A group's own policy as written, and Kule's balanced one to start one
/// from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupPolicyText {
    pub written: Option<String>,
    pub balanced: String,
}

/// What every task may spend together in any 24 hours, and what they spent
/// in the last 24.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyBudget {
    pub budget: Option<kule_engine::Money>,
    pub spent: kule_engine::Spent,
}

/// A budget tasks share, a group's or the daily one for every task, and
/// what was spent against it in its currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedBudget {
    #[serde(flatten)]
    pub scope: kule_engine::BudgetScope,
    pub budget: kule_engine::Money,
    pub spent: f64,
}

/// What this computer cannot give an agent's commands, said for a person,
/// and whether they accepted running with less; with gaps and no
/// acceptance, no agent runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationView {
    pub gaps: Vec<String>,
    pub accept_weaker: bool,
}

/// A project beside a folder that its tasks may read, and why it is not
/// opened now, if it is not (it became a link, or moved).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BesideView {
    #[serde(flatten)]
    pub beside: kule_engine::Beside,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unopened: Option<String>,
}

/// The person's home directory, where Kule hides their own folders.
fn person_home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_owned())
}

/// A task as the task list shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskView {
    pub id: String,
    pub workspace: PathBuf,
    pub created_unix: u64,
    /// Uncommitted changes a commit would take, `.git` internals included.
    pub changes: usize,
    /// Uncommitted changes to paths the workspace ignores, which stay in
    /// the copy unless a commit asks for them.
    pub ignored: usize,
    /// What the policy says about `changes`; `None` when there are none.
    pub verdict: Option<Verdict>,
    pub running: bool,
    /// Questions the task's agent is waiting on.
    pub waiting: usize,
    /// How long a question waits before it is refused.
    pub approval_timeout_secs: u64,
    /// The workspace was moved or deleted: commit and undo are refused.
    #[serde(default)]
    pub workspace_gone: bool,
    /// The copy has not been looked at yet: `changes`, `ignored` and
    /// `verdict` are not known. A [`Notice::Tasks`] follows when they are.
    #[serde(default)]
    pub scanning: bool,
    /// Where its work meets that of other tasks on the same workspace.
    #[serde(default)]
    pub overlaps: Vec<Overlap>,
    /// What the task's runs used, as their agents reported it.
    #[serde(default)]
    pub spent: kule_engine::Spent,
    /// What they may cost together; `None` for no limit.
    #[serde(default)]
    pub budget: Option<kule_engine::Money>,
    /// Changes the task's commands made in the home directory, waiting.
    #[serde(default)]
    pub home_changes: usize,
    /// The name the person gave the task.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color: Option<kule_engine::TaskColor>,
    /// The group the person put the task in.
    #[serde(default)]
    pub group: Option<u64>,
    /// What its runs may take, field by field: its own limits, else its
    /// group's, else every task's.
    #[serde(default)]
    pub limits: kule_engine::Limits,
    /// The limits it sets itself.
    #[serde(default)]
    pub own_limits: kule_engine::TaskLimits,
    /// How long its runs took together.
    #[serde(default)]
    pub time_used_secs: u64,
    /// What the workspace changed since the task's last sync, which a
    /// sync would bring into the copy.
    #[serde(default)]
    pub behind: Behind,
}

/// The most paths of each list a task view carries of what the workspace
/// changed; the counts tell the rest.
const BEHIND_SHOWN: usize = 100;

/// What the workspace changed since a task's last sync (see
/// `Task::behind`), as a task view carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behind {
    /// The first of the paths a sync would bring in or merge.
    pub changed: Vec<String>,
    pub changed_count: usize,
    /// The first of those the task changed too.
    pub both: Vec<String>,
    pub both_count: usize,
    /// The workspace's repository changed.
    pub repository: bool,
}

impl From<kule_engine::Behind> for Behind {
    fn from(b: kule_engine::Behind) -> Behind {
        let first = |paths: &[String]| paths.iter().take(BEHIND_SHOWN).cloned().collect();
        Behind {
            changed: first(&b.changed),
            changed_count: b.changed.len(),
            both: first(&b.both),
            both_count: b.both.len(),
            repository: b.repository,
        }
    }
}

/// The ACP registry as the app shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub entries: Vec<kule_acp::registry::Entry>,
    /// Read from the network now; otherwise as last read.
    pub fresh: bool,
}

/// A file this task and another one both change, and what in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overlap {
    /// The other task.
    pub task: String,
    pub path: String,
    /// What both change inside the file (`method Task::commit`); empty when
    /// Kule cannot tell, and then it is the file as a whole.
    pub entities: Vec<String>,
}

/// The part of a task's view that takes a walk through its copy.
#[derive(Debug, Clone, Default, PartialEq)]
struct Scan {
    changes: usize,
    ignored: usize,
    verdict: Option<Verdict>,
    workspace_gone: bool,
    workspace: PathBuf,
    /// What the work a commit would take changes, inside the files too.
    touched: Touched,
    /// What the workspace changed since the last sync.
    behind: Behind,
}

/// A changed file as last outlined: the metadata of its two sides then
/// (the copy's, the workspace's) and the entities it changed.
struct Outlined {
    stamps: (Option<FileStamp>, Option<FileStamp>),
    entities: Option<BTreeSet<String>>,
}

/// Size, modification and change times, inode: any write moves one.
type FileStamp = (u64, i64, i64, u64, i64, i64);

/// A task's last scan, and whether a newer one is under way.
struct Cached {
    /// `None` before the first scan, or when the last one failed.
    scan: Option<Scan>,
    /// When the last scan started.
    at: Instant,
    scanning: bool,
    /// The last scan could not read the task (being removed, damaged).
    failed: bool,
}

/// How long a scan answers the task list before it is made again. A large
/// copy takes seconds to walk; the list answers at once with the last scan
/// and a fresh one follows as a notice.
const FRESH: Duration = Duration::from_secs(2);

/// The task's uncommitted work, ignored paths marked, and what the policy
/// says about the part a commit would take.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub effects: EffectSet,
    pub ruling: Ruling,
    /// The digest of what a commit would take; a commit that names it is
    /// refused if anything changed since.
    pub digest: String,
    /// What changed inside the files a commit would take, for the ones Kule
    /// outlines.
    #[serde(default)]
    pub outline: Vec<FileOutline>,
    /// What the task's commands wrote in the home directory, caught in the
    /// task's home layer; never part of a commit.
    #[serde(default)]
    pub home: Vec<kule_engine::HomeChange>,
    /// The last apply of home changes, which an undo takes back.
    #[serde(default)]
    pub home_applied: Option<kule_engine::HomeApplied>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Commit {
    Applied {
        checkpoint: u32,
        changes: usize,
        /// Files the folder had changed too, merged with them.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        merged: Vec<String>,
    },
    Nothing,
    /// The policy holds the changes: they wait for approval, or are denied.
    Held {
        ruling: Ruling,
    },
}

/// What happened in the task, newest work last.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    /// The uncommitted work, one step per tool call.
    pub steps: Vec<Step>,
    pub checkpoints: Vec<Checkpoint>,
    pub connections: Vec<Connection>,
    pub denials: Vec<Denial>,
    /// For each step (and `0`, before the first), why the copy cannot go
    /// back to it: a path a later step changed that was not kept then.
    #[serde(default)]
    pub blockers: std::collections::BTreeMap<u32, Option<String>>,
}

/// What a run reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    /// What the person asked; the first event of a run.
    Prompt {
        text: String,
        agent: String,
        /// The places it is about, which the agent gets listed after it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        refs: Vec<kule_view::refs::Reference>,
    },
    Agent {
        event: kule_acp::Event,
    },
    /// What Kule does before the agent starts and that takes a while.
    Preparing {
        text: String,
    },
    /// What the run takes now, every two seconds while it goes on; only
    /// the latest is kept.
    Resources {
        measured: kule_acp::governor::Measured,
    },
    /// The run is over; `error` says why it failed.
    Finished {
        error: Option<String>,
        denials: Vec<Denial>,
    },
}

/// What subscribers hear as it happens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Notice {
    /// `seq` grows with every event, across tasks: a window that loads a
    /// task's [`Service::stream`] drops what it already has.
    Run {
        task: String,
        seq: u64,
        event: RunEvent,
    },
    /// Every question waiting now, after any change.
    Questions { waiting: Vec<Question> },
    /// A task's changes, ignored paths or verdict are not what the task
    /// list last said: ask for it again.
    Tasks,
    /// Something happened to a task as a whole.
    TaskChanged { task: String, change: TaskChange },
    /// An extension was installed, approved, turned on or off, or removed.
    Extensions,
}

/// What happened to a task, for whoever watches (extensions among them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskChange {
    Created,
    /// Changes reached the folder.
    Committed,
    RolledBack,
    /// The newest commit was taken back.
    Undone,
    Removed,
    /// The copy went back to one of its steps.
    WentBack,
    /// What the workspace changed came into the copy.
    Synced,
}

/// How many events of a task's runs are kept for a window that opens later.
const KEPT_EVENTS: usize = 2000;
/// How much of what they say is kept, about: well within what a window
/// takes in one answer.
const KEPT_BYTES: usize = 32 << 20;
/// The longest a message is kept as one; past that its next piece starts
/// another.
const KEPT_MESSAGE: usize = 1 << 20;

/// About how much a kept event takes: what it says.
fn weight(event: &RunEvent) -> usize {
    use kule_acp::Event;
    const EACH: usize = 128;
    EACH + match event {
        RunEvent::Prompt { text, .. } | RunEvent::Preparing { text } => text.len(),
        RunEvent::Agent { event } => match event {
            Event::Message { text, .. }
            | Event::Thought { text, .. }
            | Event::ToolOutput { text, .. }
            | Event::Note { text } => text.len(),
            Event::ToolCall { title, .. } | Event::ToolDone { title, .. } => title.len(),
            Event::Permission { title, chosen, .. } => title.len() + chosen.len(),
            Event::Plan { entries } => entries.iter().map(String::len).sum(),
            _ => 0,
        },
        RunEvent::Finished { error, denials } => {
            error.as_ref().map_or(0, String::len) + denials.len() * EACH
        }
        RunEvent::Resources { .. } => 0,
    }
}

type Subscribers = Arc<Mutex<Vec<Sender<Notice>>>>;

/// A kept run event and its place in the order of all events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sequenced {
    pub seq: u64,
    pub event: RunEvent,
}

pub struct Service {
    home: PathBuf,
    /// Tasks with an agent at work: one agent per task at a time.
    running: Mutex<HashSet<String>>,
    /// What stops each run, by task.
    controls: Mutex<HashMap<String, kule_acp::RunControl>>,
    /// Questions agents are waiting on, from every task.
    approvals: Approvals,
    subscribers: Subscribers,
    /// The latest events of each task's runs, oldest first.
    streams: Mutex<HashMap<String, VecDeque<Sequenced>>>,
    next_seq: std::sync::atomic::AtomicU64,
    /// Each task's last scan, for the task list.
    scans: Mutex<HashMap<String, Cached>>,
    /// Each task's changed files as last outlined, by path.
    outlined: Mutex<HashMap<String, HashMap<String, Outlined>>>,
    /// Ports a run's servers listen on every address, which Kule keeps off
    /// the network (macOS, see `kule_engine::shield`).
    shielded: Mutex<std::collections::BTreeSet<u16>>,
}

impl Service {
    /// Ports a run's servers listen on every address, kept off the network.
    pub fn shielded(&self) -> std::collections::BTreeSet<u16> {
        self.shielded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// macOS: keeps what a task's processes listen on off the network,
    /// looking every 50 ms, while one of its runs goes on or its limits let
    /// its servers listen (its terminals' and strays' too, found by their
    /// sandbox, whatever became of their parent); lets go when there is
    /// none. Runs for the life of the daemon.
    #[cfg(target_os = "macos")]
    pub fn shield(self: Arc<Self>) {
        let mut shield = kule_engine::shield::Shield::new();
        let mut finder = kule_engine::ProcessFinder::new();
        // Each stopped process is told once.
        let mut told: HashSet<(u32, u16)> = HashSet::new();
        // The tasks whose servers may listen, read again each second.
        let mut allowed: Vec<String> = Vec::new();
        let mut read_at: Option<std::time::Instant> = None;
        loop {
            if read_at.is_none_or(|at| at.elapsed() >= Duration::from_secs(1)) {
                allowed = self.servers_allowed();
                read_at = Some(std::time::Instant::now());
            }
            let runs: Vec<String> = self
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .cloned()
                .collect();
            let mut watched = allowed.clone();
            watched.extend(runs.iter().filter(|r| !allowed.contains(r)).cloned());
            if watched.is_empty() {
                if !shield.held().is_empty() {
                    shield.release();
                    self.shielded
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clear();
                }
                told.clear();
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            watched.sort();
            let found = finder.find(&watched);
            let mut pids: Vec<i32> = found.iter().map(|(pid, _)| *pid).collect();
            // What a run started outside a sandbox, too.
            if !runs.is_empty() {
                pids.extend(kule_engine::shield::descendants());
            }
            pids.sort_unstable();
            pids.dedup();
            let guarded = shield.guard(&pids);
            *self
                .shielded
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = guarded.shielded;
            for stopped in guarded.stopped {
                if !told.insert((stopped.pid, stopped.port)) {
                    continue;
                }
                let text = stopped.said();
                let of_task = found
                    .iter()
                    .find(|(pid, _)| *pid as u32 == stopped.pid)
                    .map(|(_, task)| task.clone());
                let to: Vec<&String> = match &of_task {
                    Some(task) => runs.iter().filter(|r| *r == task).collect(),
                    None => runs.iter().collect(),
                };
                for task in to {
                    self.emit(task, RunEvent::Preparing { text: text.clone() });
                }
            }
            // A look takes a few milliseconds; what is left open is this.
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The tasks whose limits let their servers listen.
    #[cfg(target_os = "macos")]
    fn servers_allowed(&self) -> Vec<String> {
        let Ok(tasks) = Task::list(&self.home) else {
            return Vec::new();
        };
        tasks
            .into_iter()
            .filter(|info| {
                Task::open(&self.home, &info.id)
                    .and_then(|t| t.limits_in_force())
                    .is_ok_and(|l| l.servers)
            })
            .map(|info| info.id)
            .collect()
    }

    /// Kule's home, which this service keeps everything in.
    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn new(home: PathBuf) -> Service {
        let approvals = Approvals::new();
        let subscribers = Subscribers::default();
        let heard = Arc::clone(&subscribers);
        approvals.on_change(move |waiting| notify(&heard, Notice::Questions { waiting }));
        Service {
            home,
            running: Mutex::new(HashSet::new()),
            controls: Mutex::new(HashMap::new()),
            approvals,
            subscribers,
            streams: Mutex::new(HashMap::new()),
            next_seq: std::sync::atomic::AtomicU64::new(1),
            scans: Mutex::new(HashMap::new()),
            outlined: Mutex::new(HashMap::new()),
            shielded: Mutex::new(Default::default()),
        }
    }

    /// Notices from now on, until the receiver is dropped.
    pub fn subscribe(&self) -> Receiver<Notice> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(tx);
        rx
    }

    /// The task's latest run events, oldest first.
    pub fn stream(&self, id: &str) -> Vec<Sequenced> {
        self.streams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .map(|events| events.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether anything is going on: an agent at work or a question waiting.
    pub fn busy(&self) -> bool {
        !self
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
            || !self.approvals.waiting().is_empty()
    }

    fn emit(&self, task: &str, event: RunEvent) {
        // Under the lock: the order of `seq` is the order subscribers hear.
        let mut streams = self
            .streams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let kept = streams.entry(task.to_owned()).or_default();
        // An answer arrives in many small chunks; kept as one, the prompt
        // stays in reach of a window that opens later. A piece of another
        // message starts one of its own.
        let merged = match (kept.back_mut(), &event) {
            (
                Some(Sequenced {
                    seq: last,
                    event:
                        RunEvent::Agent {
                            event:
                                kule_acp::Event::Message {
                                    text: kept_text,
                                    id: kept_id,
                                },
                        },
                }),
                RunEvent::Agent {
                    event: kule_acp::Event::Message { text, id },
                },
            ) if kept_id == id && kept_text.len() + text.len() <= KEPT_MESSAGE => {
                kept_text.push_str(text);
                *last = seq;
                true
            }
            _ => false,
        };
        // A tool call's output comes whole each time: the newer one takes
        // the older one's place, at the end, so the order stays the order
        // of `seq`.
        if let RunEvent::Agent {
            event: kule_acp::Event::ToolOutput { id, .. },
        } = &event
        {
            let same = |s: &Sequenced| {
                matches!(&s.event, RunEvent::Agent {
                    event: kule_acp::Event::ToolOutput { id: kept_id, .. },
                } if kept_id == id)
            };
            if let Some(at) = kept.iter().rposition(same) {
                kept.remove(at);
            }
        }
        if matches!(event, RunEvent::Resources { .. })
            && let Some(at) = kept
                .iter()
                .rposition(|s| matches!(s.event, RunEvent::Resources { .. }))
        {
            kept.remove(at);
        }
        if !merged {
            if kept.len() == KEPT_EVENTS {
                kept.pop_front();
            }
            kept.push_back(Sequenced {
                seq,
                event: event.clone(),
            });
        }
        let mut size: usize = kept.iter().map(|s| weight(&s.event)).sum();
        while size > KEPT_BYTES && kept.len() > 1 {
            size -= kept.pop_front().map_or(0, |s| weight(&s.event));
        }
        notify(
            &self.subscribers,
            Notice::Run {
                task: task.to_owned(),
                seq,
                event,
            },
        );
    }

    /// Where the service's proxies put their questions.
    pub fn approvals(&self) -> &Approvals {
        &self.approvals
    }

    pub fn questions(&self) -> Vec<Question> {
        self.approvals.waiting()
    }

    /// Answers a waiting question; an error when it was answered, withdrawn
    /// or timed out meanwhile.
    pub fn answer(&self, id: u64, approve: bool) -> Result<()> {
        if self.approvals.answer(id, approve) {
            Ok(())
        } else {
            Err("that question is no longer waiting".to_owned())
        }
    }

    /// [`Service::answer`]; an approval with `remember` lets the host and
    /// port through on the task's folder from now on.
    pub fn answer_remembering(&self, id: u64, approve: bool, remember: bool) -> Result<()> {
        let question = self.approvals.waiting().into_iter().find(|q| q.id == id);
        self.answer(id, approve)?;
        if let (true, true, Some(q)) = (approve, remember, question) {
            let task = self.open(&q.task)?;
            kule_engine::remember(
                &self.home,
                kule_engine::Allowance::Connect {
                    host: q.host,
                    port: q.port,
                },
                Some(task.workspace()),
            )
            .map_err(text)?;
        }
        Ok(())
    }

    /// The approvals a person told Kule to remember.
    pub fn rules(&self) -> Result<Vec<kule_engine::Remembered>> {
        kule_engine::remembered::load(&self.home).map_err(text)
    }

    pub fn forget_rule(&self, id: u64) -> Result<bool> {
        kule_engine::forget(&self.home, id).map_err(text)
    }

    /// The projects beside folders that their tasks may read.
    pub fn besides(&self) -> Result<Vec<BesideView>> {
        let person = person_home()?;
        Ok(kule_engine::besides(&self.home)
            .map_err(text)?
            .into_iter()
            .map(|beside| BesideView {
                unopened: kule_engine::beside_unopened(&beside.workspace, &beside.path, &person),
                beside,
            })
            .collect())
    }

    /// What `workspace`'s own manifests build from that its tasks cannot
    /// read.
    pub fn suggest_beside(&self, workspace: &Path) -> Result<Vec<PathBuf>> {
        kule_engine::suggest_beside(&self.home, workspace, &person_home()?).map_err(text)
    }

    pub fn add_beside(&self, workspace: &Path, path: &Path) -> Result<kule_engine::Beside> {
        kule_engine::add_beside(&self.home, workspace, path, &person_home()?).map_err(text)
    }

    pub fn remove_beside(&self, id: u64) -> Result<bool> {
        kule_engine::remove_beside(&self.home, id).map_err(text)
    }

    /// The person's secrets, without their values.
    pub fn secrets(&self) -> Result<Vec<kule_engine::SecretInfo>> {
        kule_engine::secrets(&self.home).map_err(text)
    }

    /// Who keeps the key that seals the secrets' values.
    pub fn secrets_key(&self) -> Result<Option<kule_engine::KeyHome>> {
        kule_engine::secrets_key_home(&self.home).map_err(text)
    }

    /// Keeps a secret; the value goes nowhere but into Kule's home.
    pub fn add_secret(
        &self,
        name: &str,
        value: &str,
        hosts: &[String],
        workspace: Option<&Path>,
    ) -> Result<kule_engine::SecretInfo> {
        kule_engine::add_secret(&self.home, name, value, hosts, workspace).map_err(text)
    }

    pub fn remove_secret(&self, name: &str, workspace: Option<&Path>) -> Result<bool> {
        kule_engine::remove_secret(&self.home, name, workspace).map_err(text)
    }

    pub fn rule_impact(
        &self,
        allowance: &kule_engine::Allowance,
        workspace: Option<&Path>,
    ) -> Result<kule_engine::Impact> {
        kule_engine::impact(&self.home, allowance, workspace).map_err(text)
    }

    /// Sets how long the task's questions wait, in minutes.
    pub fn set_approval_timeout(&self, id: &str, minutes: u64) -> Result<TaskView> {
        let task = self.open(id)?;
        task.set_approval_timeout(Duration::from_secs(minutes.saturating_mul(60)))
            .map_err(text)?;
        self.view(id)
    }

    /// Sets what the task's runs may cost together; `None` for no limit.
    pub fn set_budget(&self, id: &str, budget: Option<kule_engine::Money>) -> Result<TaskView> {
        self.open(id)?.set_budget(budget).map_err(text)?;
        self.view(id)
    }

    /// Names the task in the list; blank or `None` takes the name away.
    pub fn set_label(&self, id: &str, label: Option<&str>) -> Result<TaskView> {
        self.open(id)?.set_label(label).map_err(text)?;
        self.view(id)
    }

    /// Marks the task with a colour in the list; `None` for none.
    pub fn set_color(&self, id: &str, color: Option<kule_engine::TaskColor>) -> Result<TaskView> {
        self.open(id)?.set_color(color).map_err(text)?;
        self.view(id)
    }

    /// Puts the task list in the person's order.
    pub fn set_order(&self, ids: &[String]) -> Result<()> {
        kule_engine::set_task_order(&self.home, ids).map_err(text)
    }

    /// The groups, in the person's order.
    pub fn groups(&self) -> Result<Vec<kule_engine::Group>> {
        kule_engine::groups(&self.home).map_err(text)
    }

    pub fn create_group(
        &self,
        name: &str,
        color: Option<kule_engine::TaskColor>,
    ) -> Result<kule_engine::Group> {
        kule_engine::create_group(&self.home, name, color).map_err(text)
    }

    pub fn update_group(
        &self,
        id: u64,
        name: &str,
        color: Option<kule_engine::TaskColor>,
        collapsed: bool,
    ) -> Result<kule_engine::Group> {
        kule_engine::update_group(&self.home, id, name, color, collapsed).map_err(text)
    }

    /// Sets the policy a group's tasks are judged by, from their next
    /// commit and run.
    pub fn set_group_policy(
        &self,
        id: u64,
        policy: kule_engine::GroupPolicy,
        text_: Option<&str>,
    ) -> Result<kule_engine::Group> {
        kule_engine::set_group_policy(&self.home, id, policy, text_).map_err(text)
    }

    /// The policy written for a group, if one was.
    pub fn group_policy_text(&self, id: u64) -> Result<GroupPolicyText> {
        Ok(GroupPolicyText {
            written: kule_engine::group_policy_text(&self.home, id).map_err(text)?,
            balanced: kule_engine::DEFAULT_POLICY.to_owned(),
        })
    }

    pub fn set_group_order(&self, ids: &[u64]) -> Result<()> {
        kule_engine::set_group_order(&self.home, ids).map_err(text)
    }

    /// Deletes a group; its tasks stay, in no group.
    pub fn delete_group(&self, id: u64) -> Result<()> {
        kule_engine::delete_group(&self.home, id).map_err(text)
    }

    /// Puts the task in a group, or takes it out with `None`; its next
    /// commit and run are judged by that group's policy.
    pub fn set_task_group(&self, id: &str, group: Option<u64>) -> Result<TaskView> {
        self.open(id)?.set_group(group).map_err(text)?;
        self.view(id)
    }

    /// What putting the task in `group` (`None` for none) would change, for
    /// the person to see before it does.
    pub fn group_impact(&self, id: &str, group: Option<u64>) -> Result<GroupImpact> {
        let task = self.open(id)?;
        let now = task.group().map_err(text)?;
        let same = Policy::base_text(&task, now).map_err(text)?
            == Policy::base_text(&task, group).map_err(text)?;
        let mut changes = Vec::new();
        if !same {
            let pending = task.pending(false).map_err(text)?;
            let before = self.policy(&task)?.judge(&pending);
            let after = Policy::for_task_in(&task, group)
                .map_err(text)?
                .judge(&pending);
            for then in after.judgements {
                let was = before
                    .judgements
                    .iter()
                    .find(|j| j.subject == then.subject)
                    .map_or(then.verdict, |j| j.verdict);
                if was != then.verdict {
                    changes.push(VerdictChange {
                        subject: then.subject,
                        now: was,
                        then: then.verdict,
                    });
                }
            }
        }
        Ok(GroupImpact {
            same,
            changes,
            running: self
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(id),
            limits_now: task.limits_in_force_in(now).map_err(text)?,
            limits_then: task.limits_in_force_in(group).map_err(text)?,
            budgets_now: task.budgets_in(now).map_err(text)?,
            budgets_then: task.budgets_in(group).map_err(text)?,
        })
    }

    /// Which policy holds.
    pub fn policy_choice(&self) -> Result<kule_engine::PolicyChoice> {
        kule_engine::policy_choice(&self.home).map_err(text)
    }

    /// Puts one of Kule's policies in place, for the next commit and
    /// connection; returns which holds then.
    pub fn choose_policy(
        &self,
        choice: kule_engine::PolicyChoice,
    ) -> Result<kule_engine::PolicyChoice> {
        kule_engine::choose_policy(&self.home, choice).map_err(text)?;
        self.policy_choice()
    }

    /// What every task may spend together in 24 hours, and what they did.
    pub fn daily_budget(&self) -> Result<DailyBudget> {
        Ok(DailyBudget {
            budget: kule_engine::daily_budget(&self.home).map_err(text)?,
            spent: kule_engine::spent_today(&self.home).map_err(text)?,
        })
    }

    /// Every budget tasks share that is set, each group's (all runs, and
    /// any 24 hours) and the daily one, with what was spent against it. The
    /// tasks' runs are read only when one is set.
    pub fn shared_budgets(&self) -> Result<Vec<SharedBudget>> {
        use kule_engine::BudgetScope;
        let mut out = Vec::new();
        let groups = kule_engine::groups(&self.home).map_err(text)?;
        if groups
            .iter()
            .any(|g| g.budget.is_some() || g.daily_budget.is_some())
        {
            let spent = kule_engine::groups_spent(&self.home).map_err(text)?;
            for group in groups {
                let used = spent.get(&group.id).cloned().unwrap_or_default();
                if let Some(budget) = group.budget {
                    out.push(SharedBudget {
                        spent: used.total.in_currency(&budget.currency),
                        scope: BudgetScope::Group {
                            group: group.id,
                            name: group.name.clone(),
                        },
                        budget,
                    });
                }
                if let Some(budget) = group.daily_budget {
                    out.push(SharedBudget {
                        spent: used.today.in_currency(&budget.currency),
                        scope: BudgetScope::GroupDaily {
                            group: group.id,
                            name: group.name,
                        },
                        budget,
                    });
                }
            }
        }
        if let Some(budget) = kule_engine::daily_budget(&self.home).map_err(text)? {
            let spent = kule_engine::spent_today(&self.home).map_err(text)?;
            out.push(SharedBudget {
                spent: spent.in_currency(&budget.currency),
                scope: BudgetScope::Daily,
                budget,
            });
        }
        Ok(out)
    }

    /// Sets what every task may spend together in 24 hours; `None` for no
    /// limit.
    pub fn set_daily_budget(&self, budget: Option<kule_engine::Money>) -> Result<DailyBudget> {
        kule_engine::set_daily_budget(&self.home, budget).map_err(text)?;
        notify(&self.subscribers, Notice::Tasks);
        self.daily_budget()
    }

    /// The agents that can run: Kule's own and the person's approved ones.
    pub fn agents(&self) -> Result<Vec<String>> {
        Ok(kule_acp::Agents::load(&self.home).map_err(text)?.names())
    }

    /// Every profile, with why the ones that cannot run cannot.
    pub fn agent_list(&self) -> Result<Vec<kule_acp::Listed>> {
        Ok(kule_acp::Agents::load(&self.home)
            .map_err(text)?
            .listed()
            .to_vec())
    }

    pub fn agent_registry(&self) -> Result<Registry> {
        let (entries, fresh) = kule_acp::registry::load(&self.home).map_err(text)?;
        Ok(Registry { entries, fresh })
    }

    pub fn save_agent(&self, profile: &kule_acp::Profile) -> Result<()> {
        kule_acp::Agents::save(&self.home, profile).map_err(text)
    }

    pub fn remove_agent(&self, name: &str) -> Result<bool> {
        kule_acp::Agents::remove(&self.home, name).map_err(text)
    }

    pub fn approve_agent(&self, name: &str, digest: &str) -> Result<()> {
        kule_acp::Agents::approve_seen(&self.home, name, digest)
            .map(|_| ())
            .map_err(text)
    }

    pub fn revoke_agent(&self, name: &str) -> Result<bool> {
        kule_acp::Agents::revoke(&self.home, name).map_err(text)
    }

    /// Runs `profile` once in an empty folder, as the person this daemon
    /// runs for; takes as long as the agent does, up to two minutes.
    pub fn try_agent(&self, profile: &kule_acp::Profile) -> Result<kule_acp::trial::Trial> {
        let agents = kule_acp::Agents::load(&self.home).map_err(text)?;
        kule_acp::trial::trial(
            &self.home,
            &agents,
            profile,
            kule_engine::CacheSources::from_env(),
        )
        .map_err(text)
    }

    /// Every task in the person's order, those they have not placed first
    /// and newest first, from each one's last scan; scans older than
    /// [`FRESH`] are made again in the background.
    pub fn tasks(self: &Arc<Self>) -> Result<Vec<TaskView>> {
        let infos = kule_engine::arranged(&self.home).map_err(text)?;
        // One task that cannot be read (being removed, damaged) does not
        // hide the others.
        Ok(infos
            .iter()
            .filter_map(|info| {
                let scan = self.last_scan(&info.id).ok()?;
                self.assemble(&info.id, scan).ok()
            })
            .collect())
    }

    /// The task's last scan, starting a new one in the background when it
    /// is old or there is none; an error when the last one failed.
    fn last_scan(self: &Arc<Self>, id: &str) -> std::result::Result<Option<Scan>, ()> {
        let mut scans = self
            .scans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cached = scans.entry(id.to_owned()).or_insert(Cached {
            scan: None,
            at: Instant::now(),
            scanning: false,
            failed: false,
        });
        let unscanned = cached.scan.is_none() && !cached.failed;
        if !cached.scanning && (unscanned || cached.at.elapsed() >= FRESH) {
            cached.scanning = true;
            let (service, id) = (Arc::clone(self), id.to_owned());
            std::thread::spawn(move || {
                let before = service
                    .scans
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&id)
                    .map(|c| (c.scan.clone(), c.failed));
                let _ = service.rescan(&id);
                let after = service
                    .scans
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&id)
                    .map(|c| (c.scan.clone(), c.failed));
                if after != before {
                    notify(&service.subscribers, Notice::Tasks);
                }
            });
        }
        if cached.failed {
            return Err(());
        }
        Ok(cached.scan.clone())
    }

    /// Walks the task's copy now and keeps what it found.
    fn rescan(&self, id: &str) -> Result<Scan> {
        let started = Instant::now();
        let outcome = self.open(id).and_then(|task| self.scan(&task));
        let mut scans = self
            .scans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A scan that started later (after a commit, say) and finished
        // first knows better.
        if let Some(newer) = scans
            .get_mut(id)
            .filter(|c| (c.scan.is_some() || c.failed) && c.at > started)
        {
            newer.scanning = false;
            return outcome;
        }
        scans.insert(
            id.to_owned(),
            Cached {
                scan: outcome.as_ref().ok().cloned(),
                at: started,
                scanning: false,
                failed: outcome.is_err(),
            },
        );
        outcome
    }

    fn scan(&self, task: &Task) -> Result<Scan> {
        let all = task.effects().map_err(text)?;
        let effects = all.to_commit(false);
        let verdict = (!effects.is_empty()).then(|| self.policy(task).map(|p| p.judge(&effects)));
        Ok(Scan {
            changes: effects.effects.len(),
            ignored: all.ignored().count(),
            verdict: verdict.transpose()?.map(|r| r.verdict),
            workspace_gone: !task.workspace_exists(),
            workspace: task.workspace().to_owned(),
            touched: self.touched(task, &effects),
            // Never what fails the scan: the copy's side stands on its own.
            behind: if task.workspace_exists() {
                task.behind().map(Behind::from).unwrap_or_default()
            } else {
                Behind::default()
            },
        })
    }

    pub fn create(&self, workspace: &Path) -> Result<TaskView> {
        let task = Task::create(&self.home, workspace).map_err(text)?;
        self.changed(task.id(), TaskChange::Created);
        self.view(task.id())
    }

    fn changed(&self, id: &str, change: TaskChange) {
        notify(
            &self.subscribers,
            Notice::TaskChanged {
                task: id.to_owned(),
                change,
            },
        );
    }

    /// Tells subscribers the extensions changed.
    pub fn extensions_changed(&self) {
        notify(&self.subscribers, Notice::Extensions);
    }

    /// The ruling covers the ignored paths only with `include_ignored`.
    pub fn review(&self, id: &str, include_ignored: bool) -> Result<Review> {
        let task = self.open(id)?;
        let effects = task.effects().map_err(text)?;
        let pending = task.pending(include_ignored).map_err(text)?;
        let ruling = self.policy(&task)?.judge(&pending);
        // A help to the review, not part of it: a file Kule cannot read
        // now is listed without one.
        let outline = task.outline(&pending).unwrap_or_default();
        Ok(Review {
            effects,
            ruling,
            digest: pending.digest(),
            outline,
            home: task.home_changes().map_err(text)?,
            home_applied: task.home_last_applied().map_err(text)?,
        })
    }

    /// Takes the home changes at `paths` to the person's home.
    pub fn apply_home(&self, id: &str, paths: &[String]) -> Result<kule_engine::HomeApplied> {
        self.not_at_work(id)?;
        let applied = self.open(id)?.apply_home(paths).map_err(text)?;
        notify(&self.subscribers, Notice::Tasks);
        Ok(applied)
    }

    /// Throws the home changes at `paths` away.
    pub fn discard_home(&self, id: &str, paths: &[String]) -> Result<Vec<String>> {
        self.not_at_work(id)?;
        let gone = self.open(id)?.discard_home(paths).map_err(text)?;
        notify(&self.subscribers, Notice::Tasks);
        Ok(gone)
    }

    /// What the home change at `path` would do to the person's file.
    pub fn home_diff(&self, id: &str, path: &str) -> Result<kule_engine::TextDiff> {
        self.open(id)?.home_diff(path).map_err(text)
    }

    /// Takes the task's last apply of home changes back.
    pub fn undo_home(&self, id: &str) -> Result<kule_engine::HomeApplied> {
        self.not_at_work(id)?;
        let undone = self.open(id)?.undo_home().map_err(text)?;
        notify(&self.subscribers, Notice::Tasks);
        Ok(undone)
    }

    fn not_at_work(&self, id: &str) -> Result<()> {
        if self
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(id)
        {
            return Err(format!("an agent is at work in task {id}"));
        }
        Ok(())
    }

    /// What the picture at `path` in the task was and is.
    pub fn preview(&self, id: &str, path: &str) -> Result<Preview> {
        self.open(id)?.preview(path).map_err(text)
    }

    /// Commits what the policy lets through; with `approve`, also what it
    /// puts to a person. Denied changes never pass. Ignored paths stay in
    /// the copy unless `include_ignored`. With `reviewed`, the digest of a
    /// [`Review`], it commits only if that is still exactly what it takes.
    /// [`Service::commit`]; once an approved commit is in, the files in
    /// `remember` are not asked about again on the task's folder, under the
    /// rules that asked about them this time.
    pub fn commit_remembering(
        &self,
        id: &str,
        approve: bool,
        include_ignored: bool,
        reviewed: Option<&str>,
        remember: &[String],
    ) -> Result<Commit> {
        let mut keep = Vec::new();
        if approve && !remember.is_empty() {
            let task = self.open(id)?;
            let pending = task.pending(include_ignored).map_err(text)?;
            for judgement in self.policy(&task)?.judge(&pending).judgements {
                let rememberable = judgement.verdict == Verdict::Ask
                    && remember.contains(&judgement.subject)
                    && judgement
                        .rules
                        .iter()
                        .all(|r| kule_engine::REMEMBERABLE.contains(&r.as_str()));
                if rememberable {
                    for rule in judgement.rules {
                        keep.push(kule_engine::Allowance::File {
                            path: judgement.subject.clone(),
                            rule,
                        });
                    }
                }
            }
        }
        let outcome = self.commit(id, approve, include_ignored, reviewed)?;
        if matches!(outcome, Commit::Applied { .. }) && !keep.is_empty() {
            let workspace = self.open(id)?.workspace().to_owned();
            for allowance in keep {
                kule_engine::remember(&self.home, allowance, Some(&workspace)).map_err(text)?;
            }
        }
        Ok(outcome)
    }

    pub fn commit(
        &self,
        id: &str,
        approve: bool,
        include_ignored: bool,
        reviewed: Option<&str>,
    ) -> Result<Commit> {
        let task = self.idle(id)?;
        let policy = self.policy(&task)?;
        let gate = policy.gate(approve);
        let check = |set: &EffectSet| {
            if reviewed.is_some_and(|digest| digest != set.digest()) {
                return Err(Error::Invalid(
                    "the changes are not the ones you reviewed any more; review them again".into(),
                ));
            }
            gate(set)
        };
        let outcome = task.commit_with(include_ignored, check);
        let _ = self.rescan(id);
        match outcome {
            Ok(Some(checkpoint)) => {
                self.changed(id, TaskChange::Committed);
                Ok(Commit::Applied {
                    checkpoint: checkpoint.id,
                    changes: checkpoint.effects.len(),
                    merged: checkpoint.merged,
                })
            }
            Ok(None) => Ok(Commit::Nothing),
            Err(Error::Held(ruling)) => Ok(Commit::Held { ruling }),
            Err(e) => Err(text(e)),
        }
    }

    /// Throws the uncommitted work away; returns how many changes it held.
    pub fn rollback(&self, id: &str) -> Result<usize> {
        let set = self.idle(id)?.rollback();
        let _ = self.rescan(id);
        let thrown = set.map_err(text)?.effects.len();
        self.changed(id, TaskChange::RolledBack);
        Ok(thrown)
    }

    /// Brings what the workspace changed since the last sync into the
    /// task's copy, between the agent's runs (see `Task::sync`).
    pub fn sync(&self, id: &str) -> Result<kule_engine::Synced> {
        let synced = self.idle(id)?.sync();
        let _ = self.rescan(id);
        let synced = synced.map_err(text)?;
        if !synced.is_empty() {
            self.changed(id, TaskChange::Synced);
        }
        Ok(synced)
    }

    /// Reverts the newest commit; returns its checkpoint, if there was one.
    pub fn undo(&self, id: &str) -> Result<Option<u32>> {
        let undone = self.idle(id)?.undo();
        let _ = self.rescan(id);
        let undone = undone.map_err(text)?.map(|c| c.id);
        if undone.is_some() {
            self.changed(id, TaskChange::Undone);
        }
        Ok(undone)
    }

    pub fn timeline(&self, id: &str) -> Result<Timeline> {
        let task = self.open(id)?;
        Ok(Timeline {
            steps: task.steps().map_err(text)?,
            checkpoints: task.checkpoints().map_err(text)?,
            connections: connections(&task).map_err(text)?,
            denials: denials(&task).map_err(text)?,
            blockers: task.step_blockers().map_err(text)?,
        })
    }

    /// A new task on the same folder from this one's copy, as it is now or
    /// as it was after step `step`, named after it.
    pub fn fork(&self, id: &str, step: Option<u32>) -> Result<TaskView> {
        let source = self.idle(id)?;
        let fork = source.fork(step);
        let _ = self.rescan(id);
        let fork = fork.map_err(text)?;
        let name = source.label().map_err(text)?.unwrap_or_else(|| {
            source.workspace().file_name().map_or_else(
                || source.id().to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
        });
        let label = match step {
            Some(n) if n > 0 => format!("{name} (fork at step {n})"),
            Some(_) => format!("{name} (fork from the start)"),
            None => format!("{name} (fork)"),
        };
        let label: String = label.chars().take(kule_engine::MAX_LABEL).collect();
        fork.set_label(Some(&label)).map_err(text)?;
        self.changed(fork.id(), TaskChange::Created);
        self.view(fork.id())
    }

    /// Puts the task's copy back as it was after step `step` (`0`: before
    /// the first); returns the step that records it.
    pub fn back_to_step(&self, id: &str, step: u32) -> Result<Step> {
        let back = self.idle(id)?.back_to_step(step);
        let _ = self.rescan(id);
        let back = back.map_err(text)?;
        self.changed(id, TaskChange::WentBack);
        Ok(back)
    }

    /// Where the task's copy is, for the person's own terminal.
    pub fn copy_of(&self, id: &str) -> Result<PathBuf> {
        Ok(self.open(id)?.copy())
    }

    /// File `path` of the copy for the person's own app to open (see
    /// [`crate::files::openable`]), only while no agent is at work, here or
    /// started by `kule`: an agent could otherwise swap it for a link to a
    /// script between the check and the open.
    pub fn openable(&self, id: &str, path: &str) -> Result<PathBuf> {
        let task = self.idle(id)?;
        if let Some(pid) = task.agent_pid() {
            return Err(format!(
                "an agent is at work in task {id} (process {pid}); wait until it is done"
            ));
        }
        crate::files::openable(&task.copy(), path, &self.home.join(crate::files::OPENED))
    }

    /// The copy, to open in `what` outside the sandbox (see
    /// `Call::OpenOutside`), only while no agent is at work here: one could
    /// change what runs there right after the check. Noted before it is
    /// handed out, and no agent starts work in the task until the person
    /// says `what` is closed.
    pub fn open_outside(&self, id: &str, what: &str, confirmed: bool) -> Result<Outside> {
        if what.is_empty() || what.chars().count() > 64 || what.chars().any(char::is_control) {
            return Err("name what the copy opens in, in a few words".to_owned());
        }
        let task = self.idle(id)?;
        let dir = task.terminal_dir().map_err(text)?;
        if !confirmed {
            let effects = task.effects().map_err(text)?;
            let changed: Vec<String> = effects.files().map(|e| e.path.clone()).collect();
            if !changed.is_empty() {
                return Ok(Outside::Ask {
                    more: changed.len().saturating_sub(MAX_SHOWN),
                    changed: changed.into_iter().take(MAX_SHOWN).collect(),
                });
            }
        }
        task.opened_outside(what).map_err(text)?;
        // A run that started meanwhile finds the note; one already at work
        // did not.
        self.idle(id)?.terminal_dir().map_err(text)?;
        Ok(Outside::Open { dir })
    }

    /// What Kule opened the task's copy in outside the sandbox, which may
    /// still be open.
    pub fn outside_opened(&self, id: &str) -> Result<Vec<kule_engine::OpenedOutside>> {
        self.open(id)?.outside().map_err(text)
    }

    /// The person closed what Kule opened the task's copy in outside the
    /// sandbox.
    pub fn forget_outside(&self, id: &str) -> Result<()> {
        self.open(id)?.forget_outside().map_err(text)
    }

    /// Deletes the task and its copy; the workspace is not touched.
    pub fn remove(&self, id: &str) -> Result<()> {
        self.idle(id)?.remove().map_err(text)?;
        self.streams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
        self.scans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
        self.outlined
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
        self.changed(id, TaskChange::Removed);
        let home = self.home.clone();
        std::thread::spawn(move || {
            if let Err(e) = Task::empty_trash(&home) {
                eprintln!("kule: emptying the trash: {e}");
            }
        });
        Ok(())
    }

    /// Gives `agent` the prompt in the background. Subscribers hear the
    /// prompt, the agent's events as they come and [`RunEvent::Finished`]
    /// last, also when the agent could not start. `refs` are places in the
    /// task's copy the prompt is about; the agent gets them listed after it.
    pub fn start_run(
        self: &Arc<Self>,
        id: &str,
        agent: &str,
        prompt: &str,
        refs: Vec<kule_view::refs::Reference>,
    ) -> Result<()> {
        let agents = kule_acp::Agents::load(&self.home).map_err(text)?;
        let profile = agents
            .get(agent)
            .cloned()
            .ok_or_else(|| agents.refusal(agent))?;
        let task = self.open(id)?;
        if refs.len() > kule_view::refs::MAX_REFS {
            return Err(format!(
                "a prompt names at most {} places",
                kule_view::refs::MAX_REFS
            ));
        }
        // Each place is a file the copy holds, reached without a link.
        let mut refs = refs
            .into_iter()
            .map(|r| {
                let r = r.checked()?;
                crate::files::existing(&task.copy(), r.path())?;
                Ok(r)
            })
            .collect::<Result<Vec<_>>>()?;
        // Pictures of regions go to the agent beside the text, not in the
        // run's events.
        let pictures: Vec<kule_acp::Picture> = kule_view::refs::take_crops(&mut refs)?
            .into_iter()
            .map(|c| kule_acp::Picture {
                mime: c.mime,
                data: c.data,
            })
            .collect();
        // The tightest of the task's, its group's and the daily budget;
        // refused, naming it, when one is spent.
        let limit = task.run_budget().map_err(text)?;
        let plan = plan(&task)?;
        if !self
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_owned())
        {
            return Err(format!("an agent is already at work in task {id}"));
        }
        let control = kule_acp::RunControl::new();
        self.controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_owned(), control.clone());
        // Marked in the task too, so the CLI and any other process see it.
        let at_work = match task.agent_at_work() {
            Ok(at_work) => at_work,
            Err(e) => {
                self.running
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(id);
                self.controls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(id);
                return Err(text(e));
            }
        };
        self.emit(
            id,
            RunEvent::Prompt {
                text: prompt.to_owned(),
                agent: agent.to_owned(),
                refs: refs.clone(),
            },
        );
        let prompt = kule_view::refs::with_places(prompt, &refs);
        // What came in from the workspace since the agent's last run, and
        // what the sandbox refused it and why, told once; the person's
        // prompt is shown as they wrote it.
        let prompt = match task.take_agent_notes() {
            Ok(Some(note)) => format!("{note}\n\n{prompt}"),
            _ => prompt,
        };
        let service = Arc::clone(self);
        std::thread::spawn(move || {
            let run = service.run(&task, &profile, &prompt, &pictures, limit, plan, &control);
            let (error, denials) = match run {
                Ok(denials) => (None, denials),
                Err(e) => (Some(e), Vec::new()),
            };
            drop(at_work);
            service
                .controls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(task.id());
            service
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(task.id());
            // Before the window hears the run is over and asks again.
            let _ = service.rescan(task.id());
            service.emit(task.id(), RunEvent::Finished { error, denials });
        });
        Ok(())
    }

    /// Stops the run in task `id` at once: its turn is cancelled, and the
    /// agent killed if it does not end it.
    pub fn stop_run(&self, id: &str) -> Result<()> {
        match self
            .controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
        {
            Some(control) => {
                control.stop("stopped: you stopped the run");
                Ok(())
            }
            None => Err(format!("no agent is at work in task {id}")),
        }
    }

    /// What an agent's commands would go without here; runs go through the
    /// proxy, so a network of their own counts.
    pub fn isolation(&self) -> Result<IsolationView> {
        Ok(IsolationView {
            // As every task's limits have it (see `kule isolation`).
            gaps: kule_engine::isolation_gaps(
                kule_engine::Network::Proxy(0),
                kule_engine::limits(&self.home).map_err(text)?.servers,
            ),
            accept_weaker: kule_engine::isolation(&self.home)
                .map_err(text)?
                .accept_weaker,
        })
    }

    pub fn set_isolation(&self, accept_weaker: bool) -> Result<IsolationView> {
        kule_engine::set_isolation(&self.home, &kule_engine::Isolation { accept_weaker })
            .map_err(text)?;
        self.isolation()
    }

    /// The limits of every task that sets none of its own.
    pub fn limits(&self) -> Result<kule_engine::Limits> {
        kule_engine::limits(&self.home).map_err(text)
    }

    /// Sets [`Service::limits`]; runs going on keep the limits they started
    /// with.
    pub fn set_limits(&self, limits: &kule_engine::Limits) -> Result<kule_engine::Limits> {
        kule_engine::set_limits(&self.home, limits).map_err(text)?;
        notify(&self.subscribers, Notice::Tasks);
        self.limits()
    }

    /// Sets the limits task `id` sets itself.
    pub fn set_task_limits(&self, id: &str, limits: &kule_engine::TaskLimits) -> Result<TaskView> {
        self.open(id)?.set_limits(limits).map_err(text)?;
        self.view(id)
    }

    /// Sets the limits on each of group `id`'s tasks' runs, where a task
    /// sets none of its own; runs going on keep the limits they started
    /// with.
    pub fn set_group_limits(
        &self,
        id: u64,
        limits: &kule_engine::TaskLimits,
    ) -> Result<kule_engine::Group> {
        let group = kule_engine::set_group_limits(&self.home, id, limits).map_err(text)?;
        notify(&self.subscribers, Notice::Tasks);
        Ok(group)
    }

    /// Sets what group `id`'s tasks may cost together: all of their runs,
    /// and any 24 hours; `None` for no limit.
    pub fn set_group_budget(
        &self,
        id: u64,
        budget: Option<kule_engine::Money>,
        daily: Option<kule_engine::Money>,
    ) -> Result<kule_engine::Group> {
        let group = kule_engine::set_group_budget(&self.home, id, budget, daily).map_err(text)?;
        notify(&self.subscribers, Notice::Tasks);
        Ok(group)
    }

    /// What each group's tasks spent, all of their runs and the last 24
    /// hours', by group id.
    pub fn groups_spent(&self) -> Result<std::collections::BTreeMap<u64, kule_engine::GroupSpent>> {
        kule_engine::groups_spent(&self.home).map_err(text)
    }

    /// Every budget task `id`'s next run is held to, with what is left of
    /// each.
    pub fn task_budgets(&self, id: &str) -> Result<Vec<kule_engine::BudgetLeft>> {
        self.open(id)?.budgets().map_err(text)
    }

    /// One run, start to end, held to `plan`; returns what the sandbox
    /// refused.
    #[allow(clippy::too_many_arguments)]
    fn run(
        self: &Arc<Self>,
        task: &Task,
        profile: &kule_acp::Profile,
        prompt: &str,
        pictures: &[kule_acp::Picture],
        limit: Option<kule_engine::Money>,
        plan: kule_acp::governor::Plan,
        control: &kule_acp::RunControl,
    ) -> Result<Vec<Denial>> {
        let started = Instant::now();
        let governor = self.govern(task, plan, control);
        let outcome = self.run_governed(task, profile, prompt, pictures, limit, control, started);
        // The run is over by now, however it ended; one that failed before
        // its agent started (the policy, the proxy, installing or launching
        // the agent) never marked it, and the governor would wait for it
        // for ever.
        control.finish();
        let _ = governor.join();
        outcome
    }

    /// Holds the run `control` names to `plan`, telling subscribers what it
    /// takes as it goes.
    fn govern(
        self: &Arc<Self>,
        task: &Task,
        plan: kule_acp::governor::Plan,
        control: &kule_acp::RunControl,
    ) -> std::thread::JoinHandle<()> {
        let (asked, told) = (Arc::clone(self), Arc::clone(self));
        let (id, told_id) = (task.id().to_owned(), task.id().to_owned());
        kule_acp::governor::govern(
            task,
            plan,
            control.clone(),
            kule_acp::governor::Watch {
                waiting: Box::new(move || asked.approvals.waiting().iter().any(|q| q.task == id)),
                report: Box::new(move |measured| {
                    told.emit(&told_id, RunEvent::Resources { measured });
                }),
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_governed(
        self: &Arc<Self>,
        task: &Task,
        profile: &kule_acp::Profile,
        prompt: &str,
        pictures: &[kule_acp::Picture],
        limit: Option<kule_engine::Money>,
        control: &kule_acp::RunControl,
        started: Instant,
    ) -> Result<Vec<Denial>> {
        let mut usage = kule_engine::RunUsage {
            agent: profile.name.clone(),
            started_unix: unix_now(),
            ..Default::default()
        };
        // Serves this run only, with the policy as it is now; dropped when
        // the run returns, which closes it.
        let policy = profile.open_hosts(self.policy(task)?).map_err(text)?;
        let timeout = task.approval_timeout().map_err(text)?;
        let person = kule_engine::CacheSources::from_env().map(|s| s.home);
        let keys = profile.agent_keys(person.as_deref());
        let proxy =
            Proxy::start_asking(task, policy, &self.approvals, timeout, &keys).map_err(text)?;
        if let Some(spec) = profile.to_install() {
            self.emit(
                task.id(),
                RunEvent::Preparing {
                    text: format!(
                        "Installing {spec} for {}, once; later runs start from it",
                        profile.name
                    ),
                },
            );
        }
        let launch = profile.launch(task, &proxy).map_err(text)?;
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<kule_acp::Event>();
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let (service, id) = (Arc::clone(self), task.id().to_owned());
        let forward = runtime.spawn(async move {
            while let Some(event) = rx.next().await {
                event.tally(&mut usage);
                service.emit(&id, RunEvent::Agent { event });
            }
            usage
        });
        // MCP servers the person approved from extensions; the agent starts
        // them inside its own sandbox.
        let person = std::env::var_os("HOME").map(PathBuf::from);
        let mcp = crate::extensions::mcp_for_runs(&self.home, person.as_deref());
        let outcome = runtime.block_on(kule_acp::run_pictured(
            task,
            launch.command,
            prompt,
            pictures,
            launch.mode.as_deref(),
            limit,
            &mcp,
            // The agent's commands, when it asks Kule to run them, run in
            // its own sandbox, and what it leaves running ends with the run.
            &launch.sandbox,
            true,
            control,
            tx,
        ));
        if let Ok(mut usage) = runtime.block_on(forward) {
            usage.duration_secs = started.elapsed().as_secs();
            // Kept whether the run went well or not: it was spent either way.
            task.record_usage(&usage).map_err(text)?;
        }
        // The agent's process group is gone by now.
        let put_back = launch.kept.restore().map_err(text)?;
        outcome.map_err(|e| format!("{e} (see {})", task.agent_log().display()))?;
        let mut refused = launch
            .sandbox
            .denials(started.elapsed())
            .map_err(text)?
            .unwrap_or_default();
        refused.extend(put_back.iter().map(Denial::put_back));
        record_denials(task, &refused).map_err(text)?;
        Ok(refused)
    }

    /// The task's view from a scan made now.
    fn view(&self, id: &str) -> Result<TaskView> {
        let scan = self.rescan(id)?;
        self.assemble(id, Some(scan))
    }

    /// The task's view: `scan` for what takes a walk through the copy, the
    /// rest as it is now.
    fn assemble(&self, id: &str, scan: Option<Scan>) -> Result<TaskView> {
        let task = self.open(id)?;
        let scanning = scan.is_none();
        let scan = scan.unwrap_or_default();
        Ok(TaskView {
            id: task.id().to_owned(),
            workspace: task.workspace().to_owned(),
            created_unix: task.info().created_unix,
            changes: scan.changes,
            ignored: scan.ignored,
            verdict: scan.verdict,
            running: self
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(id),
            waiting: self
                .approvals
                .waiting()
                .iter()
                .filter(|q| q.task == id)
                .count(),
            approval_timeout_secs: task.approval_timeout().map_err(text)?.as_secs(),
            workspace_gone: scan.workspace_gone,
            scanning,
            overlaps: self.overlaps(id, &scan),
            spent: task.spent().map_err(text)?,
            budget: task.budget().map_err(text)?,
            home_changes: task.home_changes().map_or(0, |c| c.len()),
            label: task.label().map_err(text)?,
            color: task.color().map_err(text)?,
            group: task.group().map_err(text)?,
            limits: task.limits_in_force().map_err(text)?,
            own_limits: task.limits().map_err(text)?,
            time_used_secs: task.time_used().map_err(text)?.as_secs(),
            behind: scan.behind,
        })
    }

    /// Where the task's work meets that of the other tasks on its
    /// workspace, as their last scans have it.
    fn overlaps(&self, id: &str, scan: &Scan) -> Vec<Overlap> {
        let scans = self
            .scans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut overlaps: Vec<Overlap> = scans
            .iter()
            .filter(|(other, _)| other.as_str() != id)
            .filter_map(|(other, cached)| Some((other, cached.scan.as_ref()?)))
            .filter(|(_, theirs)| theirs.workspace == scan.workspace)
            .flat_map(|(other, theirs)| {
                meet(&scan.touched, &theirs.touched)
                    .into_iter()
                    .map(|m| Overlap {
                        task: other.clone(),
                        path: m.path,
                        entities: m.entities,
                    })
            })
            .collect();
        overlaps.sort_by(|a, b| (&a.task, &a.path).cmp(&(&b.task, &b.path)));
        overlaps
    }

    /// What the work a commit would take changes, inside the files too.
    /// A file is outlined again only when either side of it changed since
    /// the last time.
    fn touched(&self, task: &Task, set: &EffectSet) -> Touched {
        let stamp = |root: &Path, path: &str| {
            std::fs::symlink_metadata(root.join(path)).ok().map(|m| {
                (
                    m.len(),
                    m.mtime(),
                    m.mtime_nsec(),
                    m.ino(),
                    m.ctime(),
                    m.ctime_nsec(),
                )
            })
        };
        let (copy, work) = (task.copy(), task.workspace().to_owned());
        let mut known = self
            .outlined
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(task.id())
            .unwrap_or_default();
        let files: Vec<&Effect> = set.files().filter(|e| e.kind != Kind::Dir).collect();
        let mut stale = Vec::new();
        let mut stamps = HashMap::new();
        for effect in &files {
            let now = (stamp(&copy, &effect.path), stamp(&work, &effect.path));
            if known.get(&effect.path).is_none_or(|o| o.stamps != now) {
                stale.push((*effect).clone());
            }
            stamps.insert(effect.path.as_str(), now);
        }
        let stale = EffectSet {
            effects: stale,
            ..EffectSet::default()
        };
        let outlines = task.outline(&stale).unwrap_or_default();
        for effect in &stale.effects {
            let outline = outlines.iter().find(|o| o.path == effect.path);
            // A file left out for time or number is read again next scan.
            if outline.is_none() && entities::supported(&effect.path) {
                known.remove(&effect.path);
                continue;
            }
            known.insert(
                effect.path.clone(),
                Outlined {
                    stamps: stamps[effect.path.as_str()],
                    entities: outline.and_then(|o| changed_entities(&o.outline)),
                },
            );
        }
        known.retain(|path, _| stamps.contains_key(path.as_str()));
        let touched = files
            .iter()
            .map(|e| {
                let inside = known.get(&e.path).and_then(|o| o.entities.clone());
                (e.path.clone(), inside)
            })
            .collect();
        self.outlined
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(task.id().to_owned(), known);
        touched
    }

    fn open(&self, id: &str) -> Result<Task> {
        Task::open(&self.home, id).map_err(text)
    }

    /// The task, refused while an agent works in it: committing or rolling
    /// back under a running agent would split its work in two.
    fn idle(&self, id: &str) -> Result<Task> {
        if self
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(id)
        {
            return Err(format!(
                "an agent is at work in task {id}; wait until it is done"
            ));
        }
        self.open(id)
    }

    /// The task's group's policy, or the one for every task, plus the
    /// workspace's forbids.
    fn policy(&self, task: &Task) -> Result<Policy> {
        Policy::for_task(task).map_err(text)
    }
}

/// The limits the next run of `task` is held to; refused when the task's
/// runs used up its time.
fn plan(task: &Task) -> Result<kule_acp::governor::Plan> {
    let limits = task.limits_in_force().map_err(text)?;
    let left = task.time_left().map_err(text)?;
    if left.is_some_and(|l| l.is_zero()) {
        return Err(format!(
            "task {} has used its time: its runs took {} minutes together; raise its time limit or turn it off to run again",
            task.id(),
            task.time_used().map_err(text)?.as_secs() / 60
        ));
    }
    Ok(kule_acp::governor::Plan::new(&limits, left))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Sends `notice` to every subscriber, forgetting the ones that left.
fn notify(subscribers: &Subscribers, notice: Notice) {
    subscribers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|tx| tx.send(notice.clone()).is_ok());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use kule_engine::CheckpointState;
    use tempfile::TempDir;

    use super::*;

    /// A thread that died holding one of the service's locks leaves the
    /// daemon answering, not panicking at every call after.
    #[test]
    fn a_lock_left_by_a_panic_does_not_take_the_daemon_down() {
        let s = Setup::new();
        let app = Arc::clone(&s.app);
        let _ = std::thread::spawn(move || {
            let _held = app.controls.lock().unwrap();
            panic!("a run's thread failed");
        })
        .join();
        assert!(s.app.controls.is_poisoned());
        assert!(s.app.stop_run("nope").is_err());

        // The notices go on too.
        let heard = s.app.subscribe();
        let subscribers = Arc::clone(&s.app.subscribers);
        let _ = std::thread::spawn(move || {
            let _held = subscribers.lock().unwrap();
            panic!("a notice's thread failed");
        })
        .join();
        assert!(s.app.subscribers.is_poisoned());
        notify(&s.app.subscribers, Notice::Tasks);
        assert_eq!(heard.try_recv(), Ok(Notice::Tasks));
    }

    struct Setup {
        _home: TempDir,
        ws: TempDir,
        app: Arc<Service>,
    }

    impl Setup {
        fn new() -> Setup {
            let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
            fs::write(ws.path().join("a.txt"), "a\n").unwrap();
            let app = Arc::new(Service::new(home.path().to_owned()));
            Setup {
                _home: home,
                ws,
                app,
            }
        }

        fn task(&self) -> (String, PathBuf) {
            let view = self.app.create(self.ws.path()).unwrap();
            let copy = Task::open(&self.app.home, &view.id).unwrap().copy();
            (view.id, copy)
        }

        /// Ages every scan past [`FRESH`] so the list makes them again.
        fn age_scans(&self) {
            for cached in self
                .app
                .scans
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values_mut()
            {
                cached.at = cached.at.checked_sub(FRESH).unwrap();
            }
        }

        /// The task list once the scans it starts are done, as after a
        /// change made outside the daemon (a terminal in the copy).
        fn settled(&self) -> Vec<TaskView> {
            self.age_scans();
            self.app.tasks().unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while self
                .app
                .scans
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values()
                .any(|c| c.scanning)
            {
                assert!(Instant::now() < deadline, "a scan never ended");
                std::thread::sleep(Duration::from_millis(10));
            }
            self.app.tasks().unwrap()
        }
    }

    #[test]
    fn tasks_on_one_workspace_tell_where_they_meet() {
        let s = Setup::new();
        let code = "impl Task {\n    fn commit(&self) {}\n    fn undo(&self) {}\n}\n";
        fs::write(s.ws.path().join("task.rs"), code).unwrap();
        let (a, copy_a) = s.task();
        let (b, copy_b) = s.task();
        let (c, copy_c) = s.task();
        // a and b change the same method; c another one in the same file.
        let commit = "impl Task {\n    fn commit(&self) { 1; }\n    fn undo(&self) {}\n}\n";
        fs::write(copy_a.join("task.rs"), commit).unwrap();
        fs::write(copy_b.join("task.rs"), commit.replace("1;", "2;")).unwrap();
        let undo = "impl Task {\n    fn commit(&self) {}\n    fn undo(&self) { 3; }\n}\n";
        fs::write(copy_c.join("task.rs"), undo).unwrap();
        // a and c both change a file Kule does not read inside.
        fs::write(copy_a.join("a.txt"), "x\n").unwrap();
        fs::write(copy_c.join("a.txt"), "y\n").unwrap();

        let views = s.settled();
        let view = |id: &str| views.iter().find(|v| v.id == id).unwrap().clone();
        let meets = |id: &str| -> Vec<(String, String, Vec<String>)> {
            view(id)
                .overlaps
                .into_iter()
                .map(|o| (o.task, o.path, o.entities))
                .collect()
        };
        let mut expect_a = vec![
            (
                b.clone(),
                "task.rs".to_owned(),
                vec!["method Task::commit".to_owned()],
            ),
            (c.clone(), "a.txt".to_owned(), vec![]),
        ];
        expect_a.sort();
        assert_eq!(meets(&a), expect_a);
        assert_eq!(
            meets(&b),
            [(
                a.clone(),
                "task.rs".to_owned(),
                vec!["method Task::commit".to_owned()]
            )]
        );
        assert_eq!(meets(&c), [(a.clone(), "a.txt".to_owned(), vec![])]);

        // b's agent moves on to undo: it meets c there now, and a no more.
        fs::write(copy_b.join("task.rs"), undo.replace("3;", "4;")).unwrap();
        let views = s.settled();
        let b_meets: Vec<(String, Vec<String>)> = views
            .iter()
            .find(|v| v.id == b)
            .unwrap()
            .overlaps
            .iter()
            .map(|o| (o.task.clone(), o.entities.clone()))
            .collect();
        assert_eq!(b_meets, [(c.clone(), vec!["method Task::undo".to_owned()])]);

        // A task on another workspace meets nobody.
        let elsewhere = TempDir::new().unwrap();
        fs::write(elsewhere.path().join("task.rs"), code).unwrap();
        let other = s.app.create(elsewhere.path()).unwrap().id;
        let other_copy = Task::open(&s.app.home, &other).unwrap().copy();
        fs::write(other_copy.join("task.rs"), commit).unwrap();
        let views = s.settled();
        assert!(
            views
                .iter()
                .find(|v| v.id == other)
                .unwrap()
                .overlaps
                .is_empty()
        );

        // Once a rolls back, nobody meets it any more.
        s.app.rollback(&a).unwrap();
        assert!(
            s.settled()
                .iter()
                .all(|v| v.overlaps.iter().all(|o| o.task != a))
        );
    }

    /// Watchers (extensions among them) hear what happens to a task as a whole.
    #[test]
    fn subscribers_hear_a_task_created_committed_rolled_back_undone_and_removed() {
        let s = Setup::new();
        let notices = s.app.subscribe();
        let (id, copy) = s.task();
        fs::write(copy.join("b.txt"), "b\n").unwrap();
        assert!(matches!(
            s.app.commit(&id, true, false, None).unwrap(),
            Commit::Applied { .. }
        ));
        fs::write(copy.join("c.txt"), "c\n").unwrap();
        s.app.rollback(&id).unwrap();
        s.app.undo(&id).unwrap();
        s.app.remove(&id).unwrap();
        let heard: Vec<TaskChange> =
            std::iter::from_fn(|| notices.recv_timeout(Duration::from_secs(2)).ok())
                .filter_map(|n| match n {
                    Notice::TaskChanged { task, change } if task == id => Some(change),
                    _ => None,
                })
                .collect();
        assert_eq!(
            heard,
            [
                TaskChange::Created,
                TaskChange::Committed,
                TaskChange::RolledBack,
                TaskChange::Undone,
                TaskChange::Removed
            ]
        );
    }

    #[test]
    fn the_view_shows_what_the_folder_changed_until_a_sync_brings_it_in() {
        let s = Setup::new();
        let notices = s.app.subscribe();
        let (id, copy) = s.task();
        fs::write(s.ws.path().join("new.txt"), "new\n").unwrap();

        let view = s.app.view(&id).unwrap();
        assert_eq!(view.behind.changed, ["new.txt"]);
        assert_eq!(view.behind.changed_count, 1);

        let synced = s.app.sync(&id).unwrap();
        assert_eq!(synced.brought, ["new.txt"]);
        assert!(copy.join("new.txt").exists());
        let view = s.app.view(&id).unwrap();
        assert_eq!(view.behind, Behind::default());
        assert_eq!(view.changes, 0);
        // Nothing to bring in is no change of the task's.
        s.app.sync(&id).unwrap();

        let heard: Vec<TaskChange> =
            std::iter::from_fn(|| notices.recv_timeout(Duration::from_secs(2)).ok())
                .filter_map(|n| match n {
                    Notice::TaskChanged { task, change } if task == id => Some(change),
                    _ => None,
                })
                .collect();
        assert_eq!(heard, [TaskChange::Created, TaskChange::Synced]);
    }

    #[test]
    fn a_view_carries_the_first_paths_the_folder_changed_and_counts_the_rest() {
        let s = Setup::new();
        let (id, _) = s.task();
        for n in 0..BEHIND_SHOWN + 5 {
            fs::write(s.ws.path().join(format!("f{n:03}.txt")), "x\n").unwrap();
        }
        let behind = s.app.view(&id).unwrap().behind;
        assert_eq!(behind.changed.len(), BEHIND_SHOWN);
        assert_eq!(behind.changed_count, BEHIND_SHOWN + 5);
    }

    /// A large copy takes seconds to walk: the list answers at once from
    /// the last scan, and says when a fresh one finds something else.
    #[test]
    fn the_list_answers_from_the_last_scan_and_tells_when_it_changes() {
        let s = Setup::new();
        let (id, copy) = s.task();
        let notices = s.app.subscribe();
        fs::write(copy.join("b.txt"), "b\n").unwrap();

        // Scanned just now, at creation: not again yet.
        assert_eq!(s.app.tasks().unwrap()[0].changes, 0);
        s.age_scans();
        assert_eq!(
            s.app.tasks().unwrap()[0].changes,
            0,
            "answered from the last scan"
        );
        let heard = notices.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(heard, Notice::Tasks), "{heard:?}");
        assert_eq!(s.app.tasks().unwrap()[0].changes, 1);

        // Kule's own changes show at once.
        s.app.rollback(&id).unwrap();
        assert_eq!(s.app.tasks().unwrap()[0].changes, 0);
    }

    /// A task never scanned yet is listed at once, marked as such.
    #[test]
    fn a_task_not_scanned_yet_is_listed_as_scanning() {
        let s = Setup::new();
        let (id, _) = s.task();
        s.app
            .scans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        let view = s.app.tasks().unwrap().remove(0);
        assert_eq!((view.id.as_str(), view.scanning), (id.as_str(), true));
        assert!(!s.settled()[0].scanning);
    }

    #[test]
    fn tasks_come_with_their_changes_and_verdict() {
        let s = Setup::new();
        let (first, copy) = s.task();
        let (second, _) = s.task();
        fs::write(copy.join("b.txt"), "b\n").unwrap();
        fs::write(copy.join("Cargo.toml"), "[package]\n").unwrap();

        // Both were made within a second, which is all creation times tell.
        let tasks = s.settled();
        assert!(tasks.is_sorted_by_key(|t| std::cmp::Reverse(t.created_unix)));
        let get = |id: &str| tasks.iter().find(|t| t.id == id).unwrap();
        assert_eq!((get(&second).changes, get(&second).verdict), (0, None));
        let first = get(&first);
        assert_eq!((first.changes, first.verdict), (2, Some(Verdict::Ask)));
        assert!(!first.running);
    }

    #[test]
    fn the_careful_policy_holds_every_change_for_the_next_commit() {
        use kule_engine::PolicyChoice;
        let s = Setup::new();
        let (id, copy) = s.task();
        assert_eq!(s.app.policy_choice().unwrap(), PolicyChoice::Balanced);
        assert_eq!(
            s.app.choose_policy(PolicyChoice::Careful).unwrap(),
            PolicyChoice::Careful
        );
        fs::write(copy.join("a.txt"), "edited\n").unwrap();
        assert!(matches!(
            s.app.commit(&id, false, false, None).unwrap(),
            Commit::Held { .. }
        ));
        s.app.choose_policy(PolicyChoice::Balanced).unwrap();
        assert!(matches!(
            s.app.commit(&id, false, false, None).unwrap(),
            Commit::Applied { .. }
        ));
    }

    #[test]
    fn a_tasks_group_policy_judges_its_commit() {
        use kule_engine::{GroupPolicy, TaskColor};
        let s = Setup::new();
        let (held, held_copy) = s.task();
        let (free, free_copy) = s.task();
        let group = s
            .app
            .create_group("careful ones", Some(TaskColor::Red))
            .unwrap();
        s.app
            .set_group_policy(group.id, GroupPolicy::Careful, None)
            .unwrap();
        let view = s.app.set_task_group(&held, Some(group.id)).unwrap();
        assert_eq!(view.group, Some(group.id));
        assert!(s.app.set_task_group(&free, Some(group.id + 1)).is_err());

        // Only the task in the group waits; the other keeps the policy for
        // every task.
        fs::write(held_copy.join("a.txt"), "edited\n").unwrap();
        fs::write(free_copy.join("a.txt"), "edited\n").unwrap();
        assert!(matches!(
            s.app.commit(&held, false, false, None).unwrap(),
            Commit::Held { .. }
        ));
        assert!(matches!(
            s.app.commit(&free, false, false, None).unwrap(),
            Commit::Applied { .. }
        ));

        // Out of its group, it is judged as the rest.
        s.app.delete_group(group.id).unwrap();
        assert!(s.app.groups().unwrap().is_empty());
        assert!(matches!(
            s.app.commit(&held, false, false, None).unwrap(),
            Commit::Applied { .. }
        ));
    }

    #[test]
    fn moving_a_task_says_first_which_verdicts_would_change() {
        use kule_engine::GroupPolicy;
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("a.txt"), "edited\n").unwrap();
        fs::write(copy.join("Cargo.toml"), "[package]\n").unwrap();
        let same = s.app.create_group("same", None).unwrap();
        let careful = s.app.create_group("careful", None).unwrap();
        s.app
            .set_group_policy(careful.id, GroupPolicy::Careful, None)
            .unwrap();
        let trusting = s.app.create_group("trusting", None).unwrap();
        s.app
            .set_group_policy(
                trusting.id,
                GroupPolicy::Own,
                Some(r#"@id("all") permit (principal, action, resource);"#),
            )
            .unwrap();
        let change = |subject: &str, now, then| VerdictChange {
            subject: subject.to_owned(),
            now,
            then,
        };

        let impact = s.app.group_impact(&id, Some(same.id)).unwrap();
        assert!(impact.same && impact.changes.is_empty() && !impact.running);
        let impact = s.app.group_impact(&id, Some(careful.id)).unwrap();
        assert!(!impact.same);
        assert_eq!(
            impact.changes,
            [change("a.txt", Verdict::Allow, Verdict::Ask)]
        );
        let impact = s.app.group_impact(&id, Some(trusting.id)).unwrap();
        assert_eq!(
            impact.changes,
            [change("Cargo.toml", Verdict::Ask, Verdict::Allow)]
        );
        // Asking changes nothing; out of the trusting group, it is judged
        // as the rest again.
        assert_eq!(s.app.groups().unwrap().len(), 3);
        s.app.set_task_group(&id, Some(trusting.id)).unwrap();
        let impact = s.app.group_impact(&id, None).unwrap();
        assert_eq!(
            impact.changes,
            [change("Cargo.toml", Verdict::Allow, Verdict::Ask)]
        );
    }

    #[test]
    fn the_list_keeps_the_names_colours_and_order_a_person_gave() {
        let s = Setup::new();
        let (a, _) = s.task();
        let (b, _) = s.task();
        let view = s.app.set_label(&a, Some(" invoices ")).unwrap();
        assert_eq!(view.label.as_deref(), Some("invoices"));
        let view = s
            .app
            .set_color(&a, Some(kule_engine::TaskColor::Green))
            .unwrap();
        assert_eq!(view.color, Some(kule_engine::TaskColor::Green));
        assert!(s.app.set_label(&a, Some("a\u{202E}b")).is_err());
        assert!(s.app.set_label("nope", Some("x")).is_err());

        s.app.set_order(&[a.clone(), b.clone()]).unwrap();
        let ids = |tasks: Vec<TaskView>| tasks.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(s.settled()), [a.clone(), b.clone()]);
        s.app.set_order(&[b.clone(), a.clone()]).unwrap();
        let tasks = s.settled();
        assert_eq!(tasks[1].label.as_deref(), Some("invoices"));
        assert_eq!(ids(tasks), [b.clone(), a.clone()]);
        assert!(s.app.set_order(&[b.clone(), b]).is_err());
    }

    #[test]
    fn commit_follows_the_policy() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("a.txt"), "edited\n").unwrap();
        assert!(matches!(
            s.app.commit(&id, false, false, None).unwrap(),
            Commit::Applied {
                checkpoint: 1,
                changes: 1,
                ..
            }
        ));
        assert!(matches!(
            s.app.commit(&id, false, false, None).unwrap(),
            Commit::Nothing
        ));

        fs::write(copy.join("Cargo.toml"), "[package]\n").unwrap();
        match s.app.commit(&id, false, false, None).unwrap() {
            Commit::Held { ruling } => assert_eq!(ruling.verdict, Verdict::Ask),
            other => panic!("expected the change to be held, got {other:?}"),
        }
        assert!(!s.ws.path().join("Cargo.toml").exists());
        let review = s.app.review(&id, false).unwrap();
        assert_eq!(review.ruling.verdict, Verdict::Ask);
        assert_eq!(review.effects.effects.len(), 1);

        assert!(matches!(
            s.app.commit(&id, true, false, None).unwrap(),
            Commit::Applied { checkpoint: 2, .. }
        ));
        assert!(s.ws.path().join("Cargo.toml").exists());
    }

    #[test]
    fn a_denied_change_is_held_even_when_approved() {
        let s = Setup::new();
        fs::create_dir(s.ws.path().join(".kule")).unwrap();
        fs::write(
            s.ws.path().join(".kule/policy.cedar"),
            "forbid (principal, action, resource) when { resource has name && resource.name == \"secret.txt\" };\n",
        )
        .unwrap();
        let (id, copy) = s.task();
        fs::write(copy.join("secret.txt"), "x\n").unwrap();
        match s.app.commit(&id, true, false, None).unwrap() {
            Commit::Held { ruling } => assert_eq!(ruling.verdict, Verdict::Deny),
            other => panic!("expected the change to be denied, got {other:?}"),
        }
        assert!(!s.ws.path().join("secret.txt").exists());
    }

    #[test]
    fn rollback_undo_and_the_timeline() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("b.txt"), "b\n").unwrap();
        assert_eq!(s.app.rollback(&id).unwrap(), 1);
        assert!(!copy.join("b.txt").exists());

        fs::write(copy.join("b.txt"), "b\n").unwrap();
        Task::open(&s.app.home, &id)
            .unwrap()
            .step("write b")
            .unwrap();
        let timeline = s.app.timeline(&id).unwrap();
        assert_eq!(timeline.steps.len(), 1);
        assert!(timeline.checkpoints.is_empty());

        s.app.commit(&id, false, false, None).unwrap();
        let timeline = s.app.timeline(&id).unwrap();
        assert!(timeline.steps.is_empty());
        assert_eq!(timeline.checkpoints.len(), 1);
        assert_eq!(timeline.checkpoints[0].state, CheckpointState::Applied);

        assert_eq!(s.app.undo(&id).unwrap(), Some(1));
        assert!(!s.ws.path().join("b.txt").exists());
        let timeline = s.app.timeline(&id).unwrap();
        assert_eq!(timeline.checkpoints[0].state, CheckpointState::Undone);
        assert_eq!(s.app.undo(&id).unwrap(), None);
    }

    #[test]
    fn a_workspace_changed_outside_kule_is_explained() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("a.txt"), "agent\n").unwrap();
        fs::write(s.ws.path().join("a.txt"), "human, a longer line\n").unwrap();
        let e = s.app.commit(&id, false, false, None).unwrap_err();
        assert!(e.contains("since Kule last saw it, at a.txt"), "{e}");
    }

    #[test]
    fn a_task_with_an_agent_at_work_refuses_changes() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("b.txt"), "b\n").unwrap();
        fs::write(copy.join("summary.pdf"), "%PDF-1.7\n").unwrap();
        assert!(s.app.openable(&id, "summary.pdf").is_ok());
        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());

        assert!(s.app.tasks().unwrap()[0].running);
        for refused in [
            s.app.commit(&id, true, false, None).map(|_| ()),
            s.app.rollback(&id).map(|_| ()),
            s.app.undo(&id).map(|_| ()),
            s.app.sync(&id).map(|_| ()),
            s.app.remove(&id),
            s.app.start_run(&id, "claude-code", "hi", Vec::new()),
            // The agent could swap the file for a link between the check
            // and the open.
            s.app.openable(&id, "summary.pdf").map(|_| ()),
        ] {
            assert!(refused.unwrap_err().contains("at work"));
        }
        assert!(copy.join("b.txt").exists());

        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        // An agent `kule run` started, which the daemon does not run.
        let at_work = s.app.open(&id).unwrap().agent_at_work().unwrap();
        assert!(
            s.app
                .openable(&id, "summary.pdf")
                .unwrap_err()
                .contains("at work")
        );
        drop(at_work);
        assert!(s.app.openable(&id, "summary.pdf").is_ok());
        s.app.remove(&id).unwrap();
        assert!(s.app.tasks().unwrap().is_empty());
    }

    /// A run that has started, before its agent marks the task: the person's
    /// terminal or editor waits, since the agent could change what runs
    /// there right after the check.
    #[test]
    fn no_terminal_while_a_run_is_starting_or_at_work() {
        let s = Setup::new();
        let (id, _) = s.task();
        let opens = |s: &Setup| s.app.open_outside(&id, "Terminal", true);
        assert!(opens(&s).is_ok());
        s.app.open(&id).unwrap().forget_outside().unwrap();
        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());
        let e = opens(&s).unwrap_err();
        assert!(e.contains("at work"), "{e}");
        assert!(s.app.open(&id).unwrap().outside().unwrap().is_empty());
        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        assert!(opens(&s).is_ok());
    }

    /// The copy opens in a terminal app or an editor while it holds changes
    /// not committed only once the person saw them; once opened, no run
    /// starts until the person says it is closed. A terminal open on the
    /// copy holds runs off too.
    #[test]
    fn the_copy_opens_outside_only_once_its_changes_were_seen() {
        let s = Setup::new();
        let (id, copy) = s.task();
        std::fs::write(copy.join("b.txt"), "b\n").unwrap();
        assert_eq!(
            s.app.open_outside(&id, "VS Code", false).unwrap(),
            Outside::Ask {
                changed: vec!["b.txt".to_owned()],
                more: 0
            }
        );
        let task = s.app.open(&id).unwrap();
        assert!(task.outside().unwrap().is_empty());
        for bad in ["", "VS\nCode", &"x".repeat(65)] {
            assert!(s.app.open_outside(&id, bad, true).is_err());
        }
        assert!(matches!(
            s.app.open_outside(&id, "VS Code", true).unwrap(),
            Outside::Open { .. }
        ));
        let not_running = |s: &Setup| {
            !s.app
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&id)
        };
        let e = s
            .app
            .start_run(&id, "claude-code", "hi", Vec::new())
            .unwrap_err();
        assert!(e.contains("VS Code") && e.contains("kule closed"), "{e}");
        assert!(not_running(&s));
        task.forget_outside().unwrap();

        let open = task.terminal_opened().unwrap();
        let e = s
            .app
            .start_run(&id, "claude-code", "hi", Vec::new())
            .unwrap_err();
        assert!(e.contains("terminal is open"), "{e}");
        assert!(not_running(&s));
        drop(open);

        // Nothing changed: nothing to see first.
        s.app.rollback(&id).unwrap();
        assert!(matches!(
            s.app.open_outside(&id, "Terminal", false).unwrap(),
            Outside::Open { .. }
        ));
    }

    /// What the app shows, word for word, when rolling back, undoing or
    /// going back to a step waits for a terminal open on the task, and when
    /// a terminal waits for Kule writing in the copy: nothing in it the
    /// person cannot act on (a process id, Kule's own).
    #[test]
    fn what_waits_for_a_terminal_says_so_as_the_app_shows_it() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("b.txt"), "b\n").unwrap();
        let task = s.app.open(&id).unwrap();
        task.step("b").unwrap();
        let open = task.terminal_opened().unwrap();
        let waits = |doing: &str| {
            format!(
                "a terminal is open on this task's copy (in Kule's app, or kule shell); \
                 close it first, then {doing}"
            )
        };
        assert_eq!(s.app.rollback(&id).unwrap_err(), waits("roll back"));
        assert_eq!(s.app.undo(&id).unwrap_err(), waits("undo"));
        assert_eq!(
            s.app.back_to_step(&id, 0).unwrap_err(),
            waits("go back to a step")
        );
        assert_eq!(
            s.app.commit(&id, true, false, None).unwrap_err(),
            waits("commit")
        );
        assert_eq!(s.app.sync(&id).unwrap_err(), waits("sync"));
        assert!(copy.join("b.txt").exists());
        drop(open);

        let lock = s.app.home.join("tasks").join(&id).join("lock");
        fs::write(&lock, "1\n").unwrap();
        assert_eq!(
            task.terminal_opened().map(|_| ()).unwrap_err().to_string(),
            "Kule is writing in this task's copy (a commit, sync or rollback); \
             open the terminal once that is done"
        );
        fs::remove_file(&lock).unwrap();
        assert_eq!(s.app.rollback(&id).unwrap(), 1);
    }

    #[test]
    fn home_changes_are_reviewed_and_applied_apart_from_the_commit() {
        let s = Setup::new();
        let (id, _) = s.task();
        let person = TempDir::new().unwrap();
        fs::write(person.path().join(".zshrc"), "a\n").unwrap();
        let task = s.app.open(&id).unwrap();
        let layer = task
            .prepare_home(person.path(), &[], &[], &[], &[])
            .unwrap();
        fs::write(layer.join(".zshrc"), "a\nb\n").unwrap();

        let review = s.app.review(&id, false).unwrap();
        assert_eq!(review.home.len(), 1);
        assert!(review.effects.is_empty());
        assert_eq!(s.app.view(&id).unwrap().home_changes, 1);

        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());
        let e = s.app.apply_home(&id, &[".zshrc".into()]).unwrap_err();
        assert!(e.contains("at work"), "{e}");
        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);

        let applied = s.app.apply_home(&id, &[".zshrc".into()]).unwrap();
        assert_eq!(applied.paths, [".zshrc"]);
        assert_eq!(
            fs::read_to_string(person.path().join(".zshrc")).unwrap(),
            "a\nb\n"
        );
        let review = s.app.review(&id, false).unwrap();
        assert!(review.home.is_empty());
        assert_eq!(review.home_applied.unwrap().paths, [".zshrc"]);

        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());
        let e = s.app.undo_home(&id).unwrap_err();
        assert!(e.contains("at work"), "{e}");
        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);

        assert_eq!(s.app.undo_home(&id).unwrap().paths, [".zshrc"]);
        assert_eq!(
            fs::read_to_string(person.path().join(".zshrc")).unwrap(),
            "a\n"
        );
        let review = s.app.review(&id, false).unwrap();
        assert!(review.home[0].startup);
        assert!(review.home_applied.is_none());
        let kule_engine::TextDiff::Lines { hunks, .. } = s.app.home_diff(&id, ".zshrc").unwrap()
        else {
            panic!("no lines");
        };
        assert_eq!(hunks[0].lines.last().unwrap().text, "b");
    }

    /// macOS: while a run goes on, a server of the run that listens on every
    /// address is kept off the network and the window hears it shielded;
    /// with no run, the port is let go.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_run_s_servers_are_shielded_while_it_goes_on() {
        use std::io::{BufRead, BufReader};
        let s = Setup::new();
        let (id, _copy) = s.task();
        let mut server = std::process::Command::new("python3")
            .args([
                "-I",
                "-c",
                "import socket,time\ns=socket.socket()\ns.bind(('0.0.0.0',0))\ns.listen()\nprint(s.getsockname()[1],flush=True)\ntime.sleep(30)",
            ])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(server.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port: u16 = line.trim().parse().unwrap();

        let app = Arc::clone(&s.app);
        std::thread::spawn({
            let app = Arc::clone(&app);
            move || app.shield()
        });
        let wait_for = |want: bool| {
            let until = Instant::now() + Duration::from_secs(5);
            while app.shielded().contains(&port) != want && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
            app.shielded().contains(&port) == want
        };
        app.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());
        let held = wait_for(true);
        app.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        let released = wait_for(false);
        let _ = server.kill();
        let _ = server.wait();
        assert!(held, "the run's server was not shielded");
        assert!(released, "the port was kept after the run");
    }

    /// Going back to a step puts the copy back, is refused while an agent
    /// works in it, and the timeline says which steps can be gone back to.
    #[test]
    fn the_copy_goes_back_to_a_step_when_no_agent_is_at_work() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("a.txt"), "changed\n").unwrap();
        Task::open(&s.app.home, &id).unwrap().step("edit").unwrap();
        let timeline = s.app.timeline(&id).unwrap();
        assert_eq!(timeline.blockers.get(&0), Some(&None));
        assert_eq!(timeline.blockers.get(&1), Some(&None));

        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());
        assert!(s.app.back_to_step(&id, 0).unwrap_err().contains("at work"));
        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);

        let back = s.app.back_to_step(&id, 0).unwrap();
        assert_eq!(back.label, "Back to before the first step");
        assert_eq!(fs::read_to_string(copy.join("a.txt")).unwrap(), "a\n");
        assert_eq!(s.app.timeline(&id).unwrap().steps.len(), 2);
    }

    /// A fork is a task of its own on the same folder, named after the one
    /// it came from, and not made while an agent works in that one.
    #[test]
    fn a_fork_is_a_new_task_named_after_its_source() {
        let s = Setup::new();
        let (id, copy) = s.task();
        s.app.set_label(&id, Some("Report")).unwrap();
        fs::write(copy.join("a.txt"), "changed\n").unwrap();
        Task::open(&s.app.home, &id).unwrap().step("edit").unwrap();

        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());
        assert!(s.app.fork(&id, None).unwrap_err().contains("at work"));
        s.app
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);

        let fork = s.app.fork(&id, None).unwrap();
        assert_eq!(fork.label.as_deref(), Some("Report (fork)"));
        assert_eq!(fork.workspace, s.app.view(&id).unwrap().workspace);
        let at = s.app.fork(&id, Some(0)).unwrap();
        assert_eq!(at.label.as_deref(), Some("Report (fork from the start)"));
        assert_eq!(s.app.tasks().unwrap().len(), 3);
    }

    /// Each tool call's output is kept once, as it was last, at the end: a
    /// command that prints for long does not push the run out of reach.
    #[test]
    fn a_tool_call_s_output_is_kept_once() {
        let s = Setup::new();
        let (id, _) = s.task();
        let output = |call: &str, text: &str| RunEvent::Agent {
            event: kule_acp::Event::ToolOutput {
                id: call.into(),
                text: text.into(),
            },
        };
        for n in 0..(KEPT_EVENTS * 2) {
            s.app.emit(&id, output("a", &format!("line {n}")));
        }
        s.app.emit(&id, output("b", "other"));
        s.app.emit(&id, output("a", "last"));
        let kept = s.app.stream(&id);
        assert_eq!(kept.len(), 2);
        assert!(kept.windows(2).all(|w| w[0].seq < w[1].seq));
        assert_eq!(kept[0].event, output("b", "other"));
        assert_eq!(kept[1].event, output("a", "last"));
    }

    /// What is kept of a task's runs for a window that opens later stays
    /// within what a window takes in one answer, however much the agent
    /// says; the newest is what stays.
    #[test]
    fn what_is_kept_of_runs_keeps_to_a_size() {
        let s = Setup::new();
        let (id, _) = s.task();
        let piece = RunEvent::Agent {
            event: kule_acp::Event::Message {
                text: "x".repeat(1 << 20),
                id: Some("m".into()),
            },
        };
        for _ in 0..100 {
            s.app.emit(&id, piece.clone());
        }
        let last = RunEvent::Agent {
            event: kule_acp::Event::Message {
                text: "the end".into(),
                id: None,
            },
        };
        s.app.emit(&id, last.clone());
        let kept = s.app.stream(&id);
        let size = serde_json::to_vec(&kept).unwrap().len();
        assert!(size <= KEPT_BYTES + (1 << 20), "{size}");
        assert!(size >= KEPT_BYTES / 2, "{size}");
        assert_eq!(kept.last().unwrap().event, last);
    }

    /// The pieces of one message are kept as one; a piece of another
    /// message starts its own, so two answers never run together.
    #[test]
    fn pieces_of_one_message_are_kept_as_one_and_messages_apart() {
        let s = Setup::new();
        let (id, _) = s.task();
        let piece = |text: &str, message: Option<&str>| RunEvent::Agent {
            event: kule_acp::Event::Message {
                text: text.into(),
                id: message.map(str::to_owned),
            },
        };
        for (text, message) in [
            ("continuing once with that ", Some("m1")),
            ("noted", Some("m1")),
            ("API Error: ", Some("m2")),
            ("refused", Some("m2")),
            ("without", None),
            (" an id", None),
        ] {
            s.app.emit(&id, piece(text, message));
        }
        let kept: Vec<RunEvent> = s.app.stream(&id).into_iter().map(|s| s.event).collect();
        assert_eq!(
            kept,
            [
                piece("continuing once with that noted", Some("m1")),
                piece("API Error: refused", Some("m2")),
                piece("without an id", None),
            ]
        );
    }

    /// A place a prompt names is a file the copy holds, reached without a
    /// link; one that is not refuses the run before anything starts.
    #[test]
    fn a_prompt_names_places_in_the_copy_only() {
        let s = Setup::new();
        let (id, copy) = s.task();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.txt"), "s\n").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), copy.join("link.txt"))
            .unwrap();
        let file = |path: &str| kule_view::refs::Reference::File { path: path.into() };
        for (why, refs) in [
            ("nope.txt: ", vec![file("nope.txt")]),
            ("a link", vec![file("link.txt")]),
            ("without ..", vec![file("../secret.txt")]),
            (
                "at most",
                vec![file("a.txt"); kule_view::refs::MAX_REFS + 1],
            ),
        ] {
            let e = s.app.start_run(&id, "claude-code", "hi", refs).unwrap_err();
            assert!(e.contains(why), "{e}");
            assert!(
                !s.app
                    .running
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .contains(&id),
                "{why}"
            );
        }
        assert!(s.app.stream(&id).is_empty());
    }

    #[test]
    fn a_task_that_spent_its_budget_runs_no_agent() {
        let s = Setup::new();
        let (id, _) = s.task();
        let usd = |amount| kule_engine::Money {
            amount,
            currency: "USD".into(),
        };
        let view = s.app.set_budget(&id, Some(usd(1.0))).unwrap();
        assert_eq!(view.budget, Some(usd(1.0)));
        let task = s.app.open(&id).unwrap();
        task.record_usage(&kule_engine::RunUsage {
            agent: "claude-code".into(),
            input_tokens: 500,
            cost: Some(usd(1.0)),
            ..Default::default()
        })
        .unwrap();
        let view = s.app.view(&id).unwrap();
        assert_eq!((view.spent.runs, view.spent.tokens), (1, 500));

        let e = s
            .app
            .start_run(&id, "claude-code", "hi", Vec::new())
            .unwrap_err();
        assert!(e.contains("spent its budget"), "{e}");
        assert!(
            !s.app
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&id)
        );
        assert!(s.app.stream(&id).is_empty());

        assert_eq!(s.app.set_budget(&id, None).unwrap().budget, None);
    }

    #[test]
    fn a_task_that_used_its_time_runs_no_agent() {
        let s = Setup::new();
        let (id, _) = s.task();
        let every = kule_engine::Limits {
            task_minutes: Some(10),
            memory_mb: Some(2048),
            ..Default::default()
        };
        let heard = s.app.subscribe();
        assert_eq!(s.app.set_limits(&every).unwrap(), every);
        assert!(matches!(heard.try_recv(), Ok(Notice::Tasks)));
        let view = s.app.view(&id).unwrap();
        assert_eq!(view.limits, every);
        assert_eq!(view.own_limits, kule_engine::TaskLimits::default());

        let task = s.app.open(&id).unwrap();
        task.record_usage(&kule_engine::RunUsage {
            duration_secs: 600,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(s.app.view(&id).unwrap().time_used_secs, 600);
        let e = s
            .app
            .start_run(&id, "claude-code", "hi", Vec::new())
            .unwrap_err();
        assert!(e.contains("has used its time"), "{e}");
        assert!(
            !s.app
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&id)
        );
        assert!(
            s.app
                .controls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );

        // The task's own limit stands in for every task's.
        let own = kule_engine::TaskLimits {
            task_minutes: Some(Some(20)),
            memory_mb: Some(None),
            ..Default::default()
        };
        let view = s.app.set_task_limits(&id, &own).unwrap();
        assert_eq!(view.own_limits, own);
        assert_eq!(
            (view.limits.task_minutes, view.limits.memory_mb),
            (Some(20), None)
        );
        assert_eq!(
            plan(&task).unwrap().task_left,
            Some((Duration::from_secs(600), Duration::from_secs(1200)))
        );
    }

    #[test]
    fn a_person_stops_the_run_at_work() {
        let s = Setup::new();
        let (id, _) = s.task();
        let e = s.app.stop_run(&id).unwrap_err();
        assert!(e.contains("no agent is at work"), "{e}");
        let control = kule_acp::RunControl::new();
        s.app
            .controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone(), control.clone());
        s.app.stop_run(&id).unwrap();
        assert_eq!(
            control.reason().as_deref(),
            Some("stopped: you stopped the run")
        );
    }

    #[test]
    fn only_the_latest_measure_of_a_run_is_kept() {
        let s = Setup::new();
        let (id, _) = s.task();
        let measured = |memory| RunEvent::Resources {
            measured: kule_acp::governor::Measured {
                memory,
                ..Default::default()
            },
        };
        s.app.emit(&id, measured(1));
        s.app.emit(
            &id,
            RunEvent::Preparing {
                text: "between".into(),
            },
        );
        for n in 2..(KEPT_EVENTS as u64 * 2) {
            s.app.emit(&id, measured(n));
        }
        let kept = s.app.stream(&id);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].event, measured(KEPT_EVENTS as u64 * 2 - 1));
    }

    #[test]
    fn what_every_task_spent_today_counts_against_the_daily_budget() {
        let s = Setup::new();
        let (first, _) = s.task();
        let (second, _) = s.task();
        let usd = |amount| kule_engine::Money {
            amount,
            currency: "USD".into(),
        };
        let daily = s.app.set_daily_budget(Some(usd(2.0))).unwrap();
        assert_eq!(daily.budget, Some(usd(2.0)));
        s.app
            .open(&first)
            .unwrap()
            .record_usage(&kule_engine::RunUsage {
                agent: "claude-code".into(),
                started_unix: unix_now(),
                cost: Some(usd(2.5)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(s.app.daily_budget().unwrap().spent.runs, 1);

        // The second task has no budget of its own and has run nothing.
        let e = s
            .app
            .start_run(&second, "claude-code", "hi", Vec::new())
            .unwrap_err();
        assert!(e.contains("daily budget is spent"), "{e}");
        assert!(
            !s.app
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&second)
        );

        s.app.set_daily_budget(None).unwrap();
        assert_eq!(s.app.daily_budget().unwrap().budget, None);
    }

    #[test]
    fn a_group_s_budget_and_limits_hold_its_tasks_and_a_move_says_so_first() {
        let s = Setup::new();
        let (inside, _) = s.task();
        let (outside, _) = s.task();
        let usd = |amount| kule_engine::Money {
            amount,
            currency: "USD".into(),
        };
        let group = s.app.create_group("clients", None).unwrap();
        s.app.set_task_group(&inside, Some(group.id)).unwrap();
        let kept = s
            .app
            .set_group_budget(group.id, Some(usd(3.0)), Some(usd(1.0)))
            .unwrap();
        assert_eq!(
            (kept.budget, kept.daily_budget),
            (Some(usd(3.0)), Some(usd(1.0)))
        );
        assert!(s.app.set_group_budget(group.id + 1, None, None).is_err());
        let kept = s
            .app
            .set_group_limits(
                group.id,
                &kule_engine::TaskLimits {
                    memory_mb: Some(Some(2048)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(kept.limits.memory_mb, Some(Some(2048)));
        assert_eq!(s.app.groups().unwrap()[0].budget, Some(usd(3.0)));

        // The task in the group runs under the group's limits; the other
        // under every task's.
        assert_eq!(s.app.view(&inside).unwrap().limits.memory_mb, Some(2048));
        assert_eq!(s.app.view(&outside).unwrap().limits.memory_mb, None);
        let budgets = s.app.task_budgets(&inside).unwrap();
        assert_eq!(budgets.len(), 2);
        assert!(s.app.task_budgets(&outside).unwrap().is_empty());

        // Moving the other task in says first what it would be held to.
        let impact = s.app.group_impact(&outside, Some(group.id)).unwrap();
        assert_eq!(
            (impact.limits_now.memory_mb, impact.limits_then.memory_mb),
            (None, Some(2048))
        );
        assert!(impact.budgets_now.is_empty());
        assert_eq!(impact.budgets_then, budgets);

        s.app
            .open(&inside)
            .unwrap()
            .record_usage(&kule_engine::RunUsage {
                agent: "claude-code".into(),
                started_unix: unix_now(),
                cost: Some(usd(1.5)),
                ..Default::default()
            })
            .unwrap();
        let spent = s.app.groups_spent().unwrap();
        assert!((spent[&group.id].today.in_currency("USD") - 1.5).abs() < 1e-9);
        // The budgets tasks share, each with what was spent against it.
        let shared = s.app.shared_budgets().unwrap();
        assert_eq!(
            shared
                .iter()
                .map(|b| (b.scope.clone(), b.budget.amount, b.spent))
                .collect::<Vec<_>>(),
            [
                (
                    kule_engine::BudgetScope::Group {
                        group: group.id,
                        name: "clients".into()
                    },
                    3.0,
                    1.5
                ),
                (
                    kule_engine::BudgetScope::GroupDaily {
                        group: group.id,
                        name: "clients".into()
                    },
                    1.0,
                    1.5
                ),
            ]
        );
        s.app.set_daily_budget(Some(usd(9.0))).unwrap();
        assert_eq!(
            s.app.shared_budgets().unwrap().last().unwrap().scope,
            kule_engine::BudgetScope::Daily
        );
        s.app.set_daily_budget(None).unwrap();
        // Spent for the day: refused, naming the group's budget; the task in
        // no group runs on.
        let e = s
            .app
            .start_run(&inside, "claude-code", "hi", Vec::new())
            .unwrap_err();
        assert!(
            e.contains("group \"clients\"'s daily budget is spent"),
            "{e}"
        );
        assert!(
            s.app
                .open(&outside)
                .unwrap()
                .run_budget()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_run_that_fails_before_its_agent_starts_ends_and_says_why() {
        use kule_engine::GroupPolicy;
        let s = Setup::new();
        let (id, _) = s.task();
        // The group judges by a policy of its own that is gone by the run:
        // the run fails before any agent starts.
        let group = s.app.create_group("strict", None).unwrap();
        s.app
            .set_group_policy(
                group.id,
                GroupPolicy::Own,
                Some(kule_engine::CAREFUL_POLICY),
            )
            .unwrap();
        s.app.set_task_group(&id, Some(group.id)).unwrap();
        fs::remove_file(
            s.app
                .home
                .join(kule_engine::GROUP_POLICY_DIR)
                .join(format!("{}.cedar", group.id)),
        )
        .unwrap();
        s.app
            .start_run(&id, "claude-code", "hi", Vec::new())
            .unwrap();

        // Waits a while, never forever: a run that hangs fails the test.
        let deadline = Instant::now() + Duration::from_secs(20);
        let finished = loop {
            let error = s
                .app
                .stream(&id)
                .into_iter()
                .find_map(|kept| match kept.event {
                    RunEvent::Finished { error, .. } => Some(error),
                    _ => None,
                });
            if let Some(error) = error {
                break error;
            }
            assert!(Instant::now() < deadline, "the run never ended");
            std::thread::sleep(Duration::from_millis(50));
        };
        let error = finished.expect("the run failed");
        assert!(error.contains("cannot be read"), "{error}");
        assert!(!s.app.view(&id).unwrap().running);
        assert!(s.app.stop_run(&id).is_err() || !s.app.busy());
    }

    #[test]
    fn an_unknown_agent_is_refused_before_anything_starts() {
        let s = Setup::new();
        let (id, _) = s.task();
        let e = s
            .app
            .start_run(&id, "nobody", "hi", Vec::new())
            .unwrap_err();
        assert_eq!(e, "no agent nobody; known: claude-code, gemini, codex");
        assert!(
            !s.app
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&id)
        );
    }

    #[test]
    fn a_profile_not_approved_is_refused_and_not_offered() {
        let s = Setup::new();
        let (id, _) = s.task();
        fs::write(
            s.app.home.join(kule_acp::AGENTS_FILE),
            "[[agent]]\nname = \"opencode\"\ncommand = [\"opencode\", \"acp\"]\nstate = [\".local/share/opencode\"]\n",
        )
        .unwrap();
        assert_eq!(s.app.agents().unwrap(), ["claude-code", "gemini", "codex"]);
        let e = s
            .app
            .start_run(&id, "opencode", "hi", Vec::new())
            .unwrap_err();
        assert!(
            e.starts_with("agent opencode cannot run: not approved"),
            "{e}"
        );
        assert!(
            !s.app
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&id)
        );

        kule_acp::Agents::approve(&s.app.home, "opencode").unwrap();
        assert_eq!(
            s.app.agents().unwrap(),
            ["claude-code", "gemini", "codex", "opencode"]
        );
    }

    #[test]
    fn an_approved_commit_remembers_the_files_it_was_told_to() {
        let s = Setup::new();
        fs::write(s.ws.path().join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(s.ws.path().join("package.json"), "{}\n").unwrap();
        let (id, copy) = s.task();
        let change = |n: u32| {
            fs::write(copy.join("Cargo.toml"), format!("[package]\nv = {n}\n")).unwrap();
            fs::write(copy.join("package.json"), format!("{{\"v\": {n}}}\n")).unwrap();
        };
        let verdict = |subject: &str| {
            let review = s.app.review(&id, false).unwrap();
            review
                .ruling
                .judgements
                .into_iter()
                .find(|j| j.subject == subject)
                .unwrap()
                .verdict
        };

        // Not approved: nothing is remembered, nothing is committed.
        change(1);
        let held = s
            .app
            .commit_remembering(&id, false, false, None, &["Cargo.toml".into()])
            .unwrap();
        assert!(matches!(held, Commit::Held { .. }));
        assert!(s.app.rules().unwrap().is_empty());

        let done = s
            .app
            .commit_remembering(&id, true, false, None, &["Cargo.toml".into()])
            .unwrap();
        assert!(matches!(done, Commit::Applied { .. }));
        let rules = s.app.rules().unwrap();
        assert_eq!(rules.len(), 1);

        change(2);
        assert_eq!(verdict("Cargo.toml"), Verdict::Allow);
        assert_eq!(verdict("package.json"), Verdict::Ask);
        assert!(s.app.forget_rule(rules[0].id).unwrap());
        assert_eq!(verdict("Cargo.toml"), Verdict::Ask);
    }

    #[test]
    fn the_app_saves_a_profile_and_approves_only_what_it_showed() {
        let s = Setup::new();
        let mut profile = kule_acp::Profile::bare(
            "cline",
            vec![
                "npx".into(),
                "-y".into(),
                "cline@3.0.73".into(),
                "--acp".into(),
            ],
            Default::default(),
        );
        profile.state = vec![".cline".into()];
        s.app.save_agent(&profile).unwrap();
        let listed = s.app.agent_list().unwrap();
        let cline = listed.iter().find(|l| l.profile.name == "cline").unwrap();
        assert!(cline.held.is_some());
        let shown = cline.digest.clone();

        // Changed after the window showed it: not approved unseen.
        profile.hosts = vec!["api.cline.bot".into()];
        s.app.save_agent(&profile).unwrap();
        assert!(s.app.approve_agent("cline", &shown).is_err());
        assert!(!s.app.agents().unwrap().contains(&"cline".to_owned()));

        let listed = s.app.agent_list().unwrap();
        let shown = listed
            .iter()
            .find(|l| l.profile.name == "cline")
            .unwrap()
            .digest
            .clone();
        s.app.approve_agent("cline", &shown).unwrap();
        assert!(s.app.agents().unwrap().contains(&"cline".to_owned()));
        assert!(s.app.revoke_agent("cline").unwrap());
        assert!(s.app.remove_agent("cline").unwrap());
        assert!(
            s.app
                .agent_list()
                .unwrap()
                .iter()
                .all(|l| l.profile.name != "cline")
        );
    }

    #[test]
    fn ignored_paths_are_counted_apart_and_committed_only_on_request() {
        let s = Setup::new();
        fs::write(s.ws.path().join(".gitignore"), ".env\n").unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(s.ws.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "start"]);
        let (id, copy) = s.task();
        fs::write(copy.join(".env"), "SECRET=1\n").unwrap();

        let view = s.settled().remove(0);
        assert_eq!((view.changes, view.ignored, view.verdict), (0, 1, None));
        assert!(
            s.app
                .review(&id, false)
                .unwrap()
                .ruling
                .judgements
                .is_empty()
        );
        assert_eq!(s.app.review(&id, true).unwrap().ruling.judgements.len(), 1);

        assert!(matches!(
            s.app.commit(&id, false, false, None).unwrap(),
            Commit::Nothing
        ));
        assert!(!s.ws.path().join(".env").exists());
        assert!(matches!(
            s.app.commit(&id, false, true, None).unwrap(),
            Commit::Applied { changes: 1, .. }
        ));
        assert!(s.ws.path().join(".env").exists());
    }

    #[test]
    fn a_question_shows_on_its_task_and_its_answer_opens_the_connection() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};

        let s = Setup::new();
        let (id, _) = s.task();
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let up = upstream.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in upstream.incoming() {
                let _ = stream.unwrap().write_all(b"hello\n");
            }
        });
        let task = Task::open(&s.app.home, &id).unwrap();
        let proxy = Proxy::start_asking(
            &task,
            Policy::builtin(),
            &s.app.approvals,
            task.approval_timeout().unwrap(),
            &[],
        )
        .unwrap();
        let port = proxy.port();
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "CONNECT 127.0.0.1:{up} HTTP/1.1\r\n\r\n").unwrap();
            let mut reply = String::new();
            let _ = stream.read_to_string(&mut reply);
            reply
        });

        let mut waiting = Vec::new();
        for _ in 0..500 {
            waiting = s.app.questions();
            if !waiting.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(waiting.len(), 1);
        assert_eq!(s.app.tasks().unwrap()[0].waiting, 1);
        s.app.answer(waiting[0].id, true).unwrap();
        assert!(client.join().unwrap().contains("hello"));
        assert!(s.app.answer(waiting[0].id, true).is_err());
        assert_eq!(s.app.tasks().unwrap()[0].waiting, 0);
        drop(proxy);

        assert_eq!(
            s.app
                .set_approval_timeout(&id, 5)
                .unwrap()
                .approval_timeout_secs,
            300
        );
        assert!(s.app.set_approval_timeout(&id, 0).is_err());
    }

    /// Something still running in the copy can change it between the
    /// person's review and their click; the commit names what they saw.
    #[test]
    fn a_commit_takes_only_what_was_reviewed() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("b.txt"), "b\n").unwrap();
        let review = s.app.review(&id, false).unwrap();

        fs::write(copy.join("Cargo.toml"), "[package]\n").unwrap();
        let e = s
            .app
            .commit(&id, true, false, Some(&review.digest))
            .unwrap_err();
        assert!(e.contains("review them again"), "{e}");
        assert!(!s.ws.path().join("b.txt").exists());

        let again = s.app.review(&id, false).unwrap();
        assert_ne!(again.digest, review.digest);
        assert!(matches!(
            s.app.commit(&id, true, false, Some(&again.digest)).unwrap(),
            Commit::Applied { changes: 2, .. }
        ));
    }

    /// One task that cannot be read (half made, half removed) does not
    /// blank the list.
    #[test]
    fn an_unreadable_task_does_not_hide_the_others() {
        let s = Setup::new();
        let (good, _) = s.task();
        let broken = s.app.home.join("tasks/broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(
            broken.join("task.json"),
            format!(
                "{{\"id\":\"broken\",\"workspace\":{:?},\"created_unix\":1}}",
                s.ws.path()
            ),
        )
        .unwrap();
        let ids: Vec<String> = s.settled().into_iter().map(|t| t.id).collect();
        assert_eq!(ids, [good]);
    }
}
