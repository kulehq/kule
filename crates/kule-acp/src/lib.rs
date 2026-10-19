//! Kule's ACP client.
//!
//! Starts an agent inside a task's sandbox, speaks the Agent Client
//! Protocol (v1) with it over stdio, and turns what it reports into
//! [`Event`]s. Every tool call that ends marks a step, so a person can see
//! which call changed what.
//!
//! Nothing here keeps the agent in check: the sandbox does that, whatever the
//! agent says. ACP is for seeing what happens. Permission requests are
//! answered yes, because the policy decides at commit what reaches the
//! workspace; asking twice would only train people to click through.

mod control;
pub mod governor;
pub mod mcp;
pub mod packages;
mod profile;
pub mod registry;
pub mod terminals;
pub mod trial;

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ClientCapabilities, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, ImageContent, InitializeRequest, KillTerminalRequest,
    KillTerminalResponse, NewSessionRequest, PermissionOptionKind, PromptRequest,
    ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, SessionUpdate, SetSessionModeRequest, TerminalExitStatus,
    TerminalOutputRequest, TerminalOutputResponse, TextContent, ToolCallStatus, ToolKind,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse,
};
use agent_client_protocol::{Agent, ByteStreams, ConnectionTo};
use futures::channel::mpsc::UnboundedSender;
use kule_engine::{RunUsage, Step, Task};
use serde::{Deserialize, Serialize};

pub use control::RunControl;
pub use mcp::McpCommand;
pub use profile::{AGENTS_FILE, Agents, Launch, Listed, Origin, Profile, parse_profiles, profile};

/// Something the agent did, as the person watching sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A piece of the agent's answer.
    Message { text: String },
    /// A piece of the agent's reasoning.
    Thought { text: String },
    /// The agent started a tool call.
    ToolCall { id: String, title: String },
    /// A tool call ended; `step` is what it changed in the copy. Calls that
    /// only read, search, fetch or think take no step.
    ToolDone {
        id: String,
        title: String,
        failed: bool,
        step: Option<Step>,
    },
    /// The agent asked for permission and Kule answered.
    Permission {
        title: String,
        /// The option's name, as the agent words it ("Yes", "Allow").
        chosen: String,
        /// Whether that option lets the call go ahead.
        #[serde(default = "granted_by_default")]
        granted: bool,
    },
    /// The agent's plan, one line per entry.
    Plan { entries: Vec<String> },
    /// What the session has used so far, as the agent reports it: its
    /// context window, and what it cost when the agent says.
    Usage {
        context_used: u64,
        context_size: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<Cost>,
    },
    /// The tokens the turn took, when the agent says at its end.
    Tokens {
        input: u64,
        output: u64,
        #[serde(default)]
        thought: u64,
        #[serde(default)]
        cached_read: u64,
        #[serde(default)]
        cached_write: u64,
    },
    /// The turn ended.
    Done { stop_reason: String },
    /// Something Kule says about the run, not the agent.
    Note { text: String },
    /// What a tool call has shown so far, such as a command's output: the
    /// whole of it, in place of the one before, its last [`MAX_OUTPUT`]
    /// bytes when longer.
    ToolOutput { id: String, text: String },
}

/// The most of one tool call's output kept.
pub const MAX_OUTPUT: usize = 64 << 10;
/// How often output that comes in pieces is passed on.
const PRINT_EVERY: Duration = Duration::from_millis(150);

fn display_terminals() -> serde_json::Map<String, serde_json::Value> {
    let mut meta = serde_json::Map::new();
    meta.insert("terminal_output".into(), true.into());
    meta.insert("terminal_output_delta".into(), true.into());
    meta
}

/// What a tool call's `_meta` says its command printed: all of it so far
/// (`terminal_output`), or the next piece (`terminal_output_delta`).
enum Printed {
    All(String),
    More(String),
}

fn printed(meta: Option<&serde_json::Map<String, serde_json::Value>>) -> Option<Printed> {
    let meta = meta?;
    let data = |key: &str| {
        meta.get(key)
            .and_then(|v| v.get("data"))
            .and_then(|d| d.as_str())
            .map(str::to_owned)
    };
    data("terminal_output")
        .map(Printed::All)
        .or_else(|| data("terminal_output_delta").map(Printed::More))
}

fn exit_status(exit: terminals::Exit) -> TerminalExitStatus {
    TerminalExitStatus::new()
        .exit_code(exit.code)
        .signal(exit.signal)
}

