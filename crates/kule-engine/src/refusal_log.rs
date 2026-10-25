//! What a sandbox refused, as the system log tells it (macOS).
//!
//! The kernel reports each refusal to the system log as a message the log
//! may drop: when the log falls behind (any part of the kernel logging a
//! lot, many refusals at once, many readers of its stream) it loses the
//! kernel's messages for a moment, and notes each loss in its store, with
//! when, but not whose messages they were. What
//! `log show` reads back from the store comes late and out of order; the
//! live stream gets each refusal in order, but never the notes of a loss,
//! and a reader of the stream held up for a moment, as a busy Mac holds
//! one up, is given its backlog only up to a cap: the rest the log drops
//! with no word, and still delivers what comes after. So Kule reads both:
//!
//! - one stream per process, on every Kule sandbox's refusals, shown to
//!   listen by a refusal of Kule's own before any sandbox's first command;
//! - once a sandbox's commands are done, one more refusal of Kule's own,
//!   waited for in the store, which shows what it kept before that refusal
//!   and notes a loss before what came after it.
//!
//! The list is what the stream brought, and it is all the log kept only
//! when the store showed that refusal and shows nothing refused that the
//! stream did not bring; otherwise what the store shows is added to it
//! ([`stored`]) and it is told as perhaps not all. It is all there was
//! only where the store also notes no loss of the kernel's messages since
//! Kule began to listen for the sandbox. On a Mac that loses them every
//! few seconds, so a loss is told as a note of its own, when it was
//! ([`Refusals::lost`]), not as a list Kule could not read
//! ([`Refusals::incomplete`]).

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::sandbox::{Denial, Refusals, parse_denial};

/// How long Kule waits for a refusal of its own to show in the store, or
/// on the stream when it opens.
const WAIT: Duration = Duration::from_secs(10);

/// How long Kule waits for the stream's first refusal before it makes it
/// again: a stream just started may not listen yet.
const READY_EVERY: Duration = Duration::from_millis(100);

/// How long after a stream that could not be shown to listen Kule tries
/// again, so a log that does not stream does not hold up every command.
const RETRY: Duration = Duration::from_secs(60);

/// How many different refusals one sandbox keeps. Past that the list is
/// told as incomplete.
const MOST: usize = 10_000;

/// What every Kule sandbox's tag starts with.
const KULE: &str = "kule-";

/// What the words of the stream's first refusal start with.
const LISTENING: &str = "kule-listening-";

/// Runs what it is given (the stream), and holds the shell's input: once
/// that closes, when Kule's process ends however it ends, the shell ends
/// what it runs, so no stream outlives Kule.
const SHELL: &str = "exec 3>&1 >/dev/null; \"$@\" >&3 3>&- & exec 3>&-; read -r _; kill $!";

/// The process's stream, or when one could not be shown to listen.
static STREAM: Mutex<Option<Result<Arc<Stream>, Instant>>> = Mutex::new(None);

/// The stream of every Kule sandbox's refusals, for the sandboxes of this
/// process.
struct Stream {
    /// The shell holding the stream, and its input: kept open while the
    /// stream is in use.
    shell: Mutex<(Child, ChildStdin)>,
    state: Mutex<State>,
    came: Condvar,
    store: Mutex<Store>,
    stored: Condvar,
}

#[derive(Default)]
struct State {
    /// By tag, what came for the sandboxes being listened for.
    sandboxes: HashMap<String, Seen>,
    /// The stream's first refusals that came, by their words.
    listening: HashSet<String>,
    /// The stream ended.
    ended: bool,
}

/// What the stream brought for one sandbox.
#[derive(Debug, Default)]
struct Seen {
    denials: Vec<Denial>,
    known: HashSet<(String, String, String)>,
    /// Something came that Kule cannot read, or more came than are kept:
    /// the list may not be all.
    short: bool,
}

impl Seen {
    /// Adds a refusal, once however often it came; past [`MOST`] of them
    /// the list is short.
    fn add(&mut self, denial: Denial) {
        if self.knows(&denial) {
            return;
        }
        if self.known.len() >= MOST {
            self.short = true;
            return;
        }
        self.known.insert(key(&denial));
        self.denials.push(denial);
    }

    /// Whether the refusal came already.
    fn knows(&self, denial: &Denial) -> bool {
        self.known.contains(&key(denial))
    }
}

/// What makes two reports one refusal.
fn key(denial: &Denial) -> (String, String, String) {
    (
        denial.process.clone(),
        denial.operation.clone(),
        denial.path.clone(),
    )
}

