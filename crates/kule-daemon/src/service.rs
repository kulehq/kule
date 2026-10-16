//! What the daemon does for its clients: tasks, reviews, commits, agent runs
//! and the questions they wait on. The server only carries calls to it over
//! the socket, so its tests run on real tasks without one.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use kule_engine::{
    Approvals, Checkpoint, Connection, Denial, EffectSet, Error, Policy, Proxy, Question, Ruling,
    Step, Task, Verdict, connections, denials, record_denials,
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
}

/// The task's uncommitted work, ignored paths marked, and what the policy
/// says about the part a commit would take.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub effects: EffectSet,
    pub ruling: Ruling,
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
    Run {
        task: String,
        event: RunEvent,
    },
    /// Every question waiting now, after any change.
    Questions {
        waiting: Vec<Question>,
    },
}

/// How many events of a task's runs are kept for a window that opens later.
const KEPT_EVENTS: usize = 2000;

type Subscribers = Arc<Mutex<Vec<Sender<Notice>>>>;

pub struct Service {
    home: PathBuf,
    /// Tasks with an agent at work: one agent per task at a time.
    running: Mutex<HashSet<String>>,
    /// Questions agents are waiting on, from every task.
    approvals: Approvals,
    subscribers: Subscribers,
    /// The latest events of each task's runs, oldest first.
    streams: Mutex<HashMap<String, VecDeque<RunEvent>>>,
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
        }
    }

    /// Notices from now on, until the receiver is dropped.
    pub fn subscribe(&self) -> Receiver<Notice> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.subscribers.lock().unwrap().push(tx);
        rx
    }

    /// The task's latest run events, oldest first.
    pub fn stream(&self, id: &str) -> Vec<RunEvent> {
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
        {
            let mut streams = self.streams.lock().unwrap();
            let kept = streams.entry(task.to_owned()).or_default();
            if kept.len() == KEPT_EVENTS {
                kept.pop_front();
            }
            kept.push_back(event.clone());
        }
        notify(
            &self.subscribers,
            Notice::Run {
                task: task.to_owned(),
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

    /// Every task, newest first.
    pub fn tasks(&self) -> Result<Vec<TaskView>> {
        let mut infos = Task::list(&self.home).map_err(text)?;
        infos.sort_by(|a, b| (b.created_unix, &b.id).cmp(&(a.created_unix, &a.id)));
        infos.iter().map(|info| self.view(&info.id)).collect()
    }

    pub fn create(&self, workspace: &Path) -> Result<TaskView> {
        let task = Task::create(&self.home, workspace).map_err(text)?;
        self.view(task.id())
    }

    /// The ruling covers the ignored paths only with `include_ignored`.
    pub fn review(&self, id: &str, include_ignored: bool) -> Result<Review> {
        let task = self.open(id)?;
        let effects = task.effects().map_err(text)?;
        let ruling = self
            .policy(&task)?
            .judge(&effects.to_commit(include_ignored));
        Ok(Review { effects, ruling })
    }

    /// Commits what the policy lets through; with `approve`, also what it
    /// puts to a person. Denied changes never pass. Ignored paths stay in
    /// the copy unless `include_ignored`.
    pub fn commit(&self, id: &str, approve: bool, include_ignored: bool) -> Result<Commit> {
        let task = self.idle(id)?;
        let policy = self.policy(&task)?;
        match task.commit_with(include_ignored, policy.gate(approve)) {
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
        let set = self.idle(id)?.rollback().map_err(text)?;
        Ok(set.effects.len())
    }

    /// Reverts the newest commit; returns its checkpoint, if there was one.
    pub fn undo(&self, id: &str) -> Result<Option<u32>> {
        let undone = self.idle(id)?.undo().map_err(text)?;
        Ok(undone.map(|c| c.id))
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

    /// Deletes the task and its copy; the workspace is not touched.
    pub fn remove(&self, id: &str) -> Result<()> {
        self.idle(id)?.remove().map_err(text)?;
        self.streams.lock().unwrap().remove(id);
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
            service.running.lock().unwrap().remove(task.id());
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
        let outcome = runtime.block_on(kule_acp::run(task, launch.command, prompt, tx));
        let _ = runtime.block_on(forward);
        outcome.map_err(|e| {
            format!(
                "{e} (see {})",
                task.copy().with_file_name("agent.log").display()
            )
        })?;
        let refused = launch
            .sandbox
            .denials(started.elapsed())
            .map_err(text)?
            .unwrap_or_default();
        record_denials(task, &refused).map_err(text)?;
        Ok(refused)
    }

    fn view(&self, id: &str) -> Result<TaskView> {
        let task = self.open(id)?;
        let all = task.effects().map_err(text)?;
        let effects = all.to_commit(false);
        let verdict = (!effects.is_empty()).then(|| self.policy(&task).map(|p| p.judge(&effects)));
        Ok(TaskView {
            id: task.id().to_owned(),
            workspace: task.workspace().to_owned(),
            created_unix: task.info().created_unix,
            changes: effects.effects.len(),
            ignored: all.ignored().count(),
            verdict: verdict.transpose()?.map(|r| r.verdict),
            running: self.running.lock().unwrap().contains(id),
            waiting: self
                .approvals
                .waiting()
                .iter()
                .filter(|q| q.task == id)
                .count(),
            approval_timeout_secs: task.approval_timeout().map_err(text)?.as_secs(),
        })
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
    }

    #[test]
    fn tasks_come_with_their_changes_and_verdict() {
        let s = Setup::new();
        let (first, copy) = s.task();
        let (second, _) = s.task();
        fs::write(copy.join("b.txt"), "b\n").unwrap();
        fs::write(copy.join("Cargo.toml"), "[package]\n").unwrap();

        // Both were made within a second, which is all creation times tell.
        let tasks = s.app.tasks().unwrap();
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
            s.app.commit(&id, false, false).unwrap(),
            Commit::Applied {
                checkpoint: 1,
                changes: 1
            }
        ));
        assert!(matches!(
            s.app.commit(&id, false, false).unwrap(),
            Commit::Nothing
        ));

        fs::write(copy.join("Cargo.toml"), "[package]\n").unwrap();
        match s.app.commit(&id, false, false).unwrap() {
            Commit::Held { ruling } => assert_eq!(ruling.verdict, Verdict::Ask),
            other => panic!("expected the change to be held, got {other:?}"),
        }
        assert!(!s.ws.path().join("Cargo.toml").exists());
        let review = s.app.review(&id, false).unwrap();
        assert_eq!(review.ruling.verdict, Verdict::Ask);
        assert_eq!(review.effects.effects.len(), 1);

        assert!(matches!(
            s.app.commit(&id, true, false).unwrap(),
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
        match s.app.commit(&id, true, false).unwrap() {
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

        s.app.commit(&id, false, false).unwrap();
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
        let e = s.app.commit(&id, false, false).unwrap_err();
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
            s.app.commit(&id, true, false).map(|_| ()),
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

        let view = s.app.tasks().unwrap().remove(0);
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
            s.app.commit(&id, false, false).unwrap(),
            Commit::Nothing
        ));
        assert!(!s.ws.path().join(".env").exists());
        assert!(matches!(
            s.app.commit(&id, false, true).unwrap(),
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
}
