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

pub mod mcp;
pub mod packages;
mod profile;
pub mod registry;
pub mod trial;

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, SetSessionModeRequest,
    TextContent, ToolCallStatus, ToolKind,
};
use agent_client_protocol::{Agent, ByteStreams, ConnectionTo};
use futures::channel::mpsc::UnboundedSender;
use kule_engine::{RunUsage, Step, Task};
use serde::{Deserialize, Serialize};

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
    let mcp_servers: Vec<_> = mcp.iter().map(McpCommand::to_acp).collect();
    let log = task.copy().with_file_name("agent.log");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(error)?;
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
    let _group = Group(child.id() as i32);
    let stdin = child.stdin.take().ok_or_else(|| error("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| error("no stdout"))?;

    let cwd = task.copy();
    let prompt = prompt.to_owned();
    let mode = mode.map(str::to_owned);
    let over = Arc::new(tokio::sync::Notify::new());
    let stopped = Arc::new(AtomicBool::new(false));
    let watcher = Watcher {
        task: task.clone(),
        events: events.clone(),
        titles: HashMap::new(),
        kinds: HashMap::new(),
        limit: limit.clone(),
        over: Arc::clone(&over),
        stopped: Arc::clone(&stopped),
    };
    let mut watcher = Some(watcher);
    let turn_events = events.clone();
    let stop = agent_client_protocol::Client
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
                async move |request: RequestPermissionRequest, responder, _cx| {
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
        .connect_with(
            ByteStreams::new(stdin, stdout),
            async move |connection: ConnectionTo<Agent>| {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
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
                    .send_request(PromptRequest::new(
                        session.session_id,
                        vec![ContentBlock::Text(TextContent::new(prompt))],
                    ))
                    .block_task();
                tokio::pin!(prompt);
                let response = tokio::select! {
                    response = &mut prompt => response?,
                    () = over.notified() => {
                        // Over the budget: cancelled as a person would, and
                        // given a moment to end the turn.
                        connection.send_notification(CancelNotification::new(session_id))?;
                        match tokio::time::timeout(Duration::from_secs(10), &mut prompt).await {
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
        )
        .await
        .map_err(|e| Error(format!("ACP: {e}")))?;
    let _ = events.unbounded_send(Event::Done {
        stop_reason: stop.clone(),
    });
    let _ = child.kill();
    // An agent may report the cost only as the turn ends (Claude Code
    // does): then the turn is over before the cancel, nothing was stopped,
    // and the next run is the one refused.
    if stopped.load(Ordering::SeqCst)
        && stop == "cancelled"
        && let Some(limit) = &limit
    {
        return Err(Error(format!(
            "stopped: what the agent reported it cost reached what was left of the task's budget, {:.2} {}",
            limit.amount, limit.currency
        )));
    }
    Ok(stop)
}

/// The agent's process group, killed when dropped.
struct Group(i32);

impl Drop for Group {
    fn drop(&mut self) {
        if self.0 > 0 {
            // SAFETY: kill only sends a signal; a negative pid names the
            // group the agent leads.
            unsafe {
                libc::kill(-self.0, libc::SIGKILL);
            }
        }
    }
}

/// Turns session updates into events, and ended tool calls into steps.
struct Watcher {
    task: Task,
    events: UnboundedSender<Event>,
    titles: HashMap<String, String>,
    kinds: HashMap<String, ToolKind>,
    /// The cost the run may reach; `over` is told, once, when it does.
    limit: Option<Cost>,
    over: Arc<tokio::sync::Notify>,
    stopped: Arc<AtomicBool>,
}

impl Watcher {
    fn update(&mut self, update: SessionUpdate) {
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
                if reached && !self.stopped.swap(true, Ordering::SeqCst) {
                    self.over.notify_one();
                }
            }
            _ => {}
        }
    }

    fn maybe_done(&mut self, id: String, status: ToolCallStatus) {
        let failed = match status {
            ToolCallStatus::Completed => false,
            ToolCallStatus::Failed => true,
            _ => return,
        };
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
