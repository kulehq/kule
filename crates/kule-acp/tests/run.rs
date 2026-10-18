//! The ACP client against a scripted agent, in a real task and sandbox.

use std::fs;

use futures::StreamExt;
use futures::channel::mpsc;
use kule_acp::{Cost, Event, run, run_limited};
use kule_engine::{Change, RunUsage, Sandbox, Task};
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
            run(&self.task, agent, script, None, tx).await.unwrap();
        });
        runtime.block_on(rx.collect::<Vec<_>>())
    }

    /// [`Setup::run`] stopped at `limit`; returns how it ended and the
    /// events.
    fn run_limited(&self, script: &str, limit: Cost) -> (Result<String, String>, Vec<Event>) {
        let sandbox = Sandbox::new(&self.task).unwrap();
        let agent = sandbox
            .command(env!("CARGO_BIN_EXE_kule-test-agent"), &[])
            .unwrap();
        let (tx, rx) = mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let outcome = runtime
            .block_on(run_limited(
                &self.task,
                agent,
                script,
                None,
                Some(limit),
                tx,
            ))
            .map_err(|e| e.to_string());
        (outcome, runtime.block_on(rx.collect::<Vec<_>>()))
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
        chosen: "Yes".into(),
        granted: true,
    }));
    assert!(events.contains(&Event::Message {
        text: "permission: allow".into()
    }));

    // Told apart by what the option does, not by its words.
    let events = s.run("ask-reject-only");
    assert!(events.contains(&Event::Permission {
        title: "delete everything".into(),
        chosen: "No".into(),
        granted: false,
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

fn usd(amount: f64) -> Cost {
    Cost {
        amount,
        currency: "USD".into(),
    }
}

/// What the agent reports it used adds up to the run's usage: its last
/// cost (the session's so far) and the turn's tokens.
#[test]
fn what_a_run_used_is_what_the_agent_reported() {
    let s = Setup::new();
    let events = s.run("cost 0.10; say working; cost 0.25; tokens 1200 300");
    let mut usage = RunUsage::default();
    for e in &events {
        e.tally(&mut usage);
    }
    assert_eq!(usage.cost, Some(usd(0.25)));
    assert_eq!((usage.input_tokens, usage.output_tokens), (1200, 300));
    assert_eq!((usage.context_used, usage.context_size), (1000, 200_000));
}

/// Once the reported cost reaches the limit, the turn is cancelled: what
/// the agent would do next is not done, and the run fails with why.
#[test]
fn a_run_stops_when_its_cost_reaches_the_limit() {
    let s = Setup::new();
    let script = "cost 0.10; write a.txt one; cost 0.30; sleep 800; write b.txt two";
    let (outcome, _) = s.run_limited(script, usd(0.25));
    let error = outcome.unwrap_err();
    assert!(error.contains("budget"), "{error}");
    assert!(s.task.copy().join("a.txt").exists());
    assert!(!s.task.copy().join("b.txt").exists());

    // A limit in another currency cannot be compared, and holds nothing.
    let s = Setup::new();
    let eur = Cost {
        amount: 0.25,
        currency: "EUR".into(),
    };
    let (outcome, _) = s.run_limited(script, eur);
    assert!(outcome.is_ok(), "{outcome:?}");
    assert!(s.task.copy().join("b.txt").exists());
}

/// An agent that reports the cost only as its turn ends has done its work
/// by then: the run is not called stopped, though it went over.
#[test]
fn a_cost_reported_at_the_end_of_the_turn_stops_nothing() {
    let s = Setup::new();
    let (outcome, events) = s.run_limited("write a.txt one; cost 0.30", usd(0.25));
    assert_eq!(outcome, Ok("endturn".to_owned()));
    assert!(s.task.copy().join("a.txt").exists());
    let mut usage = RunUsage::default();
    for e in &events {
        e.tally(&mut usage);
    }
    assert_eq!(usage.cost, Some(usd(0.30)));
}

/// What a command shows reaches the run as it grows, each time whole, from
/// the tool call's content or its raw output.
#[test]
fn a_command_s_output_is_passed_on_as_it_grows() {
    let s = Setup::new();
    let outputs = |events: &[Event]| -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::ToolOutput { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    let events = s.run("run ok 1\\nok 2");
    assert_eq!(outputs(&events), ["ok 1", "ok 1\nok 2"]);
    // The output names its call.
    let call = events.iter().find_map(|e| match e {
        Event::ToolCall { id, .. } => Some(id.clone()),
        _ => None,
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::ToolOutput { id, .. } if Some(id) == call.as_ref()))
    );
    assert_eq!(outputs(&s.run("run-raw abcd")), ["ab", "abcd"]);
}

/// Only the end of a long output is kept.
#[test]
fn a_long_output_keeps_its_end() {
    let s = Setup::new();
    let size = kule_acp::MAX_OUTPUT * 3;
    let events = s.run(&format!("run-big {size}"));
    let last = events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::ToolOutput { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap();
    assert!(last.starts_with('…'));
    assert_eq!(last.len(), kule_acp::MAX_OUTPUT + '…'.len_utf8());
}

/// Pictures go after the prompt's text to an agent that takes them; one
/// that does not gets the text alone, and the run says so.
#[test]
fn pictures_go_only_to_an_agent_that_takes_them() {
    let s = Setup::new();
    let picture = kule_acp::Picture {
        mime: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    };
    let run = |args: &[&str]| {
        let sandbox = Sandbox::new(&s.task).unwrap();
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let agent = sandbox
            .command(env!("CARGO_BIN_EXE_kule-test-agent"), &args)
            .unwrap();
        let (tx, rx) = mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(kule_acp::run_pictured(
                &s.task,
                agent,
                "pictures",
                std::slice::from_ref(&picture),
                None,
                None,
                &[],
                tx,
            ))
            .unwrap();
        runtime.block_on(rx.collect::<Vec<_>>())
    };
    let taken = run(&["--pictures"]);
    assert!(
        taken.contains(&Event::Message {
            text: "pictures: image/png 12".into()
        }),
        "{taken:?}"
    );
    assert!(!taken.iter().any(|e| matches!(e, Event::Note { .. })));
    let left = run(&[]);
    assert!(
        left.contains(&Event::Message {
            text: "pictures: ".into()
        }),
        "{left:?}"
    );
    assert!(
        left.iter().any(|e| matches!(e, Event::Note { .. })),
        "{left:?}"
    );
}

/// The places Kule lists after a prompt reach the agent whole, as text,
/// line breaks and all.
#[test]
fn a_prompt_reaches_the_agent_as_it_was_sent() {
    let s = Setup::new();
    let prompt = "echo Check this.\n\nAbout these places in the task's files (paths are from the working directory):\n1. README.md, line 1: \"hello; write x y\"";
    let events = s.run(prompt);
    assert!(
        events.contains(&Event::Message {
            text: prompt.into()
        }),
        "{events:?}"
    );
    // What came after `echo` was not run as a command.
    assert!(!s.task.copy().join("x").exists());
}

#[test]
fn the_session_is_given_the_mcp_servers_it_is_offered() {
    let s = Setup::new();
    let server = kule_acp::McpCommand::prepared(
        None,
        "db",
        "npx",
        &["-y".into(), "@acme/db@1.2.3".into(), "--stdio".into()],
        &[("DB_NAME".to_owned(), "notes".to_owned())].into(),
    );
    let sandbox = Sandbox::new(&s.task).unwrap();
    let agent = sandbox
        .command(env!("CARGO_BIN_EXE_kule-test-agent"), &[])
        .unwrap();
    let (tx, rx) = mpsc::unbounded();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(kule_acp::run_with(
            &s.task,
            agent,
            "mcp",
            None,
            None,
            &[server],
            tx,
        ))
        .unwrap();
    let events = runtime.block_on(rx.collect::<Vec<_>>());
    assert!(
        events.contains(&Event::Message {
            text: "mcp db: npx -y @acme/db@1.2.3 --stdio".into()
        }),
        "{events:?}"
    );
    // Without any, the session is given none.
    let events = s.run("mcp");
    assert!(
        events.contains(&Event::Message {
            text: "mcp ".into()
        }),
        "{events:?}"
    );
}
