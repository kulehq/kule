//! A scripted ACP agent for Kule's tests: no model, no network, the same
//! protocol a real agent speaks. The prompt is a list of commands separated
//! by `;`:
//!
//! - `write <path> <text>`: a tool call that writes the file, relative to the
//!   session's working directory unless absolute;
//! - `say <text>`: a message;
//! - `sleep <ms>`: waits, the way a real agent waits for its model;
//! - `ask`: asks the client for permission and says what it chose;
//! - `ask-reject-only`: the same, with no option that allows;
//! - `background <file>`: leaves a process running that adds a byte to the
//!   file every few milliseconds;
//! - `reviewed-write <path> <text>`: an edit that writes the file while a
//!   review (a `think` call) runs inside it, the review ending first, as
//!   Codex does;
//! - `cost <amount>`: reports what the session cost so far, in USD;
//! - `tokens <input> <output>`: the token counts the turn's end reports;
//! - `mcp`: a message naming the MCP servers the session was given, as
//!   `name: command args`, separated by ` | `;
//! - `run <text>`: a command's tool call that shows `<text>` (`\n` for a
//!   line break) in two updates, half and then all; `run-raw <text>` the
//!   same as raw output; `run-big <n>`: the same with `n` bytes of `x`;
//! - `stream <a> <b> …`: a command the agent runs itself, its output (each
//!   word a line) reported in `_meta` piece by piece to a client that
//!   asks for `terminal_output_delta`, as text content at the end otherwise;
//! - `shell <script>`: has the client run `sh -c <script>` in a terminal
//!   (ACP `terminal/*`) shown in a tool call, and says `exit <code>:
//!   <output>`, `refused: <why>`, or `no terminals` when the client offers
//!   none; `shell-env NAME=value <script>` asks for the variable too;
//! - `terminals <n>`: has the client start `n` commands in terminals at
//!   once, none of them released, and says `made <k>`, then `, refused:
//!   <why>` when the client refused one; `terminals <n> exit`: the same,
//!   each waited for until it exits;
//! - `calls <n> <open|done|late>`: `n` tool calls that read, left open,
//!   each ended, or each ended and then shown more; `titled <n>`: a
//!   command's tool call titled with `n` bytes;
//! - `echo`: a message of the whole prompt as it came, and nothing after
//!   it is run (what Kule adds to a prompt is not a command);
//! - `pictures`: a message naming the pictures that came with the prompt,
//!   as `mime length`, separated by `, `. Started with `--pictures`, the
//!   agent says it takes pictures; without, it says it does not.
//!
//! A cancelled turn stops before its next command and ends as cancelled.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ContentBlock, ContentChunk, Cost, CreateTerminalRequest,
    EnvVariable, InitializeRequest, InitializeResponse, McpServer, MessageId, NewSessionRequest,
    NewSessionResponse, PermissionOption, PermissionOptionKind, PromptCapabilities, PromptRequest,
    PromptResponse, ReleaseTerminalRequest, RequestPermissionOutcome, RequestPermissionRequest,
    SessionId, SessionNotification, SessionUpdate, StopReason, Terminal, TerminalOutputRequest,
    TextContent, ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind, Usage, UsageUpdate, WaitForTerminalExitRequest,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Responder, Stdio};

