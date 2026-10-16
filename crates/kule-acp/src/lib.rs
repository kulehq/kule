//! Kule's ACP client.
//!
//! Starts a coding agent inside a task's sandbox, speaks the Agent Client
//! Protocol (v1) with it over stdio, and turns what it reports into
//! [`Event`]s. Every tool call that ends marks a step, so a person can see
//! which call changed what.
//!
//! Nothing here keeps the agent in check: the sandbox does that, whatever the
//! agent says. ACP is for seeing what happens. Permission requests are
//! answered yes, because the policy decides at commit what reaches the
//! workspace; asking twice would only train people to click through.

mod profile;

use std::collections::HashMap;
use std::process::{Command, Stdio};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, TextContent, ToolCallStatus,
    ToolKind,
};
use agent_client_protocol::{Agent, ByteStreams, ConnectionTo};
use futures::channel::mpsc::UnboundedSender;
use kule_engine::{Step, Task};
use serde::Serialize;

pub use profile::{Launch, PROFILES, Profile, profile};

/// Something the agent did, as the person watching sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    Permission { title: String, chosen: String },
    /// The agent's plan, one line per entry.
    Plan { entries: Vec<String> },
    /// The turn ended.
    Done { stop_reason: String },
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
    events: UnboundedSender<Event>,
) -> Result<String, Error> {
    let log = task.copy().with_file_name("agent.log");
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(error)?;
    let mut child = async_process::Command::from(agent)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error(format!("could not start the agent: {e}")))?;
    let stdin = child.stdin.take().ok_or_else(|| error("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| error("no stdout"))?;

    let cwd = task.copy();
    let prompt = prompt.to_owned();
    let watcher = Watcher {
        task: task.clone(),
        events: events.clone(),
        titles: HashMap::new(),
        kinds: HashMap::new(),
    };
    let mut watcher = Some(watcher);
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
                    .send_request(NewSessionRequest::new(cwd))
                    .block_task()
                    .await?;
                let response = connection
                    .send_request(PromptRequest::new(
                        session.session_id,
                        vec![ContentBlock::Text(TextContent::new(prompt))],
                    ))
                    .block_task()
                    .await?;
                Ok(format!("{:?}", response.stop_reason).to_lowercase())
            },
        )
        .await
        .map_err(|e| Error(format!("ACP: {e}")))?;
    let _ = events.unbounded_send(Event::Done {
        stop_reason: stop.clone(),
    });
    let _ = child.kill();
    Ok(stop)
}

/// Turns session updates into events, and ended tool calls into steps.
struct Watcher {
    task: Task,
    events: UnboundedSender<Event>,
    titles: HashMap<String, String>,
    kinds: HashMap<String, ToolKind>,
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