/// What one message is to the sandboxes being listened for.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    Refused(String, Denial),
    /// A refusal of Kule's own under a sandbox's tag.
    Own(String, String),
    /// The stream's first refusal.
    Listening(String),
    /// A sandbox's, but not a refusal Kule can read.
    Unreadable(String),
    /// None of theirs: another process's sandbox, or a refused path that
    /// only names a tag.
    Other,
}

/// Listening for one sandbox's refusals.
pub(crate) struct Watch {
    stream: Option<Arc<Stream>>,
    tag: String,
    /// When Kule began to listen for it, by the clock the log notes losses
    /// by.
    since: SystemTime,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch")
            .field("tag", &self.tag)
            .field("listening", &self.stream.is_some())
            .finish_non_exhaustive()
    }
}

impl Watch {
    /// Listens for the refusals of the sandbox tagged `tag`, on the
    /// process's stream, opened first where none listens yet. Without one
    /// what Kule reads of it is never told as all there was.
    pub(crate) fn start(tag: &str) -> Watch {
        let since = SystemTime::now();
        let stream = stream();
        if let Some(stream) = &stream {
            stream
                .lock()
                .sandboxes
                .insert(tag.to_owned(), Seen::default());
        }
        Watch {
            stream,
            tag: tag.to_owned(),
            since,
        }
    }

    /// What the stream brought refused: all the log kept once the store
    /// shows a refusal Kule makes now, after the commands', and nothing
    /// refused that the stream did not bring, as the log drops part of
    /// what it owes a stream held up for a moment, with no word. The
    /// losses of the kernel's messages the store notes meanwhile come with
    /// it, as a refusal may be among them.
    pub(crate) fn refusals(&self) -> Refusals {
        let Some(stream) = &self.stream else {
            return told(false, None);
        };
        let shown = own_word(&self.tag, "flush")
            .filter(|word| refuse(word))
            .and_then(|word| stream.shown_until(self.since, &self.tag, &word, WAIT));
        let state = stream.lock();
        let seen = state.sandboxes.get(&self.tag);
        let all = match (seen, &shown) {
            (Some(seen), Some(shown)) => shown.refused.iter().all(|d| seen.knows(d)),
            _ => false,
        };
        let mut refusals = told(all, seen);
        refusals.lost = shown.map(|s| s.lost).unwrap_or_default();
        refusals
    }
}

/// What a sandbox is told was refused: what came for it (`seen`), as all
/// there was only where `all` holds and nothing came that Kule could not
/// read.
fn told(all: bool, seen: Option<&Seen>) -> Refusals {
    Refusals {
        denials: seen.map(|s| s.denials.clone()).unwrap_or_default(),
        incomplete: !all || seen.is_none_or(|s| s.short),
        lost: Vec::new(),
    }
}

/// The most descriptors [`held`] closes in the shell it starts.
const CLOSED_UP_TO: libc::c_int = 1 << 16;

