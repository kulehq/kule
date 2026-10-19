//! Runs held to their limits, with a scripted agent in a real task and
//! sandbox: time, idleness, memory, disk and the processor.

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
                None,
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
    // Unlimited it gets nearly all of it; a busy CI machine takes some.
    let (free, _) = got(Plan::default());
    assert!(free > 2.2, "{free}");
    let (slowed, measured) = got(Plan {
        cpu_percent: Some(40),
        ..Plan::default()
    });
    assert!((0.6..1.9).contains(&slowed), "{slowed}");
    // And it is shown as it goes.
    assert!(
        measured
            .iter()
            .any(|m| m.processes >= 2 && (20..80).contains(&m.cpu_percent)),
        "{measured:?}"
    );
}
