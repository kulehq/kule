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
    Proxy, Question, Ruling, Step, Task, Touched, Verdict, changed_entities, connections, denials,
    meet, record_denials,
};
use serde::{Deserialize, Serialize};

/// Errors reach clients as text.
pub type Result<T> = std::result::Result<T, String>;

fn text(e: Error) -> String {
    match e {
        Error::Conflict(paths) => format!(
            "the workspace changed outside Kule at {}; roll back, or start a new task on \
             the current workspace",
            paths.join(", ")
        ),
        e => e.to_string(),
    }
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Commit {
    Applied {
        checkpoint: u32,
        changes: usize,
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
}

/// What a run reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    /// What the person asked; the first event of a run.
    Prompt {
        text: String,
        agent: String,
    },
    Agent {
        event: kule_acp::Event,
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
}

/// How many events of a task's runs are kept for a window that opens later.
const KEPT_EVENTS: usize = 2000;

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
}

impl Service {
    pub fn new(home: PathBuf) -> Service {
        let approvals = Approvals::new();
        let subscribers = Subscribers::default();
        let heard = Arc::clone(&subscribers);
        approvals.on_change(move |waiting| notify(&heard, Notice::Questions { waiting }));
        Service {
            home,
            running: Mutex::new(HashSet::new()),
            approvals,
            subscribers,
            streams: Mutex::new(HashMap::new()),
            next_seq: std::sync::atomic::AtomicU64::new(1),
            scans: Mutex::new(HashMap::new()),
            outlined: Mutex::new(HashMap::new()),
        }
    }

    /// Notices from now on, until the receiver is dropped.
    pub fn subscribe(&self) -> Receiver<Notice> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.subscribers.lock().unwrap().push(tx);
        rx
    }

    /// The task's latest run events, oldest first.
    pub fn stream(&self, id: &str) -> Vec<Sequenced> {
        self.streams
            .lock()
            .unwrap()
            .get(id)
            .map(|events| events.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether anything is going on: an agent at work or a question waiting.
    pub fn busy(&self) -> bool {
        !self.running.lock().unwrap().is_empty() || !self.approvals.waiting().is_empty()
    }

    fn emit(&self, task: &str, event: RunEvent) {
        // Under the lock: the order of `seq` is the order subscribers hear.
        let mut streams = self.streams.lock().unwrap();
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let kept = streams.entry(task.to_owned()).or_default();
        // An answer arrives in many small chunks; kept as one, the prompt
        // stays in reach of a window that opens later.
        let merged = match (kept.back_mut(), &event) {
            (
                Some(Sequenced {
                    seq: last,
                    event:
                        RunEvent::Agent {
                            event: kule_acp::Event::Message { text: kept_text },
                        },
                }),
                RunEvent::Agent {
                    event: kule_acp::Event::Message { text },
                },
            ) => {
                kept_text.push_str(text);
                *last = seq;
                true
            }
            _ => false,
        };
        if !merged {
            if kept.len() == KEPT_EVENTS {
                kept.pop_front();
            }
            kept.push_back(Sequenced {
                seq,
                event: event.clone(),
            });
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

    /// Sets how long the task's questions wait, in minutes.
    pub fn set_approval_timeout(&self, id: &str, minutes: u64) -> Result<TaskView> {
        let task = self.open(id)?;
        task.set_approval_timeout(Duration::from_secs(minutes.saturating_mul(60)))
            .map_err(text)?;
        self.view(id)
    }

    pub fn agents(&self) -> Vec<&'static str> {
        kule_acp::PROFILES.iter().map(|p| p.name).collect()
    }

    /// Every task, newest first, from each one's last scan; scans older
    /// than [`FRESH`] are made again in the background.
    pub fn tasks(self: &Arc<Self>) -> Result<Vec<TaskView>> {
        let mut infos = Task::list(&self.home).map_err(text)?;
        infos.sort_by(|a, b| (b.created_unix, &b.id).cmp(&(a.created_unix, &a.id)));
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
        let mut scans = self.scans.lock().unwrap();
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
                    .unwrap()
                    .get(&id)
                    .map(|c| (c.scan.clone(), c.failed));
                let _ = service.rescan(&id);
                let after = service
                    .scans
                    .lock()
                    .unwrap()
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
        let mut scans = self.scans.lock().unwrap();
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
        })
    }

    pub fn create(&self, workspace: &Path) -> Result<TaskView> {
        let task = Task::create(&self.home, workspace).map_err(text)?;
        self.view(task.id())
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
        })
    }

    /// Commits what the policy lets through; with `approve`, also what it
    /// puts to a person. Denied changes never pass. Ignored paths stay in
    /// the copy unless `include_ignored`. With `reviewed`, the digest of a
    /// [`Review`], it commits only if that is still exactly what it takes.
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
            Ok(Some(checkpoint)) => Ok(Commit::Applied {
                checkpoint: checkpoint.id,
                changes: checkpoint.effects.len(),
            }),
            Ok(None) => Ok(Commit::Nothing),
            Err(Error::Held(ruling)) => Ok(Commit::Held { ruling }),
            Err(e) => Err(text(e)),
        }
    }

    /// Throws the uncommitted work away; returns how many changes it held.
    pub fn rollback(&self, id: &str) -> Result<usize> {
        let set = self.idle(id)?.rollback();
        let _ = self.rescan(id);
        Ok(set.map_err(text)?.effects.len())
    }

    /// Reverts the newest commit; returns its checkpoint, if there was one.
    pub fn undo(&self, id: &str) -> Result<Option<u32>> {
        let undone = self.idle(id)?.undo();
        let _ = self.rescan(id);
        Ok(undone.map_err(text)?.map(|c| c.id))
    }

    pub fn timeline(&self, id: &str) -> Result<Timeline> {
        let task = self.open(id)?;
        Ok(Timeline {
            steps: task.steps().map_err(text)?,
            checkpoints: task.checkpoints().map_err(text)?,
            connections: connections(&task).map_err(text)?,
            denials: denials(&task).map_err(text)?,
        })
    }

    /// Where the task's copy is, for the person's own terminal.
    pub fn copy_of(&self, id: &str) -> Result<PathBuf> {
        Ok(self.open(id)?.copy())
    }

    /// The copy, for the person's own terminal (see `Task::terminal_dir`).
    pub fn terminal_dir(&self, id: &str) -> Result<PathBuf> {
        self.open(id)?.terminal_dir().map_err(text)
    }

    /// Deletes the task and its copy; the workspace is not touched.
    pub fn remove(&self, id: &str) -> Result<()> {
        self.idle(id)?.remove().map_err(text)?;
        self.streams.lock().unwrap().remove(id);
        self.scans.lock().unwrap().remove(id);
        self.outlined.lock().unwrap().remove(id);
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
    /// last, also when the agent could not start.
    pub fn start_run(self: &Arc<Self>, id: &str, agent: &str, prompt: &str) -> Result<()> {
        let profile = kule_acp::profile(agent).ok_or_else(|| format!("no agent {agent}"))?;
        let task = self.open(id)?;
        if !self.running.lock().unwrap().insert(id.to_owned()) {
            return Err(format!("an agent is already at work in task {id}"));
        }
        // Marked in the task too, so the CLI and any other process see it.
        let at_work = match task.agent_at_work() {
            Ok(at_work) => at_work,
            Err(e) => {
                self.running.lock().unwrap().remove(id);
                return Err(text(e));
            }
        };
        self.emit(
            id,
            RunEvent::Prompt {
                text: prompt.to_owned(),
                agent: agent.to_owned(),
            },
        );
        let (service, prompt) = (Arc::clone(self), prompt.to_owned());
        std::thread::spawn(move || {
            let (error, denials) = match service.run(&task, profile, &prompt) {
                Ok(denials) => (None, denials),
                Err(e) => (Some(e), Vec::new()),
            };
            drop(at_work);
            service.running.lock().unwrap().remove(task.id());
            // Before the window hears the run is over and asks again.
            let _ = service.rescan(task.id());
            service.emit(task.id(), RunEvent::Finished { error, denials });
        });
        Ok(())
    }

    /// One run, start to end; returns what the sandbox refused.
    fn run(
        self: &Arc<Self>,
        task: &Task,
        profile: &'static kule_acp::Profile,
        prompt: &str,
    ) -> Result<Vec<Denial>> {
        let started = Instant::now();
        // Serves this run only, with the policy as it is now; dropped when
        // the run returns, which closes it.
        let policy = profile.open_hosts(self.policy(task)?).map_err(text)?;
        let timeout = task.approval_timeout().map_err(text)?;
        let proxy = Proxy::start_asking(task, policy, &self.approvals, timeout).map_err(text)?;
        let launch = profile.launch(task, &proxy).map_err(text)?;
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let (service, id) = (Arc::clone(self), task.id().to_owned());
        let forward = runtime.spawn(async move {
            while let Some(event) = rx.next().await {
                service.emit(&id, RunEvent::Agent { event });
            }
        });
        let outcome =
            runtime.block_on(kule_acp::run(task, launch.command, prompt, launch.mode, tx));
        let _ = runtime.block_on(forward);
        // The agent's process group is gone by now.
        let put_back = launch.kept.restore().map_err(text)?;
        outcome.map_err(|e| {
            format!(
                "{e} (see {})",
                task.copy().with_file_name("agent.log").display()
            )
        })?;
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
            running: self.running.lock().unwrap().contains(id),
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
        })
    }

    /// Where the task's work meets that of the other tasks on its
    /// workspace, as their last scans have it.
    fn overlaps(&self, id: &str, scan: &Scan) -> Vec<Overlap> {
        let scans = self.scans.lock().unwrap();
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
            .unwrap()
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
            .unwrap()
            .insert(task.id().to_owned(), known);
        touched
    }

    fn open(&self, id: &str) -> Result<Task> {
        Task::open(&self.home, id).map_err(text)
    }

    /// The task, refused while an agent works in it: committing or rolling
    /// back under a running agent would split its work in two.
    fn idle(&self, id: &str) -> Result<Task> {
        if self.running.lock().unwrap().contains(id) {
            return Err(format!(
                "an agent is at work in task {id}; wait until it is done"
            ));
        }
        self.open(id)
    }

    /// The user's policy, or the built-in one, plus the workspace's forbids.
    fn policy(&self, task: &Task) -> Result<Policy> {
        Policy::load(Some(&self.home.join("policy.cedar")), task.workspace()).map_err(text)
    }
}