/// The end of `text`, at most [`MAX_OUTPUT`] bytes, cut on a character and
/// marked when cut.
pub(crate) fn tail(text: String) -> String {
    if text.len() <= MAX_OUTPUT {
        return text;
    }
    let mut at = text.len() - MAX_OUTPUT;
    while !text.is_char_boundary(at) {
        at += 1;
    }
    format!("…{}", &text[at..])
}

/// What a tool call shows: its content's text, else what its raw output
/// says it printed. Diffs are left to Review, embedded terminals to their
/// own output.
fn shown(
    content: Option<&[agent_client_protocol::schema::v1::ToolCallContent]>,
    raw: Option<&serde_json::Value>,
) -> Option<String> {
    use agent_client_protocol::schema::v1::ToolCallContent;
    let texts: Vec<String> = content
        .unwrap_or_default()
        .iter()
        .filter_map(|c| match c {
            ToolCallContent::Content(c) => text_of(&c.content),
            _ => None,
        })
        .filter(|t| !t.is_empty())
        .collect();
    if !texts.is_empty() {
        return Some(texts.join("\n"));
    }
    match raw? {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Object(o) => {
            let parts: Vec<&str> = [
                "output",
                "formatted_output",
                "aggregated_output",
                "stdout",
                "stderr",
            ]
            .iter()
            .filter_map(|k| o.get(*k).and_then(|v| v.as_str()))
            .filter(|s| !s.is_empty())
            .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

/// Money, as an agent reports it.
pub type Cost = kule_engine::Money;

impl Event {
    /// Adds what the event says about usage to `usage`. The cost and the
    /// context an agent reports are the session's so far, and a run is one
    /// session, so the last report holds.
    pub fn tally(&self, usage: &mut RunUsage) {
        match self {
            Event::Usage {
                context_used,
                context_size,
                cost,
            } => {
                usage.context_used = *context_used;
                usage.context_size = *context_size;
                if cost.is_some() {
                    usage.cost = cost.clone();
                }
            }
            Event::Tokens {
                input,
                output,
                thought,
                cached_read,
                cached_write,
            } => {
                usage.input_tokens += input;
                usage.output_tokens += output;
                usage.thought_tokens += thought;
                usage.cached_read_tokens += cached_read;
                usage.cached_write_tokens += cached_write;
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn error(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}

/// Runs one prompt: starts `agent` (a command the sandbox prepared), opens an
/// ACP session in the task's copy, sends `prompt`, and reports events until
/// the turn ends. The agent's stderr goes to the task's `agent.log`.
pub async fn run(
    task: &Task,
    agent: Command,
    prompt: &str,
    mode: Option<&str>,
    events: UnboundedSender<Event>,
) -> Result<String, Error> {
    run_limited(task, agent, prompt, mode, None, events).await
}

/// [`run`], stopped once what the agent reports it cost reaches `limit`
/// (in the same currency): the turn is cancelled, as a person cancels it,
/// and the run fails with why.
pub async fn run_limited(
    task: &Task,
    agent: Command,
    prompt: &str,
    mode: Option<&str>,
    limit: Option<Cost>,
    events: UnboundedSender<Event>,
) -> Result<String, Error> {
    run_with(task, agent, prompt, mode, limit, &[], events).await
}

/// [`run_limited`], with MCP servers offered to the agent's session (it
/// starts them itself, in its sandbox).
pub async fn run_with(
    task: &Task,
    agent: Command,
    prompt: &str,
    mode: Option<&str>,
    limit: Option<Cost>,
    mcp: &[McpCommand],
    events: UnboundedSender<Event>,
) -> Result<String, Error> {
    let control = RunControl::new();
    run_pictured(
        task,
        agent,
        prompt,
        &[],
        mode,
        limit,
        mcp,
        None,
        &control,
        events,
    )
    .await
}

/// A picture that goes with a prompt, base64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub mime: String,
    pub data: String,
}

/// [`run_with`], with pictures after the prompt's text for an agent that
/// says it takes pictures; one that does not gets the text alone, and the
/// run notes that the pictures were left out. With `sandbox` (the agent's
/// own), Kule offers to run the agent's commands for it there, so their
/// output comes as they print (see [`terminals`]). `control` stops the run
/// from outside (see [`RunControl::stop`]), and names its processes.
#[allow(clippy::too_many_arguments)]
pub async fn run_pictured(
    task: &Task,
    agent: Command,
    prompt: &str,
    pictures: &[Picture],
    mode: Option<&str>,
    limit: Option<Cost>,
    mcp: &[McpCommand],
    sandbox: Option<kule_engine::Sandbox>,
    control: &RunControl,
    events: UnboundedSender<Event>,
) -> Result<String, Error> {
    let _finish = control::Finish(control.clone());
    // Stopped before it started (a limit already reached, a person).
    if let Some(reason) = control.reason() {
        return Err(Error(reason));
    }
    let strays = sandbox.clone();
    let terminals = sandbox.map(|s| {
        Arc::new(terminals::Terminals::new(
            s,
            task.copy(),
            events.clone(),
            control.clone(),
        ))
    });
    let mcp_servers: Vec<_> = mcp.iter().map(McpCommand::to_acp).collect();
    let log = task.agent_log();
    let log = {
        use std::os::unix::fs::OpenOptionsExt;
        // For its owner alone: what the agent says may hold anything.
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&log)
            .map_err(error)?
    };
    // Through a pipe, not the file itself: the log sits in Kule's home,
    // which the sandbox hides, and the agent must not hold it open.
    let mut child = async_process::Command::from(agent)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error(format!("could not start the agent: {e}")))?;
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut log = futures::io::AllowStdIo::new(log);
            let _ = futures::executor::block_on(futures::io::copy(stderr, &mut log));
        });
    }
    // The sandbox starts the agent in a session of its own; when the run
    // ends, however it ends, everything the agent left running in it goes
    // too, so nothing keeps changing the copy behind the person's review.
    let _group = Group(child.id() as i32, strays);
    control.started_agent(child.id() as i32);
    let stdin = child.stdin.take().ok_or_else(|| error("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| error("no stdout"))?;

    let cwd = task.copy();
    let prompt = prompt.to_owned();
    let pictures = pictures.to_vec();
    let mode = mode.map(str::to_owned);
    let watcher = Watcher {
        task: task.clone(),
        events: events.clone(),
        titles: HashMap::new(),
        kinds: HashMap::new(),
        outputs: HashMap::new(),
        printing: HashMap::new(),
        terminals: terminals.clone(),
        limit: limit.clone(),
        control: control.clone(),
    };
    let mut watcher = Some(watcher);
    let turn_events = events.clone();
    let offers_terminals = terminals.is_some();
    let stopping = control.clone();
    let session = agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                if let Some(watcher) = watcher.as_mut() {
                    watcher.update(notification.update);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let events = events.clone();
                let control = control.clone();
                async move |request: RequestPermissionRequest, responder, _cx| {
                    control.touch();
                    let choice = request
                        .options
                        .iter()
                        .find(|o| matches!(o.kind, PermissionOptionKind::AllowOnce))
                        .or_else(|| {
                            request
                                .options
                                .iter()
                                .find(|o| matches!(o.kind, PermissionOptionKind::AllowAlways))
                        })
                        .or(request.options.first());
                    let title = request.tool_call.fields.title.clone().unwrap_or_default();
                    match choice {
                        Some(option) => {
                            let _ = events.unbounded_send(Event::Permission {
                                title,
                                chosen: option.name.clone(),
                                granted: matches!(
                                    option.kind,
                                    PermissionOptionKind::AllowOnce
                                        | PermissionOptionKind::AllowAlways
                                ),
                            });
                            responder.respond(RequestPermissionResponse::new(
                                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                    option.option_id.clone(),
                                )),
                            ))
                        }
                        None => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        )),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: CreateTerminalRequest, responder, _cx| {
                    let Some(terminals) = &terminals else {
                        return responder.respond_with_internal_error("Kule runs no commands for this agent");
                    };
                    let env: Vec<(String, String)> =
                        request.env.into_iter().map(|v| (v.name, v.value)).collect();
                    match terminals.create(
                        &request.command,
                        &request.args,
                        &env,
                        request.cwd.as_deref(),
                        request.output_byte_limit,
                    ) {
                        Ok(id) => responder.respond(CreateTerminalResponse::new(id)),
                        Err(e) => responder.respond_with_internal_error(e),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: TerminalOutputRequest, responder, _cx| {
                    let Some(terminals) = &terminals else {
                        return responder.respond_with_internal_error("no terminals");
                    };
                    match terminals.output(&request.terminal_id.0) {
                        Ok((text, truncated, exit)) => responder.respond(
                            TerminalOutputResponse::new(text, truncated).exit_status(exit.map(exit_status)),
                        ),
                        Err(e) => responder.respond_with_internal_error(e),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: WaitForTerminalExitRequest, responder, cx: ConnectionTo<Agent>| {
                    let Some(terminals) = terminals.clone() else {
                        return responder.respond_with_internal_error("no terminals");
                    };
                    // Waited for apart, so the connection goes on meanwhile.
                    cx.spawn(async move {
                        match terminals.wait(&request.terminal_id.0).await {
                            Ok(exit) => responder.respond(WaitForTerminalExitResponse::new(exit_status(exit))),
                            Err(e) => responder.respond_with_internal_error(e),
                        }
                    })
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: KillTerminalRequest, responder, _cx| {
                    let Some(terminals) = &terminals else {
                        return responder.respond_with_internal_error("no terminals");
                    };
                    match terminals.kill(&request.terminal_id.0) {
                        Ok(()) => responder.respond(KillTerminalResponse::new()),
                        Err(e) => responder.respond_with_internal_error(e),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let terminals = terminals.clone();
                async move |request: ReleaseTerminalRequest, responder, _cx| {
                    let Some(terminals) = &terminals else {
                        return responder.respond_with_internal_error("no terminals");
                    };
                    match terminals.release(&request.terminal_id.0) {
                        Ok(()) => responder.respond(ReleaseTerminalResponse::new()),
                        Err(e) => responder.respond_with_internal_error(e),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(
            ByteStreams::new(stdin, stdout),
            async move |connection: ConnectionTo<Agent>| {
                let init = connection
                    .send_request(
                        InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
                            ClientCapabilities::new()
                                .terminal(offers_terminals)
                                // The terminal extension of agents that run
                                // their commands themselves (Claude Code's
                                // and Codex's adapters): output in each tool
                                // call's _meta, in chunks as it comes.
                                .meta(display_terminals()),
                        ),
                    )
                    .block_task()
                    .await?;
                let mut blocks = vec![ContentBlock::Text(TextContent::new(prompt))];
                if !pictures.is_empty() {
                    if init.agent_capabilities.prompt_capabilities.image {
                        blocks.push(ContentBlock::Text(TextContent::new(
                            "The pictures that follow are the regions named above, in the order named.",
                        )));
                        blocks.extend(pictures.into_iter().map(|p| {
                            ContentBlock::Image(ImageContent::new(p.data, p.mime))
                        }));
                    } else {
                        let _ = turn_events.unbounded_send(Event::Note {
                            text: "The agent does not take pictures; it got where the regions are, not what they show".into(),
                        });
                    }
                }
                let session = connection
                    .send_request(NewSessionRequest::new(cwd).mcp_servers(mcp_servers))
                    .block_task()
                    .await?;
                if let Some(mode) = mode.filter(|m| {
                    session
                        .modes
                        .as_ref()
                        .is_some_and(|s| s.available_modes.iter().any(|a| a.id.to_string() == *m))
                }) {
                    connection
                        .send_request(SetSessionModeRequest::new(session.session_id.clone(), mode))
                        .block_task()
                        .await?;
                }
                let session_id = session.session_id.clone();
                let prompt = connection
                    .send_request(PromptRequest::new(session.session_id, blocks))
                    .block_task();
                tokio::pin!(prompt);
                let response = tokio::select! {
                    response = &mut prompt => response?,
                    () = stopping.stopped() => {
                        // Stopped (a person, a limit, the budget):
                        // cancelled as a person would, and given a moment
                        // to end the turn.
                        connection.send_notification(CancelNotification::new(session_id))?;
                        match tokio::time::timeout(STOP_GRACE, &mut prompt).await {
                            Ok(response) => response?,
                            Err(_) => return Ok("cancelled".to_owned()),
                        }
                    }
                };
                if let Some(usage) = &response.usage {
                    let _ = turn_events.unbounded_send(Event::Tokens {
                        input: usage.input_tokens,
                        output: usage.output_tokens,
                        thought: usage.thought_tokens.unwrap_or(0),
                        cached_read: usage.cached_read_tokens.unwrap_or(0),
                        cached_write: usage.cached_write_tokens.unwrap_or(0),
                    });
                }
                Ok(format!("{:?}", response.stop_reason).to_lowercase())
            },
        );
    // Stopped before the turn began (starting, or a slow session), or an
    // agent that does not end its turn: given the same moment, then left.
    let stopped_early = async {
        control.stopped().await;
        tokio::time::sleep(STOP_GRACE + Duration::from_secs(2)).await;
    };
    let outcome = tokio::select! {
        outcome = session => outcome.map_err(|e| Error(format!("ACP: {e}"))),
        () = stopped_early => Ok("cancelled".to_owned()),
    };
    let _ = child.kill();
    // Stopped, the run fails with why; also when the agent died of it (a
    // limit kills at once). An agent may report the cost only as the turn
    // ends (Claude Code does): then the turn is over before the cancel,
    // nothing was stopped, and the next run is the one refused.
    let cancelled = match &outcome {
        Ok(stop) => stop == "cancelled",
        Err(_) => true,
    };
    if let Some(reason) = control.reason()
        && cancelled
    {
        let _ = events.unbounded_send(Event::Done {
            stop_reason: "cancelled".to_owned(),
        });
        return Err(Error(reason));
    }
    let stop = outcome?;
    let _ = events.unbounded_send(Event::Done {
        stop_reason: stop.clone(),
    });
    Ok(stop)
}

/// How long a stopped agent has to end its turn.
const STOP_GRACE: Duration = Duration::from_secs(10);

/// The agent's process group, killed when dropped, and with the sandbox,
/// whatever left the group but still runs in the task's sandbox.
struct Group(i32, Option<kule_engine::Sandbox>);

impl Drop for Group {
    fn drop(&mut self) {
        if self.0 > 0 {
            // SAFETY: kill only sends a signal; a negative pid names the
            // group the agent leads.
            unsafe {
                libc::kill(-self.0, libc::SIGKILL);
            }
        }
        // What it cannot end now, a commit ends or refuses over.
        if let Some(sandbox) = &self.1 {
            let _ = sandbox.end_strays();
        }
    }
}

/// Turns session updates into events, and ended tool calls into steps.
struct Watcher {
    task: Task,
    events: UnboundedSender<Event>,
    titles: HashMap<String, String>,
    kinds: HashMap<String, ToolKind>,
    /// What each tool call showed last, so an unchanged one is not sent again.
    outputs: HashMap<String, String>,
    /// For a call printing in pieces: all it printed, and when it was last
    /// passed on.
    printing: HashMap<String, (String, Instant)>,
    /// The commands Kule runs for the agent, whose output goes to the tool
    /// call that shows them.
    terminals: Option<Arc<terminals::Terminals>>,
    /// The cost the run may reach; the run is stopped when it does.
    limit: Option<Cost>,
    control: RunControl,
}

impl Watcher {
    fn update(&mut self, update: SessionUpdate) {
        self.control.touch();
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let Some(text) = text_of(&chunk.content) {
                    self.send(Event::Message { text });
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let Some(text) = text_of(&chunk.content) {
                    self.send(Event::Thought { text });
                }
            }
            SessionUpdate::ToolCall(call) => {
                let id = call.tool_call_id.to_string();
                self.titles.insert(id.clone(), call.title.clone());
                self.kinds.insert(id.clone(), call.kind);
                self.send(Event::ToolCall {
                    id: id.clone(),
                    title: call.title,
                });
                self.attach(&id, &call.content);
                self.output(&id, shown(Some(&call.content), call.raw_output.as_ref()));
                self.print(&id, printed(call.meta.as_ref()));
                self.maybe_done(id, call.status);
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let id = update.tool_call_id.to_string();
                if let Some(title) = update.fields.title {
                    self.titles.insert(id.clone(), title);
                }
                if let Some(kind) = update.fields.kind {
                    self.kinds.insert(id.clone(), kind);
                }
                if let Some(content) = &update.fields.content {
                    self.attach(&id, content);
                }
                if update.fields.content.is_some() || update.fields.raw_output.is_some() {
                    let text = shown(
                        update.fields.content.as_deref(),
                        update.fields.raw_output.as_ref(),
                    );
                    self.output(&id, text);
                }
                self.print(&id, printed(update.meta.as_ref()));
                if let Some(status) = update.fields.status {
                    self.maybe_done(id, status);
                }
            }
            SessionUpdate::Plan(plan) => {
                let entries = plan.entries.into_iter().map(|e| e.content).collect();
                self.send(Event::Plan { entries });
            }
            SessionUpdate::UsageUpdate(usage) => {
                let cost = usage.cost.map(|c| Cost {
                    amount: c.amount,
                    currency: c.currency,
                });
                let reached = match (&self.limit, &cost) {
                    (Some(limit), Some(cost)) => {
                        cost.currency == limit.currency && cost.amount >= limit.amount
                    }
                    _ => false,
                };
                self.send(Event::Usage {
                    context_used: usage.used,
                    context_size: usage.size,
                    cost,
                });
                if reached && let Some(limit) = &self.limit {
                    self.control.stop(format!(
                        "stopped: what the agent reported it cost reached what was left of the task's budget, {:.2} {}",
                        limit.amount, limit.currency
                    ));
                }
            }
            _ => {}
        }
    }

    /// A command Kule runs, shown in tool call `id`: its output goes there.
    fn attach(&mut self, id: &str, content: &[agent_client_protocol::schema::v1::ToolCallContent]) {
        use agent_client_protocol::schema::v1::ToolCallContent;
        let Some(terminals) = &self.terminals else {
            return;
        };
        for c in content {
            if let ToolCallContent::Terminal(t) = c {
                terminals.attach(&t.terminal_id.0, id);
            }
        }
    }

    /// Output from the terminal extension: the whole of it, or a piece
    /// that goes after what came before.
    /// A command that prints in many small pieces is passed on at most
    /// every [`PRINT_EVERY`]; what is left goes when its call ends.
    fn print(&mut self, id: &str, printed: Option<Printed>) {
        let last = self.printing.get(id).map(|(_, at)| *at);
        let text = match printed {
            None => return,
            Some(Printed::All(text)) => text,
            Some(Printed::More(piece)) => {
                let mut text = match self.printing.get(id) {
                    Some((held, _)) => held.clone(),
                    None => self.outputs.get(id).cloned().unwrap_or_default(),
                };
                text.push_str(&piece);
                tail(text)
            }
        };
        match last {
            Some(at) if at.elapsed() < PRINT_EVERY => {
                self.printing.insert(id.to_owned(), (text, at));
            }
            _ => {
                self.output(id, Some(text.clone()));
                self.printing.insert(id.to_owned(), (text, Instant::now()));
            }
        }
    }

    /// What a call printed and was held back, passed on as it ends.
    fn flush(&mut self, id: &str) {
        if let Some((text, _)) = self.printing.remove(id) {
            self.output(id, Some(text));
        }
    }

    /// Passes on what a tool call shows now, when it is new.
    fn output(&mut self, id: &str, text: Option<String>) {
        let Some(text) = text.map(tail) else { return };
        if self.outputs.get(id) == Some(&text) {
            return;
        }
        self.outputs.insert(id.to_owned(), text.clone());
        self.send(Event::ToolOutput {
            id: id.to_owned(),
            text,
        });
    }

    fn maybe_done(&mut self, id: String, status: ToolCallStatus) {
        let failed = match status {
            ToolCallStatus::Completed => false,
            ToolCallStatus::Failed => true,
            _ => return,
        };
        self.flush(&id);
        let title = self.titles.get(&id).cloned().unwrap_or_default();
        // A call that only looks or thinks takes no step: what changed while
        // it ran belongs to a call that can change files and is still going,
        // such as an edit waiting on a review.
        let looks = matches!(
            self.kinds.get(&id),
            Some(
                ToolKind::Read
                    | ToolKind::Search
                    | ToolKind::Think
                    | ToolKind::Fetch
                    | ToolKind::SwitchMode
            )
        );
        if looks {
            self.send(Event::ToolDone {
                id,
                title,
                failed,
                step: None,
            });
            return;
        }
        match self.task.step(&title) {
            Ok(step) => self.send(Event::ToolDone {
                id,
                title,
                failed,
                step: Some(step),
            }),
            Err(e) => self.send(Event::Message {
                text: format!("\n[kule: could not record the step: {e}]\n"),
            }),
        }
    }

    fn send(&self, event: Event) {
        let _ = self.events.unbounded_send(event);
    }
}

fn text_of(content: &ContentBlock) -> Option<String> {
    match content {
        ContentBlock::Text(text) => Some(text.text.clone()),
        _ => None,
    }
}

/// Events kept before `granted` was recorded were all answered with an
/// allowing option when the agent offered one.
fn granted_by_default() -> bool {
    true
}
