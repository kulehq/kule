//! A scripted ACP agent for Kule's tests: no model, no network, the same
//! protocol a real agent speaks. The prompt is a list of commands separated
//! by `;`:
//!
//! - `write <path> <text>`: a tool call that writes the file, relative to the
//!   session's working directory unless absolute;
//! - `say <text>`: a message;
//! - `sleep <ms>`: waits, the way a real agent waits for its model;
//! - `ask`: asks the client for permission and says what it chose.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest,
    PromptResponse, RequestPermissionOutcome, RequestPermissionRequest, SessionId,
    SessionNotification, SessionUpdate, StopReason, TextContent, ToolCall, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Responder, Stdio};

#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    let cwd: Arc<Mutex<PathBuf>> = Arc::new(Mutex::new(PathBuf::from(".")));
    let session_cwd = cwd.clone();
    Agent
        .builder()
        .name("kule-test-agent")
        .on_receive_request(
            async move |request: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(request.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: NewSessionRequest, responder, _cx| {
                *session_cwd.lock().unwrap() = request.cwd.clone();
                responder.respond(NewSessionResponse::new(SessionId::new("test-session")))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: PromptRequest,
                        responder: Responder<PromptResponse>,
                        cx: ConnectionTo<Client>| {
                let script = request
                    .prompt
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect::<String>();
                let dir = cwd.lock().unwrap().clone();
                let session = request.session_id.clone();
                let connection = cx.clone();
                cx.spawn(async move {
                    for (n, command) in script.split(';').map(str::trim).enumerate() {
                        perform(&connection, &session, &dir, n, command).await?;
                    }
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
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
        "say" => say(rest.to_owned()),
        // A real agent waits for its model between tool calls.
        "sleep" => {
            let ms = rest.parse().unwrap_or(100);
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            Ok(())
        }
        "ask" => {
            let response = cx
                .send_request(RequestPermissionRequest::new(
                    session.clone(),
                    ToolCallUpdate::new(
                        format!("call-{n}"),
                        ToolCallUpdateFields::new().title("delete everything".to_owned()),
                    ),
                    vec![
                        PermissionOption::new("reject", "Reject", PermissionOptionKind::RejectOnce),
                        PermissionOption::new("allow", "Allow", PermissionOptionKind::AllowOnce),
                    ],
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
