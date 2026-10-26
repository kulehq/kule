//! Runs held to their limits, with a scripted agent in a real task and
//! sandbox: time, idleness, memory, disk and the processor.
#![cfg(unix)]

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc;
use kule_acp::RunControl;
use kule_acp::governor::{Measured, Plan, Watch, govern};
use kule_engine::{Sandbox, Task};
use tempfile::TempDir;

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    task: Task,
}

/// How a run ended, how long it took, and what was measured meanwhile.
struct Ran {
    outcome: Result<String, String>,
    took: Duration,
    measured: Vec<Measured>,
}

impl Setup {
    fn new() -> Setup {
        Setup::holding(0)
    }

    /// A task on a folder holding a file of `bytes`.
    fn holding(bytes: usize) -> Setup {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        fs::write(ws.path().join("README.md"), "hello\n").unwrap();
        fs::write(ws.path().join("big"), vec![1u8; bytes]).unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            _ws: ws,
            task,
        }
    }

    fn run(&self, script: &str, plan: Plan) -> Ran {
        self.run_with(script, plan, false, &RunControl::new())
    }

    /// `script` held to `plan`; `waiting` says a person is being asked.
    fn run_with(&self, script: &str, plan: Plan, waiting: bool, control: &RunControl) -> Ran {
        let sandbox = Sandbox::new(&self.task).unwrap();
        let agent = sandbox
            .command(env!("CARGO_BIN_EXE_kule-test-agent"), &[])
            .unwrap();
        let measured = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&measured);
        let governor = govern(
            &self.task,
            plan,
            control.clone(),
            Watch {
                waiting: Box::new(move || waiting),
                report: Box::new(move |m| kept.lock().unwrap().push(m)),
            },
        );
        let (tx, rx) = mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let started = Instant::now();
        let outcome = runtime
            .block_on(kule_acp::run_pictured(
                &self.task,
                agent,
                script,
                &[],
                None,
                None,
                &[],
                &sandbox,
                false,
                control,
                tx,
            ))
            .map_err(|e| e.to_string());
        let took = started.elapsed();
        governor.join().unwrap();
        let _ = runtime.block_on(rx.collect::<Vec<_>>());
        let measured = measured.lock().unwrap().clone();
        Ran {
            outcome,
            took,
            measured,
        }
    }
}

fn python() -> bool {
    std::process::Command::new("python3")
        .arg("-c")
        .arg("pass")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn stopped(ran: &Ran, why: &str) {
    let error = ran.outcome.as_ref().expect_err("the run should stop");
    assert!(error.contains(why), "{error}");
}

#[test]
fn a_run_stops_at_its_time_limit() {
    let s = Setup::new();
    let ran = s.run(
        "wait 30000",
        Plan {
            run: Some(Duration::from_secs(1)),
            ..Plan::default()
        },
    );
    stopped(&ran, "the run reached its time limit, 1 second");
    // Cancelled as a person would: the agent ended its turn.
    assert!(ran.took < Duration::from_secs(5), "{:?}", ran.took);

    let ran = s.run("wait 200", Plan::default());
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
}

#[test]
fn a_run_stops_when_the_task_s_time_is_used_up() {
    let s = Setup::new();
    let ran = s.run(
        "wait 30000",
        Plan {
            run: Some(Duration::from_secs(60)),
            task_left: Some((Duration::from_millis(700), Duration::from_secs(120))),
            ..Plan::default()
        },
    );
    stopped(&ran, "time limit together, 2 minutes");
    assert!(ran.took < Duration::from_secs(5), "{:?}", ran.took);
}

#[test]
fn an_agent_that_does_nothing_is_stopped_and_one_at_work_is_not() {
    let s = Setup::new();
    let idle = Plan {
        idle: Some(Duration::from_secs(1)),
        ..Plan::default()
    };
    let ran = s.run("say hello; wait 30000", idle);
    stopped(&ran, "the agent did nothing for 1 second");
    assert!(ran.took < Duration::from_secs(5), "{:?}", ran.took);

    // Speaking now and then.
    let ran = s.run(
        "say a; wait 600; say b; wait 600; say c; wait 600; say d",
        idle,
    );
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    // Waiting on a person.
    let ran = s.run_with("wait 2500", idle, true, &RunControl::new());
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    // Quiet but at work, as a long build is: starting short-lived
    // processes, or busy in one.
    let starting = "exec end=$(( $(date +%s) + 3 ))\nwhile [ $(date +%s) -lt $end ]\ndo :\ndone";
    let ran = s.run(starting, idle);
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    if python() {
        let busy = "exec python3 -c \"import time\ne=time.time()+3\nwhile time.time()<e: pass\"";
        let ran = s.run(busy, idle);
        assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    }
}

#[test]
fn a_person_stops_a_run() {
    let s = Setup::new();
    let control = RunControl::new();
    let stopper = control.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        stopper.stop("stopped by you");
    });
    let ran = s.run_with("wait 30000", Plan::default(), false, &control);
    stopped(&ran, "stopped by you");
    assert!(ran.took < Duration::from_secs(5), "{:?}", ran.took);
}

