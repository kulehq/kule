//! Holds a run to its limits as it goes (see [`kule_engine::Limits`]): stops
//! it when its time is up, when the agent has done nothing for too long, or
//! when its processes hold too much memory or its folders grew too much on
//! disk, and slows it down when they use more of the processor than they
//! may, by stopping and starting them in turn.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kule_engine::resources;
use kule_engine::{Limits, Task};
use serde::{Deserialize, Serialize};

use crate::RunControl;

/// What a run takes now, as last measured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    /// Processor use over the last moments, in percent of one core.
    pub cpu_percent: u32,
    /// Memory its processes hold together, in bytes.
    pub memory: u64,
    /// What its folders grew by since the task was made, in bytes; `None`
    /// until first measured.
    pub disk: Option<u64>,
    /// Its processes.
    pub processes: usize,
}

/// One look at the processor, and how long a slowed run runs in it.
const SLICE: Duration = Duration::from_millis(100);
/// The tree of processes is looked for again this often.
const TREE_EVERY: Duration = Duration::from_millis(500);
/// What is measured is reported this often.
const REPORT_EVERY: Duration = Duration::from_secs(2);
/// Processor use above this share of a core counts as the agent at work,
/// even when it says nothing (a long build).
const BUSY_PERCENT: u64 = 10;

/// What the governor needs from whoever runs it.
pub struct Watch {
    /// Whether a person is being asked something for the run: the agent
    /// waits on them, and is not idle.
    pub waiting: Box<dyn Fn() -> bool + Send>,
    /// Told what the run takes, every two seconds.
    pub report: Box<dyn Fn(Measured) + Send>,
}

/// The limits one run is held to, in the units the governor works in.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Plan {
    pub cpu_percent: Option<u32>,
    /// Bytes.
    pub memory: Option<u64>,
    /// Bytes the folders may grow by since the task was made.
    pub disk: Option<u64>,
    pub run: Option<Duration>,
    /// What the task's own time limit leaves this run, and that limit.
    pub task_left: Option<(Duration, Duration)>,
    pub idle: Option<Duration>,
}

impl Plan {
    /// `limits`, with `time_left` of the task's time.
    pub fn new(limits: &Limits, time_left: Option<Duration>) -> Plan {
        const MB: u64 = 1024 * 1024;
        let minutes = |m: u64| Duration::from_secs(m.saturating_mul(60));
        Plan {
            cpu_percent: limits.cpu_percent,
            memory: limits.memory_mb.map(|m| m.saturating_mul(MB)),
            disk: limits.disk_mb.map(|m| m.saturating_mul(MB)),
            run: limits.run_minutes.map(minutes),
            task_left: time_left.zip(limits.task_minutes.map(minutes)),
            idle: limits.idle_minutes.map(minutes),
        }
    }

    /// Which of the run's time limit and what the task has left ends it
    /// first, and why it would stop.
    fn deadline(&self) -> Option<(Duration, String)> {
        let run = self.run.map(|d| {
            (
                d,
                format!("stopped: the run reached its time limit, {}", span(d)),
            )
        });
        let task = self.task_left.map(|(left, total)| {
            (
                left,
                format!(
                    "stopped: the task's runs reached their time limit together, {}",
                    span(total)
                ),
            )
        });
        match (run, task) {
            (Some(r), Some(t)) => Some(if t.0 < r.0 { t } else { r }),
            (r, t) => r.or(t),
        }
    }
}

/// Starts holding the run `control` names to `plan`. Returns once the run
/// is over, with every process it slowed running again.
pub fn govern(task: &Task, plan: Plan, control: RunControl, watch: Watch) -> JoinHandle<()> {
    // Measured with or without a limit: what the run takes is shown.
    let disk = Arc::new(AtomicU64::new(u64::MAX));
    measure_disk(task, Arc::clone(&disk), control.clone());
    std::thread::spawn(move || {
        Governor {
            deadline: plan.deadline(),
            plan,
            control,
            watch,
            disk,
            pids: Vec::new(),
            cpu: HashMap::new(),
            reaped: HashMap::new(),
            share: 1.0,
            rate: None,
            paused: false,
        }
        .run()
    })
}