/// `program` with `args`, under the shell that ends it when its input
/// closes; its output piped, in a group of its own. The shell holds
/// nothing else Kule's process has open: it lasts as long as the process,
/// and a terminal or pipe of Kule's that it held would never end.
fn held(program: &str, args: &[&str]) -> std::io::Result<(Child, ChildStdin, ChildStdout)> {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", SHELL, "sh", program])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    // SAFETY: between fork and exec, only getdtablesize and close, both
    // async-signal-safe system calls.
    unsafe {
        command.pre_exec(|| {
            for fd in 3..libc::getdtablesize().min(CLOSED_UP_TO) {
                libc::close(fd);
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    match (child.stdin.take(), child.stdout.take()) {
        (Some(input), Some(output)) => Ok((child, input, output)),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            Err(std::io::Error::other("the shell's pipes"))
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(stream) = &self.stream {
            stream.lock().sandboxes.remove(&self.tag);
        }
    }
}

/// The process's stream, opened where none listens.
fn stream() -> Option<Arc<Stream>> {
    let mut current = STREAM.lock().unwrap_or_else(|p| p.into_inner());
    match &*current {
        Some(Ok(stream)) if !stream.lock().ended => return Some(Arc::clone(stream)),
        Some(Err(failed)) if failed.elapsed() < RETRY => return None,
        _ => {}
    }
    let predicate = format!("sender == \"Sandbox\" AND eventMessage CONTAINS \"{KULE}\"");
    let opened = Stream::open(
        "/usr/bin/log",
        &["stream", "--style", "ndjson", "--predicate", &predicate],
    );
    *current = Some(opened.clone().ok_or_else(Instant::now));
    opened
}

impl Stream {
    /// Starts the stream, `program` with `args`, and the thread reading it,
    /// and waits until the stream shows a refusal Kule makes.
    fn open(program: &str, args: &[&str]) -> Option<Arc<Stream>> {
        let (child, input, output) = held(program, args).ok()?;
        let stream = Arc::new(Stream {
            shell: Mutex::new((child, input)),
            state: Mutex::new(State::default()),
            came: Condvar::new(),
            store: Mutex::new(Store::default()),
            stored: Condvar::new(),
        });
        // Not holding it: a stream no one uses any more is dropped, which
        // ends what this reads.
        let reading = Arc::downgrade(&stream);
        std::thread::spawn(move || {
            let mut lines = BufReader::new(output).lines();
            let mut first = true;
            while let Some(Ok(line)) = lines.next() {
                let Some(stream) = reading.upgrade() else {
                    return;
                };
                // What the stream says before its events.
                if std::mem::take(&mut first) && line.starts_with("Filtering the log data") {
                    continue;
                }
                take(&mut stream.lock(), &line);
                stream.came.notify_all();
            }
            if let Some(stream) = reading.upgrade() {
                stream.lock().ended = true;
                stream.came.notify_all();
            }
        });
        stream.listens().then_some(stream)
    }

    fn listens(&self) -> bool {
        let Some(word) = own_word("kule", "listening") else {
            return false;
        };
        let started = Instant::now();
        while started.elapsed() < WAIT {
            if !refuse(&word) {
                return false;
            }
            if self.wait(READY_EVERY, |state| state.listening.contains(&word)) {
                return true;
            }
            if self.lock().ended {
                return false;
            }
        }
        false
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether `came` holds within `within`; never once the stream ended.
    fn wait(&self, within: Duration, came: impl Fn(&State) -> bool) -> bool {
        let deadline = Instant::now() + within;
        let mut state = self.lock();
        loop {
            if came(&state) {
                return true;
            }
            let now = Instant::now();
            if state.ended || now >= deadline {
                return false;
            }
            state = match self.came.wait_timeout(state, deadline - now) {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }
}

impl Drop for Stream {
    /// A stream that never listened, or ended: its shell and what is left
    /// of the stream go with it.
    fn drop(&mut self) {
        let (child, _) = match self.shell.get_mut() {
            Ok(shell) => shell,
            Err(poisoned) => poisoned.into_inner(),
        };
        // The shell and the stream are the group it leads.
        // SAFETY: kill only sends a signal.
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
        let _ = child.wait();
    }
}

/// Adds what one line of the stream says to `state`.
fn take(state: &mut State, line: &str) {
    let message = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .filter(|entry| entry["eventType"] == "logEvent")
        .and_then(|entry| entry["eventMessage"].as_str().map(str::to_owned));
    // Anything else the stream says: what it brought may not be all.
    let Some(message) = message else {
        for seen in state.sandboxes.values_mut() {
            seen.short = true;
        }
        return;
    };
    match read_line(&message, |tag| state.sandboxes.contains_key(tag)) {
        Line::Refused(tag, denial) => {
            if let Some(seen) = state.sandboxes.get_mut(&tag) {
                seen.add(denial);
            }
        }
        // Kule's refusal after a sandbox's commands is waited for in the
        // store, which the stream says nothing about.
        Line::Own(..) => {}
        Line::Listening(word) => {
            state.listening.insert(word);
        }
        Line::Unreadable(tag) => {
            if let Some(seen) = state.sandboxes.get_mut(&tag) {
                seen.short = true;
            }
        }
        Line::Other => {}
    }
}

/// The kernel writes a refusal as `Sandbox: bash(1) deny(1) op path` and
/// the sandbox's message on the line after: its tag, or the words of a
/// refusal of Kule's own, `<tag>-<what>-<hex>`. `listened` says which tags
/// are listened for.
fn read_line(message: &str, listened: impl Fn(&str) -> bool) -> Line {
    let Some((_, last)) = message.rsplit_once('\n') else {
        return Line::Other;
    };
    if listened(last) {
        return parse_denial(message).map_or_else(
            || Line::Unreadable(last.to_owned()),
            |denial| Line::Refused(last.to_owned(), denial),
        );
    }
    if last.starts_with(LISTENING) {
        return Line::Listening(last.to_owned());
    }
    let mut parts = last.rsplitn(3, '-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(_), Some(tag)) if listened(tag) => {
            Line::Own(tag.to_owned(), last.to_owned())
        }
        _ => Line::Other,
    }
}

/// How often the store is read while a refusal of Kule's own is waited
/// for there. One read at a time per process: each takes the log most of a
/// second, and many at once make it fall behind and lose messages.
const STORE_EVERY: Duration = Duration::from_millis(250);

/// How long before the earliest `from` waited for the store is read: a
/// loss is noted by when it began, a moment before it reached `from`.
const LOSS_BEFORE: f64 = 5.0;

/// Kule's own refusals waited for in the log's store, by their words.
#[derive(Default)]
struct Store {
    waiting: HashMap<String, Waiting>,
    /// A thread reads the store for them.
    reading: bool,
}

/// One refusal of Kule's own waited for in the store.
struct Waiting {
    /// Since when a loss counts for it (seconds since the epoch).
    from: f64,
    /// The sandbox it was made for.
    tag: String,
    /// What the store said, once it showed the refusal.
    said: Option<Shown>,
}

/// What the store showed for a sandbox once it showed Kule's refusal after
/// its commands.
#[derive(Debug, Default, PartialEq)]
struct Shown {
    /// When it lost the kernel's messages since Kule began to listen.
    lost: Vec<String>,
    /// What it shows refused under the sandbox's tag.
    refused: Vec<Denial>,
}

/// One loss of the kernel's messages the store notes: when it began and
/// ended (seconds since the epoch; `None` where Kule cannot place it), and
/// the time of day it began, as the person is told it.
#[derive(Debug, PartialEq)]
struct Loss {
    began: Option<f64>,
    ended: Option<f64>,
    at: String,
}

/// What one read of the store said.
#[derive(Debug, Default, PartialEq)]
struct StoreRead {
    /// The words of Kule's own refusals it showed, with when each was made
    /// (`None` where Kule cannot place it).
    shown: HashMap<String, Option<f64>>,
    losses: Vec<Loss>,
    /// What it shows refused, by the tag of the sandbox listened for.
    refused: Vec<(String, Denial)>,
}

impl StoreRead {
    /// What the store showed for the sandbox tagged `tag`, once it showed
    /// Kule's refusal carrying `word` for it: the losses from `from` until
    /// that refusal, and what it shows refused under the tag.
    fn said(&self, from: f64, tag: &str, word: &str) -> Option<Shown> {
        let made = self.shown.get(word)?;
        Some(Shown {
            lost: self.lost_between(from, *made),
            refused: self
                .refused
                .iter()
                .filter(|(t, _)| t == tag)
                .map(|(_, denial)| denial.clone())
                .collect(),
        })
    }

    /// When the losses it notes between `from` and `until` began, each time
    /// of day once: a loss that ended before `from` or began after `until`
    /// is none of theirs, and one Kule cannot place is taken as theirs.
    fn lost_between(&self, from: f64, until: Option<f64>) -> Vec<String> {
        let mut at: Vec<String> = Vec::new();
        for loss in &self.losses {
            let after = loss.ended.is_none_or(|ended| ended >= from);
            let before = loss
                .began
                .zip(until)
                .is_none_or(|(began, until)| began <= until);
            if after && before && !at.contains(&loss.at) {
                at.push(loss.at.clone());
            }
        }
        at
    }
}

impl Stream {
    /// What the log's store shows for the sandbox tagged `tag` once it
    /// shows the refusal carrying `word`: what it kept refused under the
    /// tag, and when it lost the kernel's messages from `since` until that
    /// refusal, as it notes a loss before what came after it. `None` where
    /// it does not show the refusal within `within`.
    fn shown_until(
        self: &Arc<Self>,
        since: SystemTime,
        tag: &str,
        word: &str,
        within: Duration,
    ) -> Option<Shown> {
        // A clock read a moment early or late is no gap.
        let from = unix(since) - 1.0;
        let deadline = Instant::now() + within;
        let mut store = self.store();
        store.waiting.insert(
            word.to_owned(),
            Waiting {
                from,
                tag: tag.to_owned(),
                said: None,
            },
        );
        if !store.reading {
            store.reading = true;
            let reading = Arc::clone(self);
            std::thread::spawn(move || reading.read_store());
        }
        let shown = loop {
            if let Some(shown) = store.waiting.get_mut(word).and_then(|w| w.said.take()) {
                break Some(shown);
            }
            let now = Instant::now();
            if now >= deadline {
                break None;
            }
            store = match self.stored.wait_timeout(store, deadline - now) {
                Ok((store, _)) => store,
                Err(poisoned) => poisoned.into_inner().0,
            };
        };
        store.waiting.remove(word);
        shown
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Reads the store while refusals are waited for there, and tells each
    /// what it said once it shows it.
    fn read_store(&self) {
        let predicate = "(eventType == lossEvent AND processID == 0) OR \
             (sender == \"Sandbox\" AND eventMessage CONTAINS \"kule-\")";
        loop {
            let (earliest, tags) = {
                let mut store = self.store();
                let open = store.waiting.values().filter(|w| w.said.is_none());
                let tags: HashSet<String> = open.clone().map(|w| w.tag.clone()).collect();
                match open.map(|w| w.from).reduce(f64::min) {
                    Some(earliest) => (earliest, tags),
                    None => {
                        store.reading = false;
                        return;
                    }
                }
            };
            let start = format!("@{}", (earliest - LOSS_BEFORE).max(0.0) as u64);
            let out = Command::new("/usr/bin/log")
                .args(["show", "--start", &start, "--loss", "--style", "ndjson"])
                .args(["--predicate", predicate])
                .stdin(Stdio::null())
                .output();
            if let Ok(out) = out {
                let read = store_read(&String::from_utf8_lossy(&out.stdout), |tag| {
                    tags.contains(tag)
                });
                let mut store = self.store();
                for (word, waiting) in &mut store.waiting {
                    if waiting.said.is_none() {
                        waiting.said = read.said(waiting.from, &waiting.tag, word);
                    }
                }
                self.stored.notify_all();
            }
            std::thread::sleep(STORE_EVERY);
        }
    }
}

/// What `ndjson`, what `log show` printed, says: Kule's own refusals it
/// shows, what it shows refused for the sandboxes `listened` says are
/// listened for, and the losses of the kernel's messages it notes.
fn store_read(ndjson: &str, listened: impl Fn(&str) -> bool) -> StoreRead {
    let mut read = StoreRead::default();
    for line in ndjson.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match entry["eventType"].as_str() {
            Some("lossEvent") => {
                let began = entry["lossStartTimestamp"].as_str().unwrap_or("");
                read.losses.push(Loss {
                    began: timestamp(began),
                    ended: entry["lossEndTimestamp"].as_str().and_then(timestamp),
                    // The time of day of `2026-10-24 16:04:20.968908+0300`.
                    at: began
                        .get(11..19)
                        .unwrap_or("at a time it did not say")
                        .to_owned(),
                });
            }
            Some("logEvent") => {
                let Some(message) = entry["eventMessage"].as_str() else {
                    continue;
                };
                match read_line(message, &listened) {
                    Line::Refused(tag, denial) => read.refused.push((tag, denial)),
                    Line::Own(_, word) => {
                        let made = entry["timestamp"].as_str().and_then(timestamp);
                        read.shown.insert(word, made);
                    }
                    Line::Listening(_) | Line::Unreadable(_) | Line::Other => {}
                }
            }
            _ => {}
        }
    }
    read
}

/// Seconds since the epoch of `log`'s `2026-10-24 16:04:21.188527+0300`.
fn timestamp(text: &str) -> Option<f64> {
    let (date, rest) = text.split_once(' ')?;
    let mut ymd = date.splitn(3, '-').map(str::parse::<i32>);
    let (year, month, day) = (ymd.next()?.ok()?, ymd.next()?.ok()?, ymd.next()?.ok()?);
    let sign_at = rest.rfind(['+', '-'])?;
    let (clock, zone) = rest.split_at(sign_at);
    let mut hms = clock.splitn(3, ':');
    let (hour, minute) = (
        hms.next()?.parse::<u8>().ok()?,
        hms.next()?.parse::<u8>().ok()?,
    );
    let seconds: f64 = hms.next()?.parse().ok()?;
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let zone = &zone[1..];
    let (zh, zm) = (
        zone.get(..2)?.parse::<i32>().ok()?,
        zone.get(2..4)?.parse::<i32>().ok()?,
    );
    let date = time::Date::from_calendar_date(
        year,
        time::Month::try_from(u8::try_from(month).ok()?).ok()?,
        u8::try_from(day).ok()?,
    )
    .ok()?;
    let at = date
        .with_hms(hour, minute, 0)
        .ok()?
        .assume_utc()
        .unix_timestamp();
    Some(at as f64 + seconds - f64::from(sign * (zh * 3600 + zm * 60)))
}

fn unix(at: SystemTime) -> f64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// How long the store is given to stop growing, read once a second, where
/// what it shows is read as the list: a busy Mac fills it seconds late.
const SETTLE: Duration = Duration::from_secs(8);

/// What the log's store shows refused under `tag` within `window`, once it
/// stops growing or [`SETTLE`] has passed: never all there was (see the
/// note at the top), and nothing where it cannot be read.
pub(crate) fn stored(tag: &str, window: Duration) -> Vec<Denial> {
    let predicate = format!("sender == \"Sandbox\" AND eventMessage CONTAINS \"{tag}\"");
    let seconds = window.as_secs() + SETTLE.as_secs() + 2;
    let read = || {
        let Ok(out) = Command::new("/usr/bin/log")
            .args([
                "show",
                "--last",
                &format!("{seconds}s"),
                "--style",
                "ndjson",
            ])
            .args(["--predicate", &predicate])
            .stdin(Stdio::null())
            .output()
        else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|entry| entry["eventMessage"].as_str().map(str::to_owned))
            .filter_map(|message| match read_line(&message, |t| t == tag) {
                Line::Refused(_, denial) => Some(denial),
                _ => None,
            })
            .fold(Vec::new(), |mut denials, denial| {
                if !denials.contains(&denial) {
                    denials.push(denial);
                }
                denials
            })
    };
    let started = Instant::now();
    let mut denials = read();
    while started.elapsed() < SETTLE {
        std::thread::sleep(Duration::from_secs(1));
        let again = read();
        if again.len() == denials.len() {
            break;
        }
        denials = again;
    }
    denials
}

/// Words no one can guess, under `tag`, for a refusal of Kule's own:
/// another sandbox cannot make one that would read as it.
fn own_word(tag: &str, what: &str) -> Option<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .ok()?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!("{tag}-{what}-{hex}"))
}

/// Makes a refusal of Kule's own carrying `word`; whether one was made.
fn refuse(word: &str) -> bool {
    let exec = Path::new("/usr/bin/sandbox-exec");
    // Refused by design: sandbox-exec cannot start true, and says so.
    Command::new(exec)
        .arg("-p")
        .arg(format!(
            "(version 1)\n(allow default)\n\
             (deny process-exec (literal \"/usr/bin/true\") (with message \"{word}\"))\n"
        ))
        .arg("/usr/bin/true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        // It ran: no refusal was made, and none will come.
        .is_ok_and(|status| !status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: &str = "kule-1-2";

    fn event(message: &str) -> String {
        serde_json::json!({ "eventType": "logEvent", "eventMessage": message }).to_string()
    }

    fn after(lines: &[String]) -> State {
        let mut state = State::default();
        state.sandboxes.insert(TAG.to_owned(), Seen::default());
        for line in lines {
            take(&mut state, line);
        }
        state
    }

    #[test]
    fn a_refusal_is_read_by_the_tag_on_its_last_line() {
        let state = after(&[
            event("Sandbox: bash(1) deny(1) file-write-create /Users/me/x\nkule-1-2"),
            event(
                "1 duplicate report for Sandbox: bash(1) deny(1) file-write-create /Users/me/x\nkule-1-2",
            ),
            event(
                "Sandbox: sandbox-exec(2) deny(1) process-exec* /usr/bin/true\nkule-1-2-flush-ab",
            ),
            event(
                "Sandbox: sandbox-exec(3) deny(1) process-exec* /usr/bin/true\nkule-listening-cd",
            ),
        ]);
        let seen = &state.sandboxes[TAG];
        let paths: Vec<_> = seen.denials.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["/Users/me/x"]);
        assert!(state.listening.contains("kule-listening-cd"));
        assert!(!seen.short);
    }

    /// Another sandbox's refusal, also one of a path that names this
    /// sandbox's tag or under a tag that only starts like it, is not this
    /// sandbox's.
    #[test]
    fn what_only_names_the_tag_is_not_the_sandboxs() {
        let state = after(&[
            event("Sandbox: sh(3) deny(1) file-write-create /tmp/kule-1-2\nkule-9-9"),
            event("Sandbox: sh(3) deny(1) file-write-create /tmp/y\nkule-1-23"),
            event("Sandbox: sh(3) deny(1) process-exec* /usr/bin/true\nkule-1-23-flush-ab"),
        ]);
        let seen = &state.sandboxes[TAG];
        assert!(seen.denials.is_empty() && !seen.short);
    }

    /// What Kule cannot read is never passed over as nothing: the list is
    /// then told as perhaps not all.
    #[test]
    fn what_cannot_be_read_makes_the_list_short() {
        for line in [
            "not json".to_owned(),
            serde_json::json!({ "eventType": "lossEvent", "lossCount": 4 }).to_string(),
            serde_json::json!({ "eventType": "logEvent" }).to_string(),
            event("something the kernel never wrote\nkule-1-2"),
        ] {
            assert!(
                after(std::slice::from_ref(&line)).sandboxes[TAG].short,
                "{line}"
            );
        }
    }

    #[test]
    fn past_the_most_kept_the_list_is_short() {
        let lines: Vec<_> = (0..=MOST)
            .map(|i| {
                event(&format!(
                    "Sandbox: sh(1) deny(1) file-read-data /p/{i}\nkule-1-2"
                ))
            })
            .collect();
        let state = after(&lines);
        assert_eq!(state.sandboxes[TAG].denials.len(), MOST);
        assert!(state.sandboxes[TAG].short);
    }

    /// What came that Kule could not read is told, also where Kule's last
    /// refusal came: the list may not be all. A loss of the kernel's
    /// messages alone is not that: it is told apart.
    #[test]
    fn a_short_list_is_never_told_as_all() {
        let short = Seen {
            short: true,
            ..Seen::default()
        };
        assert!(told(true, Some(&short)).incomplete);
        assert!(told(true, None).incomplete);
        assert!(told(false, Some(&Seen::default())).incomplete);
        assert!(!told(true, Some(&Seen::default())).incomplete);
        assert!(told(true, Some(&Seen::default())).lost.is_empty());
    }

    fn running(pid: u32) -> bool {
        // SAFETY: a signal of 0 only asks whether the process is there.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    fn gone_soon(pid: u32) -> bool {
        let started = Instant::now();
        while running(pid) {
            if started.elapsed() > Duration::from_secs(5) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    /// The child the shell runs, `sleep` standing for the stream.
    fn held_child(shell: &Child) -> u32 {
        let started = Instant::now();
        loop {
            let out = Command::new("/usr/bin/pgrep")
                .args(["-P", &shell.id().to_string()])
                .output()
                .unwrap();
            if let Some(pid) = String::from_utf8_lossy(&out.stdout).lines().next() {
                return pid.trim().parse().unwrap();
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the shell ran nothing"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// What Kule's process has open when the stream starts (a terminal of
    /// the app's, a pipe) is not held open by the stream, which lasts as
    /// long as the process: closed by Kule, it ends.
    #[test]
    fn what_kule_has_open_is_not_held_by_the_stream() {
        let mut ends = [0; 2];
        // SAFETY: pipe writes the two descriptors it is given; neither is
        // closed on exec, as a terminal openpty makes is not.
        assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
        let (mut shell, input, _output) = held("/bin/sleep", &["600"]).unwrap();
        let sleeping = held_child(&shell);
        // SAFETY: closing the write end pipe gave.
        unsafe { libc::close(ends[1]) };
        let mut ready = libc::pollfd {
            fd: ends[0],
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one pollfd, for up to five seconds.
        let polled = unsafe { libc::poll(&mut ready, 1, 5000) };
        let mut byte = [0u8; 1];
        // SAFETY: reading one byte into a buffer of one, once poll said so.
        let read =
            (polled > 0).then(|| unsafe { libc::read(ends[0], byte.as_mut_ptr().cast(), 1) });
        // SAFETY: closing the read end pipe gave.
        unsafe { libc::close(ends[0]) };
        drop(input);
        shell.wait().unwrap();
        assert!(gone_soon(sleeping));
        assert_eq!(read, Some(0), "the stream held the pipe Kule closed");
    }

    /// Once the shell's input closes, as when Kule's process ends however it
    /// ends, what the shell runs ends with it.
    #[test]
    fn what_the_shell_holds_ends_when_its_input_closes() {
        let (mut shell, input, _output) = held("/bin/sleep", &["600"]).unwrap();
        let held = held_child(&shell);
        assert!(running(held));
        drop(input);
        shell.wait().unwrap();
        assert!(gone_soon(held), "{held} outlived its shell");
    }

    /// A stream that never shows Kule's refusal is never taken as one that
    /// listens, and what it ran ends.
    #[test]
    fn a_stream_never_shown_to_listen_is_not_used() {
        let started = Instant::now();
        assert!(Stream::open("/bin/sleep", &["600"]).is_none());
        assert!(started.elapsed() >= WAIT);
    }

    #[test]
    fn words_of_kules_own_cannot_be_guessed() {
        let (a, b) = (
            own_word(TAG, "flush").unwrap(),
            own_word(TAG, "flush").unwrap(),
        );
        assert_ne!(a, b);
        assert!(a.starts_with("kule-1-2-flush-") && a.len() == "kule-1-2-flush-".len() + 32);
    }

    /// The store says what it lost only once it shows Kule's last refusal,
    /// and only the losses between when Kule began to listen and that
    /// refusal: one that ended before or began after is none of the
    /// sandbox's, and one Kule cannot place is taken as its.
    #[test]
    fn the_store_tells_the_losses_while_kule_listened() {
        let flush = serde_json::json!({
            "eventType": "logEvent",
            "timestamp": "2026-10-24 16:04:30.000000+0300",
            "eventMessage": "Sandbox: sandbox-exec(2) deny(1) process-exec* /usr/bin/true\nkule-1-2-flush-ab",
        })
        .to_string();
        let loss = |began: &str, ended: &str| {
            serde_json::json!({
                "eventType": "lossEvent", "processID": 0,
                "lossStartTimestamp": began,
                "lossEndTimestamp": ended,
            })
            .to_string()
        };
        let from = timestamp("2026-10-24 16:04:21.188527+0300").unwrap();
        assert!((from - 1_792_847_061.188_527).abs() < 1e-5, "{from}");
        let word = "kule-1-2-flush-ab";
        let read = |lines: &[String]| {
            store_read(&lines.join("\n"), |t| t == TAG)
                .said(from, TAG, word)
                .map(|shown| shown.lost)
        };
        assert_eq!(read(&[]), None);
        assert_eq!(read(std::slice::from_ref(&flush)), Some(vec![]));
        let ended_before = loss(
            "2026-10-24 16:04:19.000000+0300",
            "2026-10-24 16:04:20.188527+0300",
        );
        let reaching = loss(
            "2026-10-24 16:04:20.968908+0300",
            "2026-10-24 16:04:21.500000+0300",
        );
        let within = loss(
            "2026-10-24 16:04:25.100000+0300",
            "2026-10-24 16:04:25.200000+0300",
        );
        let same_second = loss(
            "2026-10-24 16:04:25.700000+0300",
            "2026-10-24 16:04:25.800000+0300",
        );
        let began_after = loss(
            "2026-10-24 16:04:31.000000+0300",
            "2026-10-24 16:04:31.100000+0300",
        );
        assert_eq!(
            read(&[
                ended_before,
                reaching,
                within,
                same_second,
                flush.clone(),
                began_after
            ]),
            Some(vec!["16:04:20".to_owned(), "16:04:25".to_owned()])
        );
        let unplaced = loss("sometime", "later");
        assert_eq!(
            read(&[unplaced, flush]),
            Some(vec!["at a time it did not say".to_owned()])
        );
    }

    /// What the store shows refused under the sandbox's tag comes with
    /// Kule's refusal, once each; another sandbox's does not, nor does
    /// anything before the store shows that refusal.
    #[test]
    fn the_store_tells_what_it_kept_refused_once_it_shows_kules_refusal() {
        let refused = |path: &str, tag: &str| {
            event(&format!(
                "Sandbox: sh(3) deny(1) file-write-create {path}\n{tag}"
            ))
        };
        let flush = event(
            "Sandbox: sandbox-exec(2) deny(1) process-exec* /usr/bin/true\nkule-1-2-flush-ab",
        );
        let read = |lines: &[String]| {
            store_read(&lines.join("\n"), |t| t == TAG)
                .said(0.0, TAG, "kule-1-2-flush-ab")
                .map(|shown| {
                    shown
                        .refused
                        .iter()
                        .map(|d| d.path.clone())
                        .collect::<Vec<_>>()
                })
        };
        let lines = [
            refused("/Users/me/x", TAG),
            refused("/Users/me/y", "kule-9-9"),
            event(
                "1 duplicate report for Sandbox: sh(3) deny(1) file-write-create /Users/me/x\nkule-1-2",
            ),
            refused("/Users/me/z", TAG),
        ];
        assert_eq!(read(&lines), None);
        assert_eq!(
            read(&[lines.as_slice(), &[flush]].concat()),
            Some(vec![
                "/Users/me/x".to_owned(),
                "/Users/me/x".to_owned(),
                "/Users/me/z".to_owned()
            ])
        );
    }
}
