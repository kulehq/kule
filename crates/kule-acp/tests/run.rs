//! The ACP client against a scripted agent, in a real task and sandbox.

use std::fs;

use futures::StreamExt;
use futures::channel::mpsc;
use kule_acp::{Event, run};
use kule_engine::{Change, Sandbox, Task};
use tempfile::TempDir;

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    outside: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        fs::write(ws.path().join("README.md"), "hello\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            _ws: ws,
            outside: TempDir::new().unwrap(),
            task,
        }
    }

    /// Runs `script` through the test agent in the sandbox; returns the
    /// events in order.
    fn run(&self, script: &str) -> Vec<Event> {
        let sandbox = Sandbox::new(&self.task).unwrap();
        let agent = sandbox
            .command(env!("CARGO_BIN_EXE_kule-test-agent"), &[])
            .unwrap();
        let (tx, rx) = mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            run(&self.task, agent, script, tx).await.unwrap();
        });
        runtime.block_on(rx.collect::<Vec<_>>())
    }
}

/// A finished tool call: its title, whether it failed, and its step's
/// changes (none when it took no step).
type Done<'a> = (&'a str, bool, Vec<(String, Option<Change>)>);

fn done(events: &[Event]) -> Vec<Done<'_>> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::ToolDone {
                title,
                failed,
                step,
                ..
            } => Some((
                title.as_str(),
                *failed,
                step.iter()
                    .flat_map(|s| &s.changes)
                    .map(|c| (c.path.clone(), c.change))
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

/// Real agents wait for their model between tool calls; then every step
/// holds exactly what its call changed.
#[test]
fn each_tool_call_becomes_a_step() {
    let s = Setup::new();
    let events = s.run("write a.txt one; say halfway; sleep 300; write b.txt two");

    assert_eq!(
        done(&events),
        [
            (
                "write a.txt",
                false,
                vec![("a.txt".into(), Some(Change::Created))]
            ),
            (
                "write b.txt",
                false,
                vec![("b.txt".into(), Some(Change::Created))]
            ),
        ]
    );
    assert!(events.contains(&Event::Message {
        text: "halfway".into()
    }));
    assert_eq!(
        events.last(),
        Some(&Event::Done {
            stop_reason: "endturn".into()
        })
    );
    assert_eq!(
        fs::read_to_string(s.task.copy().join("b.txt")).unwrap(),
        "two\n"
    );
    assert_eq!(s.task.steps().unwrap().len(), 2);
}

/// Steps come from scanning the copy when a tool call's end is reported, and
/// the agent does not wait for that. Calls back to back (or in parallel) can
/// land in one step, but no change is ever left out.
#[test]
fn back_to_back_tool_calls_lose_nothing() {
    let s = Setup::new();
    let events = s.run("write a.txt one; write b.txt two; write c.txt three");

    let mut seen: Vec<String> = done(&events)
        .into_iter()
        .flat_map(|(_, _, changes)| changes.into_iter().map(|(path, _)| path))
        .collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen, ["a.txt", "b.txt", "c.txt"]);
    assert_eq!(
        done(&events).len(),
        3,
        "one step per tool call, even if empty"
    );
}

/// Codex reviews an edit in a `think` call that ends before the edit does.
/// The review takes no step, so the change goes to the edit.
#[test]
fn a_call_that_only_thinks_takes_no_step() {
    let s = Setup::new();
    let events = s.run("reviewed-write a.txt one");

    assert_eq!(
        done(&events),
        [
            ("review", false, vec![]),
            (
                "edit a.txt",
                false,
                vec![("a.txt".into(), Some(Change::Created))]
            ),
        ]
    );
    let review_step = events.iter().find_map(|e| match e {
        Event::ToolDone { title, step, .. } if title == "review" => Some(step.is_none()),
        _ => None,
    });
    assert_eq!(review_step, Some(true));
    assert_eq!(s.task.steps().unwrap().len(), 1);
}

#[test]
fn the_agent_runs_inside_the_sandbox() {
    let s = Setup::new();
    let evil = s.outside.path().join("evil.txt");
    let events = s.run(&format!("write {} pwned", evil.display()));

    assert_eq!(
        done(&events),
        [(format!("write {}", evil.display()).as_str(), true, vec![])]
    );
    assert!(!evil.exists());
}

#[test]
fn permission_requests_are_answered_with_the_allow_option() {
    let s = Setup::new();
    let events = s.run("ask");

    assert!(events.contains(&Event::Permission {
        title: "delete everything".into(),
        chosen: "Allow".into()
    }));
    assert!(events.contains(&Event::Message {
        text: "permission: allow".into()
    }));
}

/// What the agent leaves running goes when the run ends, so nothing keeps
/// changing the copy behind the person's review.
#[test]
fn processes_the_agent_left_running_end_with_the_run() {
    let s = Setup::new();
    s.run("background pid.txt");
    let pid: i32 = fs::read_to_string(s.task.copy().join("pid.txt"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let start = std::time::Instant::now();
    // SAFETY: signal 0 only checks that the process exists.
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "process {pid} outlived the run"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
