//! Questions for a person while a command waits: a connection the policy
//! puts to a person is held until someone answers or the task's time runs
//! out. Only the task waiting is held; other tasks carry on. When nobody
//! answers in time the connection is refused with a reason that tells the
//! agent to skip the step, so the agent never hangs.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How long a task waits for an answer when nobody set it.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// The shortest and longest wait a task can be given; it cannot be turned off.
pub const MIN_TIMEOUT: Duration = Duration::from_secs(60);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(8 * 60 * 60);

/// A question waiting for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub id: u64,
    pub task: String,
    pub host: String,
    pub port: u16,
    /// The `@id` of the rules that put it to a person.
    pub rules: Vec<String>,
    pub asked_unix: u64,
    /// When it is refused unless answered.
    pub deadline_unix: u64,
    /// What Kule does inside the connection once approved; `None` when it
    /// only tunnels it, seeing the host and port alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside: Option<Inside>,
}

/// What Kule does inside a connection it looks into: the hosts a stored
/// secret or an agent's key goes to, and the package registries. An
/// approval never adds a host to these.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inside {
    /// The secrets Kule puts into its requests, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<String>,
    /// A registry Kule lets commands read from, never send to.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// By the person.
    Approved,
    /// By the person.
    Denied,
    /// Nobody answered in time.
    TimedOut,
    /// Refused by Kule: the run ended, and its proxy with it, before anyone
    /// answered.
    Withdrawn,
    /// Refused by Kule without asking: [`MAX_WAITING`] questions of the run
    /// were waiting already.
    TooMany,
}

impl Answer {
    /// Whether a person gave it, rather than the clock or Kule.
    pub fn by_a_person(self) -> bool {
        matches!(self, Answer::Approved | Answer::Denied)
    }
}

/// A question that stopped waiting, or one Kule refused without putting
/// it to anyone: what became of it, told once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settled {
    pub task: String,
    pub host: String,
    pub port: u16,
    /// The question it was, if one was put to the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<u64>,
    pub answer: Answer,
}

/// A connection a person approved that did not go through as approved:
/// Kule cut it, refused it for where it led, or it failed on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AfterApproval {
    pub task: String,
    pub host: String,
    pub port: u16,
    /// What became of it, in Kule's words (see `Connection::told`).
    pub why: String,
}

type Listener = Box<dyn Fn(Vec<Question>) + Send + Sync>;
type SettledListener = Box<dyn Fn(&Settled) + Send + Sync>;
type AfterListener = Box<dyn Fn(&AfterApproval) + Send + Sync>;

/// Where questions wait and answers come from; clones share them.
#[derive(Clone, Default)]
pub struct Approvals {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    answered: Condvar,
    listeners: Mutex<Vec<Listener>>,
    settled: Mutex<Vec<SettledListener>>,
    after: Mutex<Vec<AfterListener>>,
}

#[derive(Default)]
struct State {
    next: u64,
    /// Waiting questions, and the answer once it came; by id.
    waiting: BTreeMap<u64, Waiting>,
    /// Proxies that stopped: what they ask from now on is denied at once.
    withdrawn: HashSet<u64>,
    /// A person's answers, kept for the rest of the proxy's run: one answer
    /// per host and port, not one per connection.
    remembered: HashMap<(u64, String, u16), Answer>,
    /// Hosts and ports a proxy was refused at once, told once each.
    refused_told: HashSet<(u64, String, u16)>,
}

/// The most questions one proxy can have waiting; more are refused at once,
/// so a task cannot bury the person in them.
pub const MAX_WAITING: usize = 16;

struct Waiting {
    question: Question,
    /// Which proxy asked, so stopping it can withdraw its questions.
    source: u64,
    answer: Option<Answer>,
}

impl Approvals {
    pub fn new() -> Approvals {
        Approvals::default()
    }