#[test]
fn a_run_over_its_memory_limit_is_stopped_at_once() {
    if !python() {
        eprintln!("skipped: no python3");
        return;
    }
    let s = Setup::new();
    // It beats into a file while it lives. (A script's commands are split
    // at semicolons.)
    let beat = s.task.copy().join("beat");
    let script = "exec python3 -c \"import time\nbeat=lambda: open('beat','a').write('.')\nbeat()\nb=b'x'*(400*1024*1024)\nwhile True:\n  beat()\n  time.sleep(0.02)\"";
    let ran = s.run(
        script,
        Plan {
            memory: Some(150 * 1024 * 1024),
            ..Plan::default()
        },
    );
    stopped(&ran, "over its memory limit of 150 MB");
    assert!(ran.took < Duration::from_secs(10), "{:?}", ran.took);
    assert!(beat.exists(), "it never beat");
    let len = || fs::metadata(&beat).map_or(0, |m| m.len());
    let before = len();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(len(), before, "still alive");
}

#[test]
fn a_run_whose_folders_grow_past_the_disk_limit_is_stopped() {
    // What the folder held when the task was made does not count.
    let s = Setup::holding(8 << 20);
    let ran = s.run(
        "wait 1500",
        Plan {
            disk: Some(4 << 20),
            ..Plan::default()
        },
    );
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));

    let ran = s.run(
        "exec head -c 20971520 /dev/zero > grown && sleep 60",
        Plan {
            disk: Some(10 << 20),
            ..Plan::default()
        },
    );
    stopped(&ran, "over its disk limit of 10 MB");
    assert!(ran.took < Duration::from_secs(15), "{:?}", ran.took);
}

#[test]
fn a_run_over_its_processor_limit_is_slowed_down() {
    if !python() {
        eprintln!("skipped: no python3");
        return;
    }
    let s = Setup::new();
    // Busy for three seconds of the clock; writes the processor time it got.
    let script = "exec python3 -c \"import time\ne=time.time()+3\nwhile time.time()<e: pass\nopen('cpu','w').write(str(time.process_time()))\"";
    let got = |plan| {
        let ran = s.run(script, plan);
        assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
        let cpu: f64 = fs::read_to_string(s.task.copy().join("cpu"))
            .unwrap()
            .parse()
            .unwrap();
        (cpu, ran.measured)
    };
    // Unlimited it gets what the machine gives, a busy CI machine as little
    // as two thirds of it; slowed it gets clearly less, near its 40%, busy
    // machine or not.
    let (free, _) = got(Plan::default());
    assert!(free > 1.5, "{free}");
    let (slowed, measured) = got(Plan {
        cpu_percent: Some(40),
        ..Plan::default()
    });
    assert!((0.6..1.35).contains(&slowed), "{slowed}");
    assert!(free > slowed * 1.3, "free {free}, slowed {slowed}");
    // And it is shown as it goes.
    assert!(
        measured
            .iter()
            .any(|m| m.processes >= 2 && (20..80).contains(&m.cpu_percent)),
        "{measured:?}"
    );
}

#[test]
fn a_slowed_run_that_goes_quiet_runs_on() {
    if !python() {
        eprintln!("skipped: no python3");
        return;
    }
    let s = Setup::new();
    // Busy for a second, then quiet for one, then done.
    let script = "exec python3 -c \"import time\ne=time.time()+1\nwhile time.time()<e: pass\ntime.sleep(1)\nopen('done','w').write('1')\"";
    let ran = s.run(
        script,
        Plan {
            cpu_percent: Some(40),
            run: Some(Duration::from_secs(20)),
            ..Plan::default()
        },
    );
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    assert!(ran.took < Duration::from_secs(8), "{:?}", ran.took);
    assert!(s.task.copy().join("done").exists());
}

/// ACP is a message a line, and a line is held whole before it is read:
/// one past the limit ends the run before it can fill Kule's memory.
#[test]
fn a_message_past_its_limit_ends_the_run() {
    let s = Setup::new();
    let ran = s.run(
        "exec head -c 104857600 /dev/zero | tr '\\0' x",
        Plan {
            run: Some(Duration::from_secs(60)),
            ..Plan::default()
        },
    );
    stopped(&ran, "the agent sent a message larger than 16 MiB");
    assert!(ran.took < Duration::from_secs(30), "{:?}", ran.took);
}

