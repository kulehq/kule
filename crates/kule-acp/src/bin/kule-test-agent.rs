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
//! - `background <file>`: leaves a process running, its pid in the file;
//! - `reviewed-write <path> <text>`: an edit that writes the file while a
//!   review (a `think` call) runs inside it, the review ending first, as
//!   Codex does;
//! - `cost <amount>`: reports what the session cost so far, in USD;
//! - `tokens <input> <output>`: the token counts the turn's end reports;
//! - `mcp`: a message naming the MCP servers the session was given, as
//!   `name: command args`, separated by ` | `;
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
    AgentCapabilities, CancelNotification, ContentBlock, ContentChunk, Cost, InitializeRequest,
    InitializeResponse, McpServer, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, PromptCapabilities, PromptRequest, PromptResponse,
    RequestPermissionOutcome, RequestPermissionRequest, SessionId, SessionNotification,
    SessionUpdate, StopReason, TextContent, ToolCall, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind, Usage, UsageUpdate,
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
        "cost" => cx.send_notification(SessionNotification::new(
            session.clone(),
            SessionUpdate::UsageUpdate(
                UsageUpdate::new(1000, 200_000).cost(Cost::new(rest.parse().unwrap_or(0.0), "USD")),
            ),
        )),
        // A real agent waits for its model between tool calls.
        // Leaves a process running after the turn, writing its pid to the
        // file, the way an agent's `npm run dev &` would.
        "background" => {
            let _ = std::process::Command::new("sh")
                .args(["-c", &format!("echo $$ > {rest}; exec sleep 30")])
                .current_dir(dir)
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