    /// Calls `listener` with the waiting questions whenever they change.
    pub fn on_change(&self, listener: impl Fn(Vec<Question>) + Send + Sync + 'static) {
        lock(&self.inner.listeners).push(Box::new(listener));
    }

    /// Calls `listener` once for each question that stops waiting, with
    /// what became of it, and for each host and port Kule refuses without
    /// asking (see [`Answer`]); not for an answer kept from earlier.
    pub fn on_settled(&self, listener: impl Fn(&Settled) + Send + Sync + 'static) {
        lock(&self.inner.settled).push(Box::new(listener));
    }

    /// Calls `listener` for each approved connection that did not go
    /// through as approved (see [`AfterApproval`]).
    pub fn on_after_approval(&self, listener: impl Fn(&AfterApproval) + Send + Sync + 'static) {
        lock(&self.inner.after).push(Box::new(listener));
    }

    pub(crate) fn after_approval(&self, after: &AfterApproval) {
        for listener in lock(&self.inner.after).iter() {
            listener(after);
        }
    }

    /// The questions waiting, oldest first.
    pub fn waiting(&self) -> Vec<Question> {
        Self::open(&lock(&self.inner.state))
    }

    /// Answers question `id`; false when it is no longer waiting.
    pub fn answer(&self, id: u64, approve: bool) -> bool {
        let answer = if approve {
            Answer::Approved
        } else {
            Answer::Denied
        };
        self.settle(|w| (w.question.id == id).then_some(answer)) > 0
    }