/// A time limit as a person set it: in minutes or hours, or seconds below
/// a minute.
fn span(d: Duration) -> String {
    let secs = d.as_secs();
    let (n, unit) = match secs {
        s if s < 60 => (s, "second"),
        s if s % 3600 == 0 => (s / 3600, "hour"),
        s => (s / 60, "minute"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn size(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= 1024 * MB {
        format!("{:.1} GB", bytes as f64 / (1024 * MB) as f64)
    } else {
        format!("{} MB", bytes / MB)
    }
}

/// Measures what the task's folders take, again and again until the run is
/// over: as often as a walk through them allows, at most every second.
fn measure_disk(task: &Task, into: Arc<AtomicU64>, control: RunControl) {
    let task = task.clone();
    std::thread::spawn(move || {
        while !control.is_over() {
            let started = Instant::now();
            // Read again each time: the caches made after the run started
            // (its first, as the agent is launched) are left out too.
            if let Ok(grown) = task.disk_grown() {
                into.store(grown, Ordering::SeqCst);
            }
            let pause = (started.elapsed() * 4).max(Duration::from_secs(1));
            let until = Instant::now() + pause;
            while Instant::now() < until && !control.is_over() {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    });
}

struct Governor {
    plan: Plan,
    deadline: Option<(Duration, String)>,
    control: RunControl,
    watch: Watch,
    /// What the folders grew by, as last measured; `u64::MAX` until then.
    disk: Arc<AtomicU64>,
    pids: Vec<i32>,
    /// Each process's processor time as last read.
    cpu: HashMap<i32, Duration>,
    /// Each process's ended children's processor time as last read.
    reaped: HashMap<i32, Duration>,
    /// The share of each slice a slowed run runs in.
    share: f64,
    /// Processor use while running, in cores, smoothed.
    rate: Option<f64>,
    paused: bool,
}

impl Governor {
    fn run(mut self) {
        let mut tree_at = Instant::now() - TREE_EVERY;
        let mut report_at = Instant::now();
        let mut used_since_report = Duration::ZERO;
        let mut used_this_second = (Instant::now(), Duration::ZERO);
        let mut memory = 0u64;
        while !self.control.is_over() {
            if self.control.reason().is_some() {
                // Stopping: the agent has to run to end its turn.
                self.resume();
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            if tree_at.elapsed() >= TREE_EVERY {
                tree_at = Instant::now();
                let pids = resources::tree(&self.control.roots());
                // Processes coming and going are work too (a build's
                // compilers), however short each one's life.
                if pids.iter().any(|p| !self.pids.contains(p)) {
                    self.control.touch();
                }
                self.pids = pids;
                let (held, used) = self.look();
                memory = held;
                used_since_report += used;
                used_this_second.1 += used;
                if let Some(limit) = self.plan.memory
                    && memory > limit
                {
                    // At once: memory over its limit can take the computer
                    // down before the agent ends its turn.
                    self.control.stop(format!(
                        "stopped: the run's processes held {}, over its memory limit of {}",
                        size(memory),
                        size(limit)
                    ));
                    resources::kill(&self.pids);
                    continue;
                }
            }
            if let Some((after, why)) = &self.deadline
                && self.control.elapsed() >= *after
            {
                self.control.stop(why.clone());
                continue;
            }
            let disk = self.disk.load(Ordering::SeqCst);
            let grown = (disk != u64::MAX).then_some(disk);
            if let (Some(limit), Some(grown)) = (self.plan.disk, grown)
                && grown > limit
            {
                self.control.stop(format!(
                    "stopped: the task's folders grew by {}, over its disk limit of {}",
                    size(grown),
                    size(limit)
                ));
                continue;
            }

            let used = self.slice();
            used_since_report += used;
            // Busy processes are an agent at work, quiet or not.
            used_this_second.1 += used;
            if used_this_second.0.elapsed() >= Duration::from_secs(1) {
                let window = used_this_second.0.elapsed();
                if used_this_second.1.as_nanos() * 100 >= window.as_nanos() * BUSY_PERCENT as u128 {
                    self.control.touch();
                }
                used_this_second = (Instant::now(), Duration::ZERO);
            }
            // Counted from when the agent started: installing it is not
            // the agent doing nothing.
            if let Some(idle) = self.plan.idle
                && !self.control.roots().is_empty()
            {
                if (self.watch.waiting)() {
                    self.control.touch();
                } else if self.control.quiet_for() >= idle {
                    self.control
                        .stop(format!("stopped: the agent did nothing for {}", span(idle)));
                    continue;
                }
            }
            if report_at.elapsed() >= REPORT_EVERY {
                let window = report_at.elapsed();
                (self.watch.report)(Measured {
                    cpu_percent: (used_since_report.as_secs_f64() / window.as_secs_f64() * 100.0)
                        .round() as u32,
                    memory,
                    disk: grown,
                    processes: self.pids.len(),
                });
                report_at = Instant::now();
                used_since_report = Duration::ZERO;
            }
        }
        self.resume();
    }

    /// What the run's processes hold in memory together, and the processor
    /// time they used since last looked at.
    fn look(&mut self) -> (u64, Duration) {
        let mut memory = 0;
        let mut used = Duration::ZERO;
        let mut ended = false;
        let mut seen = HashMap::with_capacity(self.pids.len());
        let mut reaped = HashMap::with_capacity(self.pids.len());
        for u in resources::usage(&self.pids) {
            memory += u.memory;
            let before = self.cpu.get(&u.pid).copied().unwrap_or_default();
            used += u.cpu.saturating_sub(before);
            seen.insert(u.pid, u.cpu);
            // Kept apart from `used`: a child seen at work was counted
            // there already, and the processor limit would count it twice.
            ended |= u.reaped > self.reaped.get(&u.pid).copied().unwrap_or_default();
            reaped.insert(u.pid, u.reaped);
        }
        self.cpu = seen;
        self.reaped = reaped;
        // Children that ran and ended between two looks: a build or a
        // script at work, though none of them was seen.
        if ended {
            self.control.touch();
        }
        (memory, used)
    }

    /// One slice: the run runs in all of it, or, when it uses more of the
    /// processor than it may, in a share of it and is stopped for the
    /// rest. Returns the processor time it used.
    fn slice(&mut self) -> Duration {
        let Some(percent) = self.plan.cpu_percent else {
            std::thread::sleep(SLICE);
            return self.look().1;
        };
        let running = SLICE.mul_f64(self.share);
        if self.share < 1.0 {
            self.resume();
        }
        std::thread::sleep(running);
        if self.share < 1.0 {
            resources::pause(&self.pids);
            self.paused = true;
            std::thread::sleep(SLICE - running);
        }
        let used = self.look().1;
        // How fast it uses the processor while running, and the share of
        // a slice that keeps it to its limit.
        let now = used.as_secs_f64() / running.as_secs_f64().max(1e-3);
        let rate = match self.rate {
            Some(rate) => rate * 0.6 + now * 0.4,
            None => now,
        };
        self.rate = Some(rate);
        let allowed = f64::from(percent) / 100.0;
        self.share = if rate <= allowed {
            1.0
        } else {
            (allowed / rate).clamp(0.02, 1.0)
        };
        used
    }

    fn resume(&mut self) {
        if self.paused {
            // The tree as it is now: a process paused when last looked at
            // may have been started since.
            resources::resume(&self.pids);
            self.paused = false;
        }
    }
}