#[cfg(unix)]
/// What the agent prints for itself goes to its log, which Kule writes
/// outside the task's limits; it keeps the end of it, not all of it, and
/// says where it turned the log over and what it let go of.
#[test]
fn the_agent_s_log_keeps_its_end_within_a_limit() {
    let s = Setup::new();
    // A log turned over before, which this run's turn replaces.
    fs::write(s.task.agent_log().with_file_name("agent.log.1"), "older\n").unwrap();
    let ran = s.run(
        // Three bytes first, so the pieces read from the pipe do not end on
        // the limit by chance before it turns over the first time.
        "exec printf abc >&2; exec head -c 10485760 /dev/zero >&2; exec echo the-end >&2",
        Plan::default(),
    );
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    let log = s.task.agent_log();
    let older = log.with_file_name("agent.log.1");
    // The log is written apart from the run: given a moment to catch up.
    let until = Instant::now() + Duration::from_secs(20);
    while !fs::read(&log).unwrap_or_default().ends_with(b"the-end\n") && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        fs::read(&log).unwrap().ends_with(b"the-end\n"),
        "the end was lost"
    );
    let len = |p: &std::path::Path| fs::metadata(p).map_or(0, |m| m.len());
    assert!(
        len(&log) <= 8 << 20 && len(&older) <= 8 << 20,
        "{} and {}",
        len(&log),
        len(&older)
    );
    assert!(len(&older) > 0, "it never turned over");
    let first = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    assert!(
        first.starts_with("--- Kule turned this log over here")
            && first.contains("agent.log.1")
            && first.contains("the log before that was let go"),
        "{:?}",
        first.chars().take(120).collect::<String>()
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&older).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

/// A process that left the agent's group and session, its parent gone.
/// It beats into `beat` and holds `mb` megabytes; its pid is in `pid`. The
/// command ends once it has beaten.
fn escaped(mb: usize) -> String {
    format!(
        "exec python3 -c \"import os, time\nif os.fork():\n  end=time.time()+10\n  while time.time()<end and not os.path.exists('beat'): time.sleep(0.01)\n  os._exit(0)\nos.setsid()\nif os.fork(): os._exit(0)\nopen('pid','w').write(str(os.getpid()))\nb=b'x'*({mb}*1024*1024)\nwhile True:\n  open('beat','a').write('.')\n  time.sleep(0.02)\" > /dev/null 2>&1"
    )
}

/// Ends what [`escaped`] left, whatever the test found.
struct Reaped(std::path::PathBuf);

impl Drop for Reaped {
    fn drop(&mut self) {
        // In a PID namespace of its own the pid is the namespace's, and
        // the namespace ended it.
        if kule_engine::own_processes() {
            return;
        }
        if let Some(pid) = fs::read_to_string(&self.0)
            .ok()
            .and_then(|p| p.trim().parse::<i32>().ok())
        {
            #[cfg(unix)]
            #[cfg(unix)]
            // SAFETY: kill only sends a signal.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            #[cfg(windows)]
            kule_engine::resources::kill(&[pid]);
            #[cfg(windows)]
            kule_engine::resources::kill(&[pid]);
        }
    }
}

/// Whether what adds to `file` stops within a few seconds.
fn stops_beating(file: &std::path::Path) -> bool {
    let len = || fs::metadata(file).map_or(0, |m| m.len());
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        let before = len();
        std::thread::sleep(Duration::from_millis(300));
        if len() == before {
            return true;
        }
    }
    false
}

/// However a process leaves the agent's group, it ends with the run, so
/// nothing keeps changing the copy behind the person's review.
#[test]
fn a_process_that_left_the_agent_ends_with_the_run() {
    if !python() {
        eprintln!("skipped: no python3");
        return;
    }
    let s = Setup::new();
    let _reaped = Reaped(s.task.copy().join("pid"));
    let beat = s.task.copy().join("beat");
    let ran = s.run(&format!("{}; wait 500", escaped(0)), Plan::default());
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    assert!(beat.exists(), "it never beat");
    assert!(stops_beating(&beat), "a process outlived the run");
}

/// The run's limits count a process that left the agent too.
#[test]
fn a_process_that_left_the_agent_counts_toward_its_memory_limit() {
    if !python() {
        eprintln!("skipped: no python3");
        return;
    }
    let s = Setup::new();
    let _reaped = Reaped(s.task.copy().join("pid"));
    let ran = s.run(
        &format!("{}; wait 15000", escaped(400)),
        Plan {
            memory: Some(150 * 1024 * 1024),
            ..Plan::default()
        },
    );
    stopped(&ran, "over its memory limit of 150 MB");
    assert!(ran.took < Duration::from_secs(12), "{:?}", ran.took);
}

/// Kule keeps what it needs of each tool call while it is open: an agent
/// cannot open them without end, and those that end are let go.
#[test]
fn tool_calls_left_open_without_end_stop_the_run() {
    let s = Setup::new();
    let ran = s.run("calls 5000 done", Plan::default());
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    // What comes for a call after it ended does not open it again.
    let ran = s.run("calls 5000 late; wait 2000", Plan::default());
    assert_eq!(ran.outcome.as_deref(), Ok("endturn"));
    // Stopped as it goes on.
    let ran = s.run("calls 5000 open; wait 10000", Plan::default());
    stopped(&ran, "the agent left more than 1024 tool calls open");
}