    /// Asks and waits up to `timeout` for the answer. A person answers once
    /// per host and port in a proxy's run: a question already waiting for
    /// them is shared, and their answer is kept for the rest of the run.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ask(
        &self,
        source: u64,
        task: &str,
        host: &str,
        port: u16,
        rules: &[String],
        inside: Option<Inside>,
        timeout: Duration,
    ) -> Answer {
        let now = unix_now();
        let key = (source, host.to_owned(), port);
        let refused = |state: &mut State, answer: Answer| {
            // Told once per host and port: a task asking again and again
            // does not bury the person in notes either.
            let first = state.refused_told.insert(key.clone());
            (answer, first)
        };
        let (id, owner) = {
            let mut state = lock(&self.inner.state);
            if state.withdrawn.contains(&source) {
                let told = refused(&mut state, Answer::Withdrawn);
                drop(state);
                return self.refused(task, host, port, told);
            }
            if let Some(answer) = state.remembered.get(&key) {
                return *answer;
            }
            let same = state
                .waiting
                .values()
                .find(|w| w.source == source && w.question.host == host && w.question.port == port);
            if let Some(id) = same.map(|w| w.question.id) {
                (id, false)
            } else if state
                .waiting
                .values()
                .filter(|w| w.source == source)
                .count()
                >= MAX_WAITING
            {
                let told = refused(&mut state, Answer::TooMany);
                drop(state);
                return self.refused(task, host, port, told);
            } else {
                state.next += 1;
                let id = state.next;
                let question = Question {
                    id,
                    task: task.to_owned(),
                    host: host.to_owned(),
                    port,
                    rules: rules.to_vec(),
                    asked_unix: now,
                    deadline_unix: now + timeout.as_secs(),
                    inside,
                };
                state.waiting.insert(
                    id,
                    Waiting {
                        question,
                        source,
                        answer: None,
                    },
                );
                (id, true)
            }
        };
        if owner {
            self.changed();
        }
        self.wait(id, &key, timeout, owner)
    }

    /// Waits up to `timeout` for question `id`'s answer. Its `owner` keeps
    /// the answer for `key` and takes the question off the list, under one
    /// lock, so one sharing it finds the answer either way.
    fn wait(&self, id: u64, key: &(u64, String, u16), timeout: Duration, owner: bool) -> Answer {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.inner.state);
        let answer = loop {
            match state.waiting.get(&id) {
                Some(w) if w.answer.is_some() => break w.answer.unwrap_or(Answer::Denied),
                Some(_) => {}
                // Taken off by its owner: answered (and kept) or timed out.
                None => {
                    break state
                        .remembered
                        .get(key)
                        .copied()
                        .unwrap_or(Answer::TimedOut);
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break Answer::TimedOut;
            }
            state = self
                .inner
                .answered
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        if owner {
            // Only a person's answer stands for the rest of the run.
            if answer.by_a_person() {
                state.remembered.insert(key.clone(), answer);
            }
            let question = state.waiting.remove(&id).map(|w| w.question);
            drop(state);
            self.inner.answered.notify_all();
            self.changed();
            if let Some(q) = question {
                self.tell(&Settled {
                    task: q.task,
                    host: q.host,
                    port: q.port,
                    question: Some(q.id),
                    answer,
                });
            }
        }
        answer
    }

    /// `answer`, told to the listeners when `first` (see [`Self::ask`]).
    fn refused(
        &self,
        task: &str,
        host: &str,
        port: u16,
        (answer, first): (Answer, bool),
    ) -> Answer {
        if first {
            self.tell(&Settled {
                task: task.to_owned(),
                host: host.to_owned(),
                port,
                question: None,
                answer,
            });
        }
        answer
    }

    fn tell(&self, settled: &Settled) {
        for listener in lock(&self.inner.settled).iter() {
            listener(settled);
        }
    }

    /// Refuses every question proxy `source` has waiting or asks later, as
    /// [`Answer::Withdrawn`].
    pub(crate) fn withdraw(&self, source: u64) {
        lock(&self.inner.state).withdrawn.insert(source);
        self.settle(|w| (w.source == source).then_some(Answer::Withdrawn));
    }

    /// A number no other proxy using these approvals has.
    pub(crate) fn new_source(&self) -> u64 {
        let mut state = lock(&self.inner.state);
        state.next += 1;
        state.next
    }

    /// Gives an answer to the unanswered questions `pick` chooses; returns
    /// how many it answered.
    fn settle(&self, pick: impl Fn(&Waiting) -> Option<Answer>) -> usize {
        let mut settled = 0;
        {
            let mut state = lock(&self.inner.state);
            for waiting in state.waiting.values_mut() {
                if waiting.answer.is_none()
                    && let Some(answer) = pick(waiting)
                {
                    waiting.answer = Some(answer);
                    settled += 1;
                }
            }
        }
        if settled > 0 {
            self.inner.answered.notify_all();
            self.changed();
        }
        settled
    }

    fn open(state: &State) -> Vec<Question> {
        state
            .waiting
            .values()
            .filter(|w| w.answer.is_none())
            .map(|w| w.question.clone())
            .collect()
    }

    fn changed(&self) {
        let open = Self::open(&lock(&self.inner.state));
        for listener in lock(&self.inner.listeners).iter() {
            listener(open.clone());
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A poisoned lock only means a connection's thread panicked; the questions
/// behind it are still good.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A proxy that stopped asks no one: what it asks after is Kule's
    /// refusal, told once per host and port, never a person's.
    #[test]
    fn what_a_stopped_proxy_asks_is_refused_by_kule() {
        let approvals = Approvals::new();
        let told = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&told);
        approvals.on_settled(move |s| lock(&into).push(s.clone()));
        let source = approvals.new_source();
        approvals.withdraw(source);
        for _ in 0..2 {
            let answer = approvals.ask(
                source,
                "t",
                "example.com",
                443,
                &[],
                None,
                Duration::from_secs(60),
            );
            assert_eq!(answer, Answer::Withdrawn);
            assert!(!answer.by_a_person());
        }
        let told = lock(&told).clone();
        assert_eq!(
            told,
            [Settled {
                task: "t".into(),
                host: "example.com".into(),
                port: 443,
                question: None,
                answer: Answer::Withdrawn,
            }]
        );
        assert!(approvals.waiting().is_empty());
    }
}
