//! The ACP client against a scripted agent, in a real task and sandbox.
#![cfg(unix)]

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
            .start(env!("CARGO_BIN_EXE_kule-test-agent"), &[])
            .unwrap();
        let (tx, rx) = mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            run(&self.task, agent, &sandbox, script, None, tx)
                .await
                .unwrap();
        });
        runtime.block_on(rx.collect::<Vec<_>>())
    }

    /// [`Setup::run`] stopped at `limit`; returns how it ended and the
    /// events.
    fn run_limited(&self, script: &str, limit: Cost) -> (Result<String, String>, Vec<Event>) {
        let sandbox = Sandbox::new(&self.task).unwrap();
        let agent = sandbox
            .start(env!("CARGO_BIN_EXE_kule-test-agent"), &[])
            .unwrap();
        let (tx, rx) = mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let outcome = runtime
            .block_on(run_limited(
                &self.task,
                agent,
                &sandbox,
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

#[test]
fn each_piece_of_an_answer_says_which_message_it_is_part_of() {
    let s = Setup::new();
    let events = s.run("say-in m1 one; say-in m1 two; say-in m2 three");
    let pieces: Vec<(String, Option<String>)> = events
        .iter()
        .filter_map(|e| match e {
            Event::Message { text, id } => Some((text.clone(), id.clone())),
            _ => None,
        })
        .collect();
    let piece = |text: &str, id: &str| (text.to_owned(), Some(id.to_owned()));
    assert_eq!(
        pieces,
        [piece("one", "m1"), piece("two", "m1"), piece("three", "m2")]
    );
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
        text: "halfway".into(),
        id: None
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
    // What the agent printed is its owner's alone to read.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let log = s.task.agent_log();
        let mode = std::fs::metadata(&log).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
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
        text: "permission: allow".into(),
        id: None
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
    let beat = s.task.copy().join("beat");
    s.run("background beat");
    assert!(beat.exists(), "the process never beat");
    assert!(stops_beating(&beat), "a process outlived the run");
}

/// What the agent left running outside its process group is told by name
/// as the run ends, where the person watches it.
#[cfg(target_os = "macos")]
#[test]
fn a_stray_the_agent_left_is_told_as_the_run_ends() {
    let s = Setup::new();
    let events = s.run("stray");
    let notes = notes(&events);
    assert!(
        notes.iter().any(|n| n.starts_with("Kule ended sleep,")),
        "{notes:?}"
    );
}

/// A run that leaves nothing running tells of nothing ended: its own
/// processes, ended with it, are not news.
#[test]
fn a_run_that_leaves_nothing_tells_of_nothing_ended() {
    let s = Setup::new();
    let beat = s.task.copy().join("beat");
    let events = s.run("background beat");
    assert!(stops_beating(&beat));
    let notes = notes(&events);
    assert!(!notes.iter().any(|n| n.contains("Kule ended")), "{notes:?}");
}

/// A log that cannot be turned over is said once in the run's notes, and
/// the agent goes on: what it writes for itself is still read, so it is
/// never cut off for it.
#[test]
fn a_log_that_cannot_turn_over_says_so_and_the_agent_goes_on() {
    let s = Setup::new();
    let older = s.task.agent_log().with_file_name("agent.log.1");
    fs::create_dir(&older).unwrap();
    fs::write(older.join("in-the-way"), "x").unwrap();
    let events =
        s.run("exec head -c 10485760 /dev/zero >&2; exec head -c 10485760 /dev/zero >&2; say done");
    let notes = notes(&events);
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.contains("not kept for the rest of this run"))
            .count(),
        1,
        "{notes:?}"
    );
    assert!(
        said(&events).iter().any(|m| m == "done"),
        "{:?}",
        said(&events)
    );
}

fn notes(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Note { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Whether what adds to `file` stops within a few seconds: its length then
/// holds for a while.
fn stops_beating(file: &std::path::Path) -> bool {
    let len = || fs::metadata(file).map_or(0, |m| m.len());
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < until {
        let before = len();
        std::thread::sleep(std::time::Duration::from_millis(300));
        if len() == before {
            return true;
        }
    }
    false
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

/// An agent that runs its commands itself and reports their output in
/// pieces (the terminal extension Claude Code's and Codex's adapters speak)
/// gets asked to, and the pieces add up as they come.
#[test]
fn output_reported_in_pieces_adds_up_as_it_comes() {
    let s = Setup::new();
    let outputs = |script: &str| -> Vec<String> {
        s.run(script)
            .iter()
            .filter_map(|e| match e {
                Event::ToolOutput { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    // The first piece goes at once; the ones close behind it wait, and
    // none is lost. Each message is all the output so far. How many
    // messages it takes rests on the clock: a loaded machine can space
    // pieces wider than the wait.
    let few = outputs("stream one two three");
    assert_eq!(few.first().map(String::as_str), Some("one\n"), "{few:?}");
    assert_eq!(
        few.last().map(String::as_str),
        Some("one\ntwo\nthree\n"),
        "{few:?}"
    );
    assert!(
        few.windows(2)
            .all(|w| w[1].starts_with(&w[0]) && w[1] != w[0]),
        "{few:?}"
    );
    // A thousand pieces in a burst are not a thousand messages.
    let words: Vec<String> = (0..1000).map(|n| n.to_string()).collect();
    let many = outputs(&format!("stream {}", words.join(" ")));
    assert!(many.len() < 10, "{} messages", many.len());
    assert_eq!(many.last().unwrap(), &format!("{}\n", words.join("\n")));
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

impl Setup {
    /// [`Setup::run`], with Kule running the agent's commands for it in
    /// the agent's sandbox when `terminals` is set.
    fn run_terminals(&self, script: &str, terminals: bool) -> Vec<Event> {
        self.run_terminals_with(script, terminals, &kule_acp::RunControl::new())
    }

    fn run_terminals_with(
        &self,
        script: &str,
        terminals: bool,
        control: &kule_acp::RunControl,
    ) -> Vec<Event> {
        let sandbox = Sandbox::new(&self.task).unwrap();
        let agent = sandbox
            .start(env!("CARGO_BIN_EXE_kule-test-agent"), &[])
            .unwrap();
        let (tx, rx) = mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(kule_acp::run_pictured(
                &self.task,
                agent,
                script,
                &[],
                None,
                None,
                &[],
                &sandbox,
                terminals,
                control,
                tx,
            ))
            .unwrap();
        runtime.block_on(rx.collect::<Vec<_>>())
    }
}

fn said(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Message { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// A command the agent has Kule run: it runs in the task's copy, its
/// output (both streams) reaches the agent and the tool call that shows
/// it, and how it ended reaches the agent.
#[test]
fn kule_runs_the_agent_s_commands_and_passes_their_output_on() {
    let s = Setup::new();
    let events = s.run_terminals("shell pwd && echo out && echo err >&2 && exit 3", true);
    let copy = fs::canonicalize(s.task.copy()).unwrap();
    let said = said(&events);
    assert_eq!(said.len(), 1, "{events:?}");
    assert!(said[0].starts_with("exit 3: "), "{said:?}");
    for part in [copy.to_str().unwrap(), "out", "err"] {
        assert!(said[0].contains(part), "{part} missing from {said:?}");
    }
    let shown = events.iter().rev().find_map(|e| match e {
        Event::ToolOutput { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert!(
        shown.is_some_and(|t| t.contains("out") && t.contains("err")),
        "{events:?}"
    );
    // Unless it is to, Kule offers no terminals.
    assert_eq!(said_of(&s, "shell echo hi", false), ["no terminals"]);
}

fn said_of(s: &Setup, script: &str, terminals: bool) -> Vec<String> {
    said(&s.run_terminals(script, terminals))
}

/// The agent's commands are held by its sandbox, may not set what the
/// sandbox sets or what loads code into every program, and run in the
/// copy only.
#[test]
fn the_agent_s_commands_run_inside_its_sandbox() {
    let s = Setup::new();
    let outside = s.outside.path().join("written");
    let said = said_of(&s, &format!("shell echo x > {}", outside.display()), true);
    assert!(!said[0].starts_with("exit 0"), "{said:?}");
    assert!(!outside.exists());
    for name in ["DYLD_INSERT_LIBRARIES", "LD_PRELOAD", "HTTPS_PROXY", "HOME"] {
        let said = said_of(&s, &format!("shell-env {name}=x echo hi"), true);
        assert!(said[0].starts_with("refused: "), "{name}: {said:?}");
    }
}

/// What a command leaves running, such as a server it started in the
/// background, ends with the run at the latest (where its processes are
/// its own, with the command); the command itself ends when its own
/// process does, though the leftover holds its output open.
#[test]
fn what_a_command_leaves_running_ends_with_the_run() {
    let s = Setup::new();
    let started = std::time::Instant::now();
    // Beats into a file while it lives; a prompt's commands are split at
    // semicolons, so the loop has none.
    let said = said_of(
        &s,
        "shell awk 'BEGIN{while(!system(\"printf . >> beat && sleep 0.02\")){}}' & sleep 0.3 && echo started",
        true,
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(said[0].trim(), "exit 0: started", "{said:?}");
    let beat = s.task.copy().join("beat");
    assert!(beat.exists(), "the background process never beat");
    assert!(
        stops_beating(&beat),
        "the background process outlived the run"
    );
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
            .start(env!("CARGO_BIN_EXE_kule-test-agent"), &args)
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
                &sandbox,
                false,
                &kule_acp::RunControl::new(),
                tx,
            ))
            .unwrap();
        runtime.block_on(rx.collect::<Vec<_>>())
    };
    let taken = run(&["--pictures"]);
    assert!(
        taken.contains(&Event::Message {
            text: "pictures: image/png 12".into(),
            id: None
        }),
        "{taken:?}"
    );
    assert!(!taken.iter().any(|e| matches!(e, Event::Note { .. })));
    let left = run(&[]);
    assert!(
        left.contains(&Event::Message {
            text: "pictures: ".into(),
            id: None
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
            text: prompt.into(),
            id: None
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
        .start(env!("CARGO_BIN_EXE_kule-test-agent"), &[])
        .unwrap();
    let (tx, rx) = mpsc::unbounded();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(kule_acp::run_with(
            &s.task,
            agent,
            &sandbox,
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
            text: "mcp db: npx -y @acme/db@1.2.3 --stdio".into(),
            id: None
        }),
        "{events:?}"
    );
    // Without any, the session is given none.
    let events = s.run("mcp");
    assert!(
        events.contains(&Event::Message {
            text: "mcp ".into(),
            id: None
        }),
        "{events:?}"
    );
}

/// The agent can keep only so many commands going at once in Kule's
/// terminals, each with its threads and output held.
#[test]
fn the_agent_has_only_so_many_terminals_at_once() {
    let s = Setup::new();
    let said = said_of(&s, "terminals 40", true);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("made 32, refused: "), "{said:?}");
    assert!(said[0].contains("32 commands"), "{said:?}");
    // Nor hold the output of more that ended, unreleased.
    let said = said_of(&s, "terminals 300 exit", true);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("made 256, refused: "), "{said:?}");
}

/// A tool call's title is shown and kept as its step's name: cut to a
/// length a person reads.
#[test]
fn a_long_title_is_cut() {
    let s = Setup::new();
    let events = s.run("titled 100000");
    let titles: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            Event::ToolCall { title, .. } | Event::ToolDone { title, .. } => Some(title),
            _ => None,
        })
        .collect();
    assert_eq!(titles.len(), 2, "{events:?}");
    for title in titles {
        assert_eq!(title.chars().count(), 201, "{}", title.len());
        assert!(title.ends_with('…'));
    }
    let steps = s.task.steps().unwrap();
    assert_eq!(steps.last().unwrap().label.chars().count(), 201);
}

/// A command that ended is no longer one the run looks from: its pid may
/// name another process by then, which the run's limits must not slow
/// down or end.
#[test]
fn commands_that_ended_are_no_longer_the_run_s() {
    let s = Setup::new();
    let control = kule_acp::RunControl::new();
    let said = said(&s.run_terminals_with("shell true; shell true", true, &control));
    assert_eq!(said, ["exit 0: ", "exit 0: "], "{said:?}");
    // The agent's own, alone.
    let roots = control.roots();
    assert!(roots.len() <= 1, "{roots:?}");
}