#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    let cwd: Arc<Mutex<PathBuf>> = Arc::new(Mutex::new(PathBuf::from(".")));
    let session_cwd = cwd.clone();
    let mcp: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let session_mcp = mcp.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let on_cancel = cancelled.clone();
    // Whether the client offered to run commands (`terminal/*`).
    let terminals = Arc::new(AtomicBool::new(false));
    let offered = terminals.clone();
    // Whether the client renders output sent in tool calls' _meta.
    let deltas = Arc::new(AtomicBool::new(false));
    let renders = deltas.clone();
    Agent
        .builder()
        .name("kule-test-agent")
        .on_receive_notification(
            async move |_: CancelNotification, _cx| {
                on_cancel.store(true, Ordering::SeqCst);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: InitializeRequest, responder, _cx| {
                // `--pictures` makes it an agent that takes pictures.
                let pictures = std::env::args().any(|a| a == "--pictures");
                offered.store(request.client_capabilities.terminal, Ordering::SeqCst);
                let delta = request
                    .client_capabilities
                    .meta
                    .as_ref()
                    .and_then(|m| m.get("terminal_output_delta"))
                    .and_then(|v| v.as_bool());
                renders.store(delta == Some(true), Ordering::SeqCst);
                responder.respond(
                    InitializeResponse::new(request.protocol_version).agent_capabilities(
                        AgentCapabilities::new()
                            .prompt_capabilities(PromptCapabilities::new().image(pictures)),
                    ),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: NewSessionRequest, responder, _cx| {
                *session_cwd.lock().unwrap() = request.cwd.clone();
                *session_mcp.lock().unwrap() = request
                    .mcp_servers
                    .iter()
                    .filter_map(|s| match s {
                        McpServer::Stdio(s) => Some(format!(
                            "{}: {} {}",
                            s.name,
                            s.command.display(),
                            s.args.join(" ")
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                responder.respond(NewSessionResponse::new(SessionId::new("test-session")))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: PromptRequest,
                        responder: Responder<PromptResponse>,
                        cx: ConnectionTo<Client>| {
                // The first text block is the script; what follows it is
                // what Kule sent with it.
                let script = request
                    .prompt
                    .iter()
                    .find_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let pictures = request
                    .prompt
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Image(image) => {
                            Some(format!("{} {}", image.mime_type, image.data.len()))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let dir = cwd.lock().unwrap().clone();
                let servers = mcp.lock().unwrap().clone();
                let session = request.session_id.clone();
                let connection = cx.clone();
                let cancelled = cancelled.clone();
                let terminals = terminals.clone();
                let deltas = deltas.clone();
                cx.spawn(async move {
                    let mut usage = None;
                    for (n, command) in script.split(';').map(str::trim).enumerate() {
                        if cancelled.load(Ordering::SeqCst) {
                            return responder.respond(PromptResponse::new(StopReason::Cancelled));
                        }
                        if let Some(counts) = command.strip_prefix("tokens ") {
                            let mut counts = counts.split(' ').map(|n| n.parse().unwrap_or(0));
                            let (input, output) =
                                (counts.next().unwrap_or(0), counts.next().unwrap_or(0));
                            usage = Some(Usage::new(input + output, input, output));
                            continue;
                        }
                        if command.split_whitespace().next() == Some("echo") {
                            let said = format!("say {script}");
                            perform(&connection, &session, &dir, n, &said).await?;
                            break;
                        }
                        if command == "pictures" {
                            let said = format!("say pictures: {pictures}");
                            perform(&connection, &session, &dir, n, &said).await?;
                            continue;
                        }
                        if command == "mcp" {
                            let said = format!("say mcp {servers}");
                            perform(&connection, &session, &dir, n, &said).await?;
                            continue;
                        }
                        // Says nothing for a while, or runs a command of
                        // its own (a long build), and stops either when
                        // cancelled, as a real agent does.
                        let quiet = if let Some(ms) = command.strip_prefix("wait ") {
                            Some((ms.parse().unwrap_or(100), None))
                        } else {
                            command.strip_prefix("exec ").map(|script| {
                                let child = std::process::Command::new("/bin/sh")
                                    .args(["-c", script])
                                    .current_dir(&dir)
                                    .stdin(std::process::Stdio::null())
                                    .spawn()
                                    .ok();
                                (u64::MAX, child)
                            })
                        };
                        if let Some((ms, mut child)) = quiet {
                            let until = std::time::Instant::now()
                                + std::time::Duration::from_millis(ms.min(3_600_000));
                            while std::time::Instant::now() < until {
                                if cancelled.load(Ordering::SeqCst) {
                                    if let Some(child) = child.as_mut() {
                                        let _ = child.kill();
                                        let _ = child.wait();
                                    }
                                    return responder
                                        .respond(PromptResponse::new(StopReason::Cancelled));
                                }
                                if let Some(c) = child.as_mut()
                                    && !matches!(c.try_wait(), Ok(None))
                                {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                            }
                            continue;
                        }
                        if let Some(pieces) = command.strip_prefix("stream ") {
                            let deltas = deltas.load(Ordering::SeqCst);
                            stream(&connection, &session, n, pieces, deltas).await?;
                            continue;
                        }
                        if let Some(rest) = command.strip_prefix("terminals ") {
                            let (count, how) = rest.split_once(' ').unwrap_or((rest, ""));
                            let said = if terminals.load(Ordering::SeqCst) {
                                open_terminals(
                                    &connection,
                                    &session,
                                    count.parse().unwrap_or(0),
                                    how == "exit",
                                )
                                .await
                            } else {
                                "no terminals".to_owned()
                            };
                            perform(&connection, &session, &dir, n, &format!("say {said}")).await?;
                            continue;
                        }
                        if command.starts_with("shell ") || command.starts_with("shell-env ") {
                            let said = if terminals.load(Ordering::SeqCst) {
                                shell(&connection, &session, n, command).await?
                            } else {
                                "no terminals".to_owned()
                            };
                            perform(&connection, &session, &dir, n, &format!("say {said}")).await?;
                            continue;
                        }
                        perform(&connection, &session, &dir, n, command).await?;
                    }
                    responder.respond(PromptResponse::new(StopReason::EndTurn).usage(usage))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(Stdio::new())
        .await
}

/// A command the agent runs itself, its output reported as Claude Code's
/// and Codex's adapters report it: to a client that renders it, each
/// space-separated piece as a line in `_meta.terminal_output_delta`; to one
/// that does not, all of it as text content at the end.
async fn stream(
    cx: &ConnectionTo<Client>,
    session: &SessionId,
    n: usize,
    pieces: &str,
    deltas: bool,
) -> agent_client_protocol::Result<()> {
    let id = format!("call-{n}");
    let meta = |pairs: serde_json::Value| pairs.as_object().cloned();
    let mut call = ToolCall::new(id.clone(), format!("stream {n}"))
        .kind(ToolKind::Execute)
        .status(ToolCallStatus::InProgress);
    if deltas {
        call = call
            .content(vec![ToolCallContent::Terminal(Terminal::new(id.clone()))])
            .meta(meta(
                serde_json::json!({ "terminal_info": { "terminal_id": id } }),
            ));
    }
    cx.send_notification(SessionNotification::new(
        session.clone(),
        SessionUpdate::ToolCall(call),
    ))?;
    let mut all = String::new();
    for piece in pieces.split(' ') {
        let line = format!("{piece}\n");
        all.push_str(&line);
        if deltas {
            cx.send_notification(SessionNotification::new(
                session.clone(),
                SessionUpdate::ToolCallUpdate(
                    ToolCallUpdate::new(id.clone(), ToolCallUpdateFields::new()).meta(meta(
                        serde_json::json!({
                            "terminal_output_delta": { "terminal_id": id, "data": line }
                        }),
                    )),
                ),
            ))?;
        }
    }
    let mut done = ToolCallUpdate::new(
        id.clone(),
        ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
    );
    if deltas {
        done = done.meta(meta(serde_json::json!({
            "terminal_exit": { "terminal_id": id, "exit_code": 0, "signal": null }
        })));
    } else {
        done.fields.content = Some(vec![ToolCallContent::from(ContentBlock::Text(
            TextContent::new(all),
        ))]);
    }
    cx.send_notification(SessionNotification::new(
        session.clone(),
        SessionUpdate::ToolCallUpdate(done),
    ))
}

/// Has the client run `sh -c <script>` (after `shell-env`, a `NAME=value`
/// first) in a terminal shown in a tool call; returns what to say: how it
/// ended and what it printed, or why the client refused.
async fn shell(
    cx: &ConnectionTo<Client>,
    session: &SessionId,
    n: usize,
    command: &str,
) -> agent_client_protocol::Result<String> {
    let (verb, rest) = command.split_once(' ').unwrap_or((command, ""));
    let (env, script) = if verb == "shell-env" {
        let (pair, script) = rest.split_once(' ').unwrap_or((rest, ""));
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (vec![EnvVariable::new(name, value)], script)
    } else {
        (Vec::new(), rest)
    };
    let id = format!("call-{n}");
    cx.send_notification(SessionNotification::new(
        session.clone(),
        SessionUpdate::ToolCall(
            ToolCall::new(id.clone(), format!("shell {n}"))
                .kind(ToolKind::Execute)
                .status(ToolCallStatus::InProgress),
        ),
    ))?;
    let created = cx
        .send_request(
            CreateTerminalRequest::new(session.clone(), "/bin/sh")
                .args(vec!["-c".to_owned(), script.to_owned()])
                .env(env),
        )
        .block_task()
        .await;
    let terminal = match created {
        Ok(created) => created.terminal_id,
        Err(e) => return Ok(format!("refused: {}", e.message)),
    };
    cx.send_notification(SessionNotification::new(
        session.clone(),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            id.clone(),
            ToolCallUpdateFields::new().content(vec![ToolCallContent::Terminal(Terminal::new(
                terminal.clone(),
            ))]),
        )),
    ))?;
    let exit = cx
        .send_request(WaitForTerminalExitRequest::new(
            session.clone(),
            terminal.clone(),
        ))
        .block_task()
        .await?
        .exit_status;
    let output = cx
        .send_request(TerminalOutputRequest::new(
            session.clone(),
            terminal.clone(),
        ))
        .block_task()
        .await?;
    cx.send_request(ReleaseTerminalRequest::new(session.clone(), terminal))
        .block_task()
        .await?;
    let status = if exit.exit_code == Some(0) {
        ToolCallStatus::Completed
    } else {
        ToolCallStatus::Failed
    };
    cx.send_notification(SessionNotification::new(
        session.clone(),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            id,
            ToolCallUpdateFields::new().status(status),
        )),
    ))?;
    Ok(format!(
        "exit {}: {}",
        exit.exit_code.map_or("none".to_owned(), |c| c.to_string()),
        output.output.trim_end()
    ))
}

/// Has the client start `count` commands, released never: commands that
/// sleep, or with `exit` that end, each waited for; returns what to say:
/// how many it made, and why it refused one.
async fn open_terminals(
    cx: &ConnectionTo<Client>,
    session: &SessionId,
    count: usize,
    exit: bool,
) -> String {
    let script = if exit { "true" } else { "sleep 30" };
    for made in 0..count {
        let created = cx
            .send_request(
                CreateTerminalRequest::new(session.clone(), "/bin/sh")
                    .args(vec!["-c".to_owned(), script.to_owned()]),
            )
            .block_task()
            .await;
        let terminal = match created {
            Ok(created) => created.terminal_id,
            Err(e) => return format!("made {made}, refused: {e}"),
        };
        if exit {
            let _ = cx
                .send_request(WaitForTerminalExitRequest::new(session.clone(), terminal))
                .block_task()
                .await;
        }
    }
    format!("made {count}")
}

async fn perform(
    cx: &ConnectionTo<Client>,
    session: &SessionId,
    dir: &Path,
    n: usize,
    command: &str,
) -> agent_client_protocol::Result<()> {
    let say = |text: String| {
        cx.send_notification(SessionNotification::new(
            session.clone(),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new(text),
            ))),
        ))
    };
    let (verb, rest) = command.split_once(' ').unwrap_or((command, ""));
    match verb {
        "write" => {
            let (path, text) = rest.split_once(' ').unwrap_or((rest, ""));
            let id = format!("call-{n}");
            cx.send_notification(SessionNotification::new(
                session.clone(),
                SessionUpdate::ToolCall(
                    ToolCall::new(id.clone(), format!("write {path}"))
                        .status(ToolCallStatus::InProgress),
                ),
            ))?;
            let target = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                dir.join(path)
            };
            let status = match std::fs::write(&target, format!("{text}\n")) {
                Ok(()) => ToolCallStatus::Completed,
                Err(_) => ToolCallStatus::Failed,
            };
            cx.send_notification(SessionNotification::new(
                session.clone(),
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    id,
                    ToolCallUpdateFields::new().status(status),
                )),
            ))
        }
        "reviewed-write" => {
            let (path, text) = rest.split_once(' ').unwrap_or((rest, ""));
            let (edit, review) = (format!("call-{n}"), format!("call-{n}-review"));
            let start = |id: &str, title: String, kind: ToolKind| {
                cx.send_notification(SessionNotification::new(
                    session.clone(),
                    SessionUpdate::ToolCall(
                        ToolCall::new(id.to_owned(), title)
                            .kind(kind)
                            .status(ToolCallStatus::InProgress),
                    ),
                ))
            };
            let end = |id: &str| {
                cx.send_notification(SessionNotification::new(
                    session.clone(),
                    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        id.to_owned(),
                        ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
                    )),
                ))
            };
            start(&edit, format!("edit {path}"), ToolKind::Edit)?;
            start(&review, "review".to_owned(), ToolKind::Think)?;
            let _ = std::fs::write(dir.join(path), format!("{text}\n"));
            end(&review)?;
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            end(&edit)
        }
        "say" => say(rest.to_owned()),
        // A piece of message ID: `say-in m1 hello`.
        "say-in" => {
            let (id, text) = rest.split_once(' ').unwrap_or((rest, ""));
            cx.send_notification(SessionNotification::new(
                session.clone(),
                SessionUpdate::AgentMessageChunk(
                    ContentChunk::new(ContentBlock::Text(TextContent::new(text.to_owned())))
                        .message_id(MessageId::new(id.to_owned())),
                ),
            ))
        }
        "run" | "run-raw" | "run-big" => {
            // A command's tool call, its output shown in two updates: half,
            // then all of it (as content, or for `run-raw` as raw output).
            let text = match verb {
                "run-big" => "x".repeat(rest.parse().unwrap_or(0)),
                _ => rest.replace("\\n", "\n"),
            };
            let id = format!("call-{n}");
            cx.send_notification(SessionNotification::new(
                session.clone(),
                SessionUpdate::ToolCall(
                    ToolCall::new(id.clone(), format!("run {n}"))
                        .kind(ToolKind::Execute)
                        .status(ToolCallStatus::InProgress),
                ),
            ))?;
            let half: String = text.chars().take(text.chars().count() / 2).collect();
            for (part, status) in [
                (half, ToolCallStatus::InProgress),
                (text.clone(), ToolCallStatus::Completed),
            ] {
                let fields = if verb == "run-raw" {
                    ToolCallUpdateFields::new()
                        .raw_output(serde_json::json!({ "stdout": part, "exit_code": 0 }))
                } else {
                    ToolCallUpdateFields::new().content(vec![ToolCallContent::from(
                        ContentBlock::Text(TextContent::new(part)),
                    )])
                };
                cx.send_notification(SessionNotification::new(
                    session.clone(),
                    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        id.clone(),
                        fields.status(status),
                    )),
                ))?;
            }
            Ok(())
        }
        "calls" => {
            let (count, how) = rest.split_once(' ').unwrap_or((rest, "open"));
            for k in 0..count.parse().unwrap_or(0) {
                let id = format!("call-{n}-{k}");
                cx.send_notification(SessionNotification::new(
                    session.clone(),
                    SessionUpdate::ToolCall(
                        ToolCall::new(id.clone(), format!("read {k}"))
                            .kind(ToolKind::Read)
                            .status(ToolCallStatus::InProgress),
                    ),
                ))?;
                if how == "done" || how == "late" {
                    cx.send_notification(SessionNotification::new(
                        session.clone(),
                        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                            id.clone(),
                            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
                        )),
                    ))?;
                }
                if how == "late" {
                    cx.send_notification(SessionNotification::new(
                        session.clone(),
                        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                            id,
                            ToolCallUpdateFields::new().content(vec![ToolCallContent::from(
                                ContentBlock::Text(TextContent::new("more".to_owned())),
                            )]),
                        )),
                    ))?;
                }
            }
            Ok(())
        }
        "titled" => {
            let id = format!("call-{n}");
            cx.send_notification(SessionNotification::new(
                session.clone(),
                SessionUpdate::ToolCall(
                    ToolCall::new(id.clone(), "x".repeat(rest.parse().unwrap_or(0)))
                        .kind(ToolKind::Execute)
                        .status(ToolCallStatus::InProgress),
                ),
            ))?;
            cx.send_notification(SessionNotification::new(
                session.clone(),
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    id,
                    ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
                )),
            ))
        }
        "cost" => cx.send_notification(SessionNotification::new(
            session.clone(),
            SessionUpdate::UsageUpdate(
                UsageUpdate::new(1000, 200_000).cost(Cost::new(rest.parse().unwrap_or(0.0), "USD")),
            ),
        )),
        // A real agent waits for its model between tool calls.
        // Leaves a process running after the turn, the way an agent's
        // `npm run dev &` would; it beats into the file while it lives (its
        // pid would say nothing outside the sandbox's own process space).
        "background" => {
            let _ = std::process::Command::new("sh")
                .args([
                    "-c",
                    &format!("while :; do printf . >> {rest}; sleep 0.02; done"),
                ])
                .current_dir(dir)
                .spawn();
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            Ok(())
        }
        // Leaves a process that left the agent's process group, as one
        // started with `setsid` or `nohup ... &` from another session would.
        "stray" => {
            use std::os::unix::process::CommandExt;
            let _ = std::process::Command::new("sleep")
                .arg("30")
                .current_dir(dir)
                .process_group(0)
                .spawn();
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            Ok(())
        }
        "sleep" => {
            let ms = rest.parse().unwrap_or(100);
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            Ok(())
        }
        "ask" | "ask-reject-only" => {
            // Worded as Claude Code words it; the kind says what it does.
            let mut options = vec![PermissionOption::new(
                "reject",
                "No",
                PermissionOptionKind::RejectOnce,
            )];
            if verb == "ask" {
                options.push(PermissionOption::new(
                    "allow",
                    "Yes",
                    PermissionOptionKind::AllowOnce,
                ));
            }
            let response = cx
                .send_request(RequestPermissionRequest::new(
                    session.clone(),
                    ToolCallUpdate::new(
                        format!("call-{n}"),
                        ToolCallUpdateFields::new().title("delete everything".to_owned()),
                    ),
                    options,
                ))
                .block_task()
                .await?;
            let chosen = match response.outcome {
                RequestPermissionOutcome::Selected(selected) => selected.option_id.to_string(),
                _ => "cancelled".to_owned(),
            };
            say(format!("permission: {chosen}"))
        }
        _ => say(format!("unknown command: {command}")),
    }
}