/// Sends `notice` to every subscriber, forgetting the ones that left.
fn notify(subscribers: &Subscribers, notice: Notice) {
    subscribers
        .lock()
        .unwrap()
        .retain(|tx| tx.send(notice.clone()).is_ok());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use kule_engine::CheckpointState;
    use tempfile::TempDir;

    use super::*;

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
            for cached in self.app.scans.lock().unwrap().values_mut() {
                cached.at = cached.at.checked_sub(FRESH).unwrap();
            }
        }

        /// The task list once the scans it starts are done, as after a
        /// change made outside the daemon (a terminal in the copy).
        fn settled(&self) -> Vec<TaskView> {
            self.age_scans();
            self.app.tasks().unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.app.scans.lock().unwrap().values().any(|c| c.scanning) {
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
        s.app.scans.lock().unwrap().clear();
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
    fn commit_follows_the_policy() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("a.txt"), "edited\n").unwrap();
        assert!(matches!(
            s.app.commit(&id, false, false, None).unwrap(),
            Commit::Applied {
                checkpoint: 1,
                changes: 1
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
        assert!(e.contains("changed outside Kule at a.txt"), "{e}");
    }

    #[test]
    fn a_task_with_an_agent_at_work_refuses_changes() {
        let s = Setup::new();
        let (id, copy) = s.task();
        fs::write(copy.join("b.txt"), "b\n").unwrap();
        s.app.running.lock().unwrap().insert(id.clone());

        assert!(s.app.tasks().unwrap()[0].running);
        for refused in [
            s.app.commit(&id, true, false, None).map(|_| ()),
            s.app.rollback(&id).map(|_| ()),
            s.app.undo(&id).map(|_| ()),
            s.app.remove(&id),
            s.app.start_run(&id, "claude-code", "hi"),
        ] {
            assert!(refused.unwrap_err().contains("at work"));
        }
        assert!(copy.join("b.txt").exists());

        s.app.running.lock().unwrap().remove(&id);
        s.app.remove(&id).unwrap();
        assert!(s.app.tasks().unwrap().is_empty());
    }

    #[test]
    fn an_unknown_agent_is_refused_before_anything_starts() {
        let s = Setup::new();
        let (id, _) = s.task();
        let e = s.app.start_run(&id, "nobody", "hi").unwrap_err();
        assert_eq!(e, "no agent nobody");
        assert!(!s.app.running.lock().unwrap().contains(&id));
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
