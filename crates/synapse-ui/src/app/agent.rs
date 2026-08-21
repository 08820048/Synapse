use std::{
    fmt, fs,
    io::{self, Read as _, Write as _},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self as std_mpsc, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, ConnectionTo,
    schema::{
        ProtocolVersion,
        v1::{
            CancelNotification, ContentBlock, DeleteSessionRequest, EmbeddedResource,
            EmbeddedResourceResource, Implementation, InitializeRequest, LoadSessionRequest,
            NewSessionRequest, PermissionOptionKind, PromptRequest, RequestPermissionOutcome,
            RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
            SessionNotification, SessionUpdate, StopReason, TextContent, TextResourceContents,
            ToolCall, ToolCallContent, ToolCallUpdate,
        },
    },
};
use futures::{
    FutureExt as _,
    channel::{
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded},
        oneshot,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const PI_ACP_PACKAGE: &str = "pi-acp@0.0.33";
const MINIMUM_NODE_MAJOR: u64 = 22;
const MINIMUM_PI_VERSION: [u64; 3] = [0, 80, 4];
static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const BRIDGE_MAX_HEADER_BYTES: usize = 16 * 1024;
const BRIDGE_MAX_BODY_BYTES: usize = 64 * 1024;
const BRIDGE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const PI_EXTENSION_SOURCE: &str = include_str!("synapse_agent_tools.ts");

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(in crate::app) struct AgentSessionMetadata {
    pub id: String,
    pub acp_session_id: Option<String>,
    pub vault_path: PathBuf,
    pub title: String,
    pub created_at_ms: u128,
    pub updated_at_ms: u128,
}

impl AgentSessionMetadata {
    pub fn new(vault_path: PathBuf, title: String) -> Self {
        let now = unix_time_ms();
        Self {
            id: format!(
                "{now}-{}-{}",
                std::process::id(),
                SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            acp_session_id: None,
            vault_path,
            title,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    pub fn set_acp_session_id(&mut self, session_id: String) {
        self.acp_session_id = Some(session_id);
        self.updated_at_ms = unix_time_ms();
    }

    pub fn rename(&mut self, title: String) {
        self.title = title;
        self.updated_at_ms = unix_time_ms();
    }
}

pub(in crate::app) fn load_agent_sessions(path: &Path) -> io::Result<Vec<AgentSessionMetadata>> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    serde_json::from_str(&source).map_err(io::Error::other)
}

pub(in crate::app) fn save_agent_sessions(
    path: &Path,
    sessions: &[AgentSessionMetadata],
) -> io::Result<()> {
    let source = serde_json::to_vec_pretty(sessions).map_err(io::Error::other)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Agent session path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary_path = parent.join(format!(
        ".agent-sessions-{}-{}.tmp",
        std::process::id(),
        SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)?;
        file.write_all(&source)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct PiAcpPrerequisites {
    pub node_version: String,
    pub npx_version: String,
    pub pi_version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct PromptContext {
    pub uri: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct PiAcpPrompt {
    pub text: String,
    pub contexts: Vec<PromptContext>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct PiAcpBridgeEnvironment {
    pub url: String,
    pub token: String,
    pub pi_command: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "operation")]
#[serde(deny_unknown_fields)]
pub(in crate::app) enum AgentWorkspaceRequest {
    #[serde(rename = "todo.list")]
    TodoList,
    #[serde(rename = "todo.create")]
    TodoCreate { text: String },
    #[serde(rename = "todo.update")]
    TodoUpdate {
        id: u64,
        text: Option<String>,
        done: Option<bool>,
    },
    #[serde(rename = "todo.delete")]
    TodoDelete { id: u64 },
    #[serde(rename = "bookmark.list")]
    BookmarkList,
    #[serde(rename = "bookmark.create")]
    BookmarkCreate { url: String, title: Option<String> },
    #[serde(rename = "bookmark.update")]
    BookmarkUpdate { id: u64, title: String },
    #[serde(rename = "bookmark.delete")]
    BookmarkDelete { id: u64 },
}

pub(in crate::app) struct AgentWorkspaceBridgeRequest {
    pub request: AgentWorkspaceRequest,
    response: SyncSender<AgentWorkspaceResponse>,
}

impl AgentWorkspaceBridgeRequest {
    pub fn respond_with(
        self,
        handler: impl FnOnce(AgentWorkspaceRequest) -> AgentWorkspaceResponse,
    ) {
        let response = handler(self.request);
        self.response.send(response).ok();
    }
}

#[derive(Debug)]
pub(in crate::app) struct AgentWorkspaceResponse {
    status: u16,
    body: Value,
}

impl AgentWorkspaceResponse {
    pub fn success(data: Value) -> Self {
        Self {
            status: 200,
            body: json!({ "ok": true, "data": data }),
        }
    }

    pub fn error(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({
                "ok": false,
                "error": { "code": code, "message": message.into() }
            }),
        }
    }
}

pub(in crate::app) struct AgentWorkspaceBridge {
    environment: PiAcpBridgeEnvironment,
    stopped: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
}

impl AgentWorkspaceBridge {
    pub fn start(
        config_directory: &Path,
    ) -> io::Result<(Self, UnboundedReceiver<AgentWorkspaceBridgeRequest>)> {
        let pi_command = prepare_pi_wrapper(config_directory)?
            .into_os_string()
            .into_string()
            .map_err(|_| io::Error::other("Pi wrapper path must be valid Unicode"))?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let token = random_bridge_token()?;
        let (requests, receiver) = unbounded();
        let stopped = Arc::new(AtomicBool::new(false));
        let listener_stopped = stopped.clone();
        let listener_token = token.clone();
        let listener_thread = thread::Builder::new()
            .name("synapse-agent-bridge".to_owned())
            .spawn(move || {
                while !listener_stopped.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let response =
                                handle_bridge_connection(&mut stream, &listener_token, &requests);
                            let _ = write_http_response(&mut stream, response);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(25));
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok((
            Self {
                environment: PiAcpBridgeEnvironment {
                    url: format!("http://{address}"),
                    token,
                    pi_command,
                },
                stopped,
                listener_thread: Some(listener_thread),
            },
            receiver,
        ))
    }

    pub fn environment(&self) -> PiAcpBridgeEnvironment {
        self.environment.clone()
    }
}

impl Drop for AgentWorkspaceBridge {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(listener_thread) = self.listener_thread.take() {
            listener_thread.join().ok();
        }
    }
}

fn prepare_pi_wrapper(config_directory: &Path) -> io::Result<PathBuf> {
    let runtime_directory = config_directory.join("agent-runtime");
    fs::create_dir_all(&runtime_directory)?;
    let extension_path = runtime_directory.join("synapse-tools.ts");
    write_if_changed(&extension_path, PI_EXTENSION_SOURCE.as_bytes())?;

    #[cfg(target_os = "windows")]
    let (wrapper_path, wrapper_source) = (
        runtime_directory.join("synapse-pi.cmd"),
        b"@echo off\r\npi.cmd -e \"%~dp0synapse-tools.ts\" %*\r\n".as_slice(),
    );
    #[cfg(not(target_os = "windows"))]
    let (wrapper_path, wrapper_source) = (
        runtime_directory.join("synapse-pi"),
        b"#!/bin/sh\nexec pi -e \"${0%/*}/synapse-tools.ts\" \"$@\"\n".as_slice(),
    );
    write_if_changed(&wrapper_path, wrapper_source)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&wrapper_path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(wrapper_path)
}

fn write_if_changed(path: &Path, source: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == source) {
        return Ok(());
    }
    fs::write(path, source)
}

fn random_bridge_token() -> io::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| io::Error::other(format!("unable to create bridge token: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn handle_bridge_connection(
    stream: &mut TcpStream,
    token: &str,
    requests: &UnboundedSender<AgentWorkspaceBridgeRequest>,
) -> AgentWorkspaceResponse {
    if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
        return AgentWorkspaceResponse::error(500, "bridge_io", error.to_string());
    }
    if let Err(error) = stream.set_write_timeout(Some(Duration::from_secs(5))) {
        return AgentWorkspaceResponse::error(500, "bridge_io", error.to_string());
    }
    let request = match read_http_request(stream, token) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let (response, response_receiver) = std_mpsc::sync_channel(1);
    if requests
        .unbounded_send(AgentWorkspaceBridgeRequest { request, response })
        .is_err()
    {
        return AgentWorkspaceResponse::error(503, "bridge_unavailable", "Synapse is closing");
    }
    response_receiver
        .recv_timeout(BRIDGE_RESPONSE_TIMEOUT)
        .unwrap_or_else(|_| {
            AgentWorkspaceResponse::error(
                504,
                "bridge_timeout",
                "Synapse did not answer the workspace request",
            )
        })
}

fn read_http_request(
    stream: &mut TcpStream,
    token: &str,
) -> Result<AgentWorkspaceRequest, AgentWorkspaceResponse> {
    let mut bytes = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    let headers_end = loop {
        let read = stream.read(&mut buffer).map_err(|error| {
            AgentWorkspaceResponse::error(400, "invalid_request", error.to_string())
        })?;
        if read == 0 {
            return Err(AgentWorkspaceResponse::error(
                400,
                "invalid_request",
                "Unexpected end of HTTP request",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() > BRIDGE_MAX_HEADER_BYTES {
            return Err(AgentWorkspaceResponse::error(
                413,
                "request_too_large",
                "HTTP headers are too large",
            ));
        }
    };
    if headers_end > BRIDGE_MAX_HEADER_BYTES {
        return Err(AgentWorkspaceResponse::error(
            413,
            "request_too_large",
            "HTTP headers are too large",
        ));
    }
    let headers = std::str::from_utf8(&bytes[..headers_end]).map_err(|_| {
        AgentWorkspaceResponse::error(400, "invalid_request", "HTTP headers must be UTF-8")
    })?;
    let mut lines = headers.split("\r\n");
    if lines.next() != Some("POST /v1/workspace HTTP/1.1") {
        return Err(AgentWorkspaceResponse::error(
            404,
            "not_found",
            "Unknown bridge endpoint",
        ));
    }
    let mut authorization = None;
    let mut content_length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(AgentWorkspaceResponse::error(
                400,
                "invalid_request",
                "Malformed HTTP header",
            ));
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "authorization" if authorization.is_none() => authorization = Some(value.trim()),
            "content-length" if content_length.is_none() => {
                content_length = value.trim().parse::<usize>().ok()
            }
            _ => {}
        }
    }
    if authorization != Some(format!("Bearer {token}").as_str()) {
        return Err(AgentWorkspaceResponse::error(
            401,
            "unauthorized",
            "Invalid Synapse bridge token",
        ));
    }
    let content_length = content_length.ok_or_else(|| {
        AgentWorkspaceResponse::error(411, "length_required", "Content-Length is required")
    })?;
    if content_length > BRIDGE_MAX_BODY_BYTES {
        return Err(AgentWorkspaceResponse::error(
            413,
            "request_too_large",
            "JSON body is too large",
        ));
    }
    let body_end = headers_end + content_length;
    while bytes.len() < body_end {
        let read = stream.read(&mut buffer).map_err(|error| {
            AgentWorkspaceResponse::error(400, "invalid_request", error.to_string())
        })?;
        if read == 0 {
            return Err(AgentWorkspaceResponse::error(
                400,
                "invalid_request",
                "Unexpected end of JSON body",
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    serde_json::from_slice(&bytes[headers_end..body_end])
        .map_err(|error| AgentWorkspaceResponse::error(400, "invalid_input", error.to_string()))
}

fn write_http_response(stream: &mut TcpStream, response: AgentWorkspaceResponse) -> io::Result<()> {
    let body = serde_json::to_vec(&response.body).map_err(io::Error::other)?;
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        411 => "Length Required",
        413 => "Payload Too Large",
        422 => "Unprocessable Content",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason,
        body.len()
    )?;
    stream.write_all(&body)
}

#[derive(Debug)]
pub(in crate::app) enum PiAcpEvent {
    Starting,
    HistoryReplayStarted,
    HistoryReplayFinished(Option<String>),
    SessionStarted(String),
    SessionUpdate(Box<SessionUpdate>),
    PermissionRequested(PiAcpPermissionRequest),
    Finished(StopReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct PermissionOptionView {
    pub id: String,
    pub name: String,
    pub kind: PermissionOptionKind,
}

impl PermissionOptionView {
    pub fn allows(&self) -> bool {
        matches!(
            self.kind,
            PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
        )
    }
}

#[derive(Debug)]
pub(in crate::app) struct PiAcpPermissionRequest {
    pub tool_title: String,
    pub options: Vec<PermissionOptionView>,
    pub response: oneshot::Sender<Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) enum AgentTranscriptItem {
    User(String),
    Assistant(String),
    Tool(AgentToolView),
    System(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct AgentToolDiff {
    pub path: PathBuf,
    pub old_text: Option<String>,
    pub new_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct AgentToolView {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub locations: Vec<String>,
    pub details: Vec<String>,
    pub diffs: Vec<AgentToolDiff>,
}

impl AgentToolView {
    pub fn display_text(&self) -> String {
        let mut output = format!("{} · {} · {}", self.title, self.kind, self.status);
        for location in &self.locations {
            output.push_str("\n↳ ");
            output.push_str(location);
        }
        for detail in &self.details {
            output.push('\n');
            output.push_str(detail);
        }
        for diff in &self.diffs {
            output.push_str(&format!("\n--- {}\n", diff.path.display()));
            output.push_str(diff.old_text.as_deref().unwrap_or("<new file>"));
            output.push_str(&format!("\n+++ {}\n", diff.path.display()));
            output.push_str(&diff.new_text);
        }
        output
    }

    fn from_call(call: ToolCall) -> Self {
        let (details, diffs) = tool_content(&call.content);
        Self {
            id: call.tool_call_id.to_string(),
            title: call.title,
            kind: format!("{:?}", call.kind),
            status: format!("{:?}", call.status),
            locations: tool_locations(&call.locations),
            details,
            diffs,
        }
    }

    fn apply_update(&mut self, update: ToolCallUpdate) {
        let fields = update.fields;
        if let Some(title) = fields.title {
            self.title = title;
        }
        if let Some(kind) = fields.kind {
            self.kind = format!("{kind:?}");
        }
        if let Some(status) = fields.status {
            self.status = format!("{status:?}");
        }
        if let Some(locations) = fields.locations {
            self.locations = tool_locations(&locations);
        }
        if let Some(content) = fields.content {
            (self.details, self.diffs) = tool_content(&content);
        }
    }

    fn from_update(update: ToolCallUpdate) -> Self {
        let id = update.tool_call_id.to_string();
        let mut view = Self {
            id,
            title: "Pi tool".to_owned(),
            kind: "Other".to_owned(),
            status: "Pending".to_owned(),
            locations: Vec::new(),
            details: Vec::new(),
            diffs: Vec::new(),
        };
        view.apply_update(update);
        view
    }
}

fn tool_locations(
    locations: &[agent_client_protocol::schema::v1::ToolCallLocation],
) -> Vec<String> {
    locations
        .iter()
        .map(|location| match location.line {
            Some(line) => format!("{}:{line}", location.path.display()),
            None => location.path.display().to_string(),
        })
        .collect()
}

fn tool_content(content: &[ToolCallContent]) -> (Vec<String>, Vec<AgentToolDiff>) {
    let mut details = Vec::new();
    let mut diffs = Vec::new();
    for content in content {
        match content {
            ToolCallContent::Content(content) => {
                if let ContentBlock::Text(text) = &content.content {
                    details.push(text.text.clone());
                }
            }
            ToolCallContent::Diff(diff) => diffs.push(AgentToolDiff {
                path: diff.path.clone(),
                old_text: diff.old_text.clone(),
                new_text: diff.new_text.clone(),
            }),
            ToolCallContent::Terminal(terminal) => {
                details.push(format!("Terminal {}", terminal.terminal_id));
            }
            _ => {}
        }
    }
    (details, diffs)
}

pub(in crate::app) fn apply_event(
    transcript: &mut Vec<AgentTranscriptItem>,
    event: PiAcpEvent,
) -> Option<String> {
    match event {
        PiAcpEvent::Starting => Some("Starting pi-acp…".to_owned()),
        PiAcpEvent::HistoryReplayStarted | PiAcpEvent::HistoryReplayFinished(_) => None,
        PiAcpEvent::SessionStarted(session_id) => Some(format!("Session {session_id}")),
        PiAcpEvent::SessionUpdate(update) => match *update {
            SessionUpdate::UserMessageChunk(chunk) => {
                let ContentBlock::Text(text) = chunk.content else {
                    return None;
                };
                if let Some(AgentTranscriptItem::User(message)) = transcript.last_mut() {
                    message.push_str(&text.text);
                } else {
                    transcript.push(AgentTranscriptItem::User(text.text));
                }
                None
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                let ContentBlock::Text(text) = chunk.content else {
                    return None;
                };
                if let Some(AgentTranscriptItem::Assistant(message)) = transcript.last_mut() {
                    message.push_str(&text.text);
                } else {
                    transcript.push(AgentTranscriptItem::Assistant(text.text));
                }
                None
            }
            SessionUpdate::ToolCall(tool) => {
                let view = AgentToolView::from_call(tool);
                if let Some(AgentTranscriptItem::Tool(existing)) = transcript.iter_mut().find(
                    |item| matches!(item, AgentTranscriptItem::Tool(tool) if tool.id == view.id),
                ) {
                    *existing = view;
                } else {
                    transcript.push(AgentTranscriptItem::Tool(view));
                }
                None
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let id = update.tool_call_id.to_string();
                if let Some(AgentTranscriptItem::Tool(existing)) = transcript
                    .iter_mut()
                    .find(|item| matches!(item, AgentTranscriptItem::Tool(tool) if tool.id == id))
                {
                    existing.apply_update(update);
                } else {
                    transcript.push(AgentTranscriptItem::Tool(AgentToolView::from_update(
                        update,
                    )));
                }
                None
            }
            _ => None,
        },
        PiAcpEvent::PermissionRequested(_) => None,
        PiAcpEvent::Finished(reason) => Some(format!("Finished: {reason:?}")),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct PiAcpTurnResult {
    pub session_id: String,
    pub stop_reason: StopReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) enum PiAcpError {
    MissingExecutable(&'static str),
    UnsupportedNode(String),
    UnsupportedPi(String),
    Protocol(String),
}

impl fmt::Display for PiAcpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingExecutable("node") => formatter
                .write_str("Node.js 22+ is required. Install Node.js, then restart Synapse."),
            Self::MissingExecutable("npx") => {
                formatter.write_str("npx is required and is normally installed with Node.js.")
            }
            Self::MissingExecutable("pi") => formatter
                .write_str("Pi is required. Run: npm install -g @earendil-works/pi-coding-agent"),
            Self::MissingExecutable(name) => {
                write!(formatter, "Required executable not found: {name}")
            }
            Self::UnsupportedNode(version) => {
                write!(formatter, "Node.js 22+ is required; found {version}.")
            }
            Self::UnsupportedPi(version) => {
                write!(formatter, "Pi 0.80.4+ is required; found {version}.")
            }
            Self::Protocol(message) => {
                write!(formatter, "pi-acp error: {message}")?;
                if [
                    "No model selected",
                    "No models available",
                    "No API key found",
                ]
                .iter()
                .any(|needle| message.contains(needle))
                {
                    formatter.write_str(
                        " Open Pi in a terminal, run /login, then select a model with /model.",
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for PiAcpError {}

pub(in crate::app) struct PiAcpRuntime;

impl PiAcpRuntime {
    pub fn check_prerequisites() -> Result<PiAcpPrerequisites, PiAcpError> {
        let node_version = command_version("node")?;
        let npx_version = command_version("npx")?;
        let pi_version = command_version("pi")?;

        if parse_version(&node_version).is_none_or(|version| version[0] < MINIMUM_NODE_MAJOR) {
            return Err(PiAcpError::UnsupportedNode(node_version));
        }
        if parse_version(&pi_version).is_none_or(|version| version < MINIMUM_PI_VERSION) {
            return Err(PiAcpError::UnsupportedPi(pi_version));
        }

        Ok(PiAcpPrerequisites {
            node_version,
            npx_version,
            pi_version,
        })
    }

    pub async fn run_turn(
        vault_root: PathBuf,
        session_id: Option<String>,
        prompt: PiAcpPrompt,
        bridge: Option<PiAcpBridgeEnvironment>,
        events: UnboundedSender<PiAcpEvent>,
        cancel: oneshot::Receiver<()>,
    ) -> Result<PiAcpTurnResult, PiAcpError> {
        ensure_absolute_vault(&vault_root)?;
        events.unbounded_send(PiAcpEvent::Starting).ok();

        let notification_events = events.clone();
        let permission_events = events.clone();
        let agent = pi_acp_agent(bridge);

        agent_client_protocol::Client
            .builder()
            .name("synapse")
            .on_receive_notification(
                async move |notification: SessionNotification, _cx| {
                    notification_events
                        .unbounded_send(PiAcpEvent::SessionUpdate(Box::new(notification.update)))
                        .ok();
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |request: RequestPermissionRequest, responder, _connection| {
                    let (decision_sender, decision_receiver) = oneshot::channel();
                    let tool_title = request
                        .tool_call
                        .fields
                        .title
                        .clone()
                        .unwrap_or_else(|| "Pi tool".to_owned());
                    permission_events
                        .unbounded_send(PiAcpEvent::PermissionRequested(PiAcpPermissionRequest {
                            tool_title,
                            options: request
                                .options
                                .iter()
                                .map(|option| PermissionOptionView {
                                    id: option.option_id.to_string(),
                                    name: option.name.clone(),
                                    kind: option.kind,
                                })
                                .collect(),
                            response: decision_sender,
                        }))
                        .ok();
                    let outcome = match decision_receiver.await.ok().flatten() {
                        Some(option_id) => RequestPermissionOutcome::Selected(
                            SelectedPermissionOutcome::new(option_id),
                        ),
                        None => RequestPermissionOutcome::Cancelled,
                    };
                    responder.respond(RequestPermissionResponse::new(outcome))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1).client_info(
                        Implementation::new("synapse", env!("CARGO_PKG_VERSION")).title("Synapse"),
                    ))
                    .block_task()
                    .await?;

                let session_id = match session_id {
                    Some(session_id) => {
                        events
                            .unbounded_send(PiAcpEvent::HistoryReplayStarted)
                            .ok();
                        connection
                            .send_request(LoadSessionRequest::new(
                                session_id.clone(),
                                vault_root.clone(),
                            ))
                            .block_task()
                            .await?;
                        events
                            .unbounded_send(PiAcpEvent::HistoryReplayFinished(Some(
                                prompt.text.clone(),
                            )))
                            .ok();
                        session_id
                    }
                    None => connection
                        .send_request(NewSessionRequest::new(vault_root))
                        .block_task()
                        .await?
                        .session_id
                        .to_string(),
                };
                events
                    .unbounded_send(PiAcpEvent::SessionStarted(session_id.clone()))
                    .ok();

                let prompt_request = connection
                    .send_request(PromptRequest::new(
                        session_id.clone(),
                        prompt_content(prompt),
                    ))
                    .block_task()
                    .fuse();
                let cancel = cancel.fuse();
                futures::pin_mut!(prompt_request, cancel);
                let response = futures::select! {
                    response = prompt_request => response?,
                    _ = cancel => {
                        connection.send_notification(CancelNotification::new(session_id.clone()))?;
                        let (timeout_sender, timeout) = oneshot::channel();
                        thread::spawn(move || {
                            thread::sleep(Duration::from_secs(5));
                            timeout_sender.send(()).ok();
                        });
                        let timeout = timeout.fuse();
                        futures::pin_mut!(timeout);
                        futures::select! {
                            response = prompt_request => response?,
                            _ = timeout => agent_client_protocol::schema::v1::PromptResponse::new(StopReason::Cancelled),
                        }
                    }
                };
                events
                    .unbounded_send(PiAcpEvent::Finished(response.stop_reason))
                    .ok();

                Ok(PiAcpTurnResult {
                    session_id,
                    stop_reason: response.stop_reason,
                })
            })
            .await
            .map_err(|error| PiAcpError::Protocol(error.to_string()))
    }

    pub async fn load_session(
        vault_root: PathBuf,
        session_id: String,
        bridge: Option<PiAcpBridgeEnvironment>,
        events: UnboundedSender<PiAcpEvent>,
        cancel: oneshot::Receiver<()>,
    ) -> Result<(), PiAcpError> {
        ensure_absolute_vault(&vault_root)?;
        events.unbounded_send(PiAcpEvent::Starting).ok();
        let notification_events = events.clone();
        let load = agent_client_protocol::Client
            .builder()
            .name("synapse")
            .on_receive_notification(
                async move |notification: SessionNotification, _cx| {
                    notification_events
                        .unbounded_send(PiAcpEvent::SessionUpdate(Box::new(notification.update)))
                        .ok();
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_with(
                pi_acp_agent(bridge),
                |connection: ConnectionTo<Agent>| async move {
                    connection
                        .send_request(
                            InitializeRequest::new(ProtocolVersion::V1).client_info(
                                Implementation::new("synapse", env!("CARGO_PKG_VERSION"))
                                    .title("Synapse"),
                            ),
                        )
                        .block_task()
                        .await?;
                    events.unbounded_send(PiAcpEvent::HistoryReplayStarted).ok();
                    connection
                        .send_request(LoadSessionRequest::new(session_id.clone(), vault_root))
                        .block_task()
                        .await?;
                    events
                        .unbounded_send(PiAcpEvent::HistoryReplayFinished(None))
                        .ok();
                    events
                        .unbounded_send(PiAcpEvent::SessionStarted(session_id))
                        .ok();
                    Ok(())
                },
            )
            .fuse();
        let cancel = cancel.fuse();
        futures::pin_mut!(load, cancel);
        futures::select! {
            result = load => result.map_err(|error| PiAcpError::Protocol(error.to_string())),
            _ = cancel => Err(PiAcpError::Protocol("session loading cancelled".to_owned())),
        }
    }

    pub async fn delete_session(session_id: String) -> Result<(), PiAcpError> {
        agent_client_protocol::Client
            .builder()
            .name("synapse")
            .connect_with(
                pi_acp_agent(None),
                |connection: ConnectionTo<Agent>| async move {
                    connection
                        .send_request(
                            InitializeRequest::new(ProtocolVersion::V1).client_info(
                                Implementation::new("synapse", env!("CARGO_PKG_VERSION"))
                                    .title("Synapse"),
                            ),
                        )
                        .block_task()
                        .await?;
                    connection
                        .send_request(DeleteSessionRequest::new(session_id))
                        .block_task()
                        .await?;
                    Ok(())
                },
            )
            .await
            .map_err(|error| PiAcpError::Protocol(error.to_string()))
    }
}

fn pi_acp_agent(bridge: Option<PiAcpBridgeEnvironment>) -> AcpAgent {
    let mut config = AcpAgentConfig::new("npx")
        .args(["-y", PI_ACP_PACKAGE])
        .env("PI_ACP_ENABLE_EMBEDDED_CONTEXT", "true");
    if let Some(bridge) = bridge {
        config = config
            .env("PI_ACP_PI_COMMAND", bridge.pi_command)
            .env("SYNAPSE_AGENT_BRIDGE_URL", bridge.url)
            .env("SYNAPSE_AGENT_BRIDGE_TOKEN", bridge.token);
    }
    AcpAgent::new(config)
}

fn ensure_absolute_vault(vault_root: &Path) -> Result<(), PiAcpError> {
    if vault_root.is_absolute() {
        Ok(())
    } else {
        Err(PiAcpError::Protocol(
            "the Vault root must be an absolute path".to_owned(),
        ))
    }
}

fn command_version(executable: &'static str) -> Result<String, PiAcpError> {
    let output = Command::new(executable)
        .arg("--version")
        .output()
        .map_err(|_| PiAcpError::MissingExecutable(executable))?;
    if !output.status.success() {
        return Err(PiAcpError::MissingExecutable(executable));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn parse_version(value: &str) -> Option<[u64; 3]> {
    let mut numbers = value
        .trim()
        .trim_start_matches(|character: char| !character.is_ascii_digit())
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(str::parse::<u64>);
    Some([
        numbers.next()?.ok()?,
        numbers.next().transpose().ok()?.unwrap_or(0),
        numbers.next().transpose().ok()?.unwrap_or(0),
    ])
}

fn prompt_content(prompt: PiAcpPrompt) -> Vec<ContentBlock> {
    let mut content = vec![ContentBlock::Text(TextContent::new(prompt.text))];
    content.extend(prompt.contexts.into_iter().map(|context| {
        ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::TextResourceContents(
                TextResourceContents::new(context.text, context.uri).mime_type("text/markdown"),
            ),
        ))
    }));
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as _;

    fn bridge_stream(request: String) -> TcpStream {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let mut client = TcpStream::connect(address).unwrap();
            client.write_all(request.as_bytes()).unwrap();
        });
        listener.accept().unwrap().0
    }

    #[test]
    fn parses_node_and_pi_versions() {
        assert_eq!(parse_version("v26.0.0"), Some([26, 0, 0]));
        assert_eq!(parse_version("0.84.1"), Some([0, 84, 1]));
        assert_eq!(parse_version("22"), Some([22, 0, 0]));
        assert_eq!(parse_version("unknown"), None);
    }

    #[test]
    fn prompt_keeps_text_and_embedded_markdown_separate() {
        let content = prompt_content(PiAcpPrompt {
            text: "Summarize this".to_owned(),
            contexts: vec![PromptContext {
                uri: "file:///vault/note.md".to_owned(),
                text: "# Note".to_owned(),
            }],
        });
        assert_eq!(content.len(), 2);
        assert!(matches!(&content[0], ContentBlock::Text(text) if text.text == "Summarize this"));
        assert!(matches!(
            &content[1],
            ContentBlock::Resource(EmbeddedResource {
                resource: EmbeddedResourceResource::TextResourceContents(resource),
                ..
            }) if resource.text == "# Note" && resource.uri == "file:///vault/note.md"
        ));
    }

    #[test]
    fn session_metadata_round_trips_and_stays_scoped_to_its_vault() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("agent-sessions.json");
        let session =
            AgentSessionMetadata::new(directory.path().join("vault"), "Planning".to_owned());

        save_agent_sessions(&path, std::slice::from_ref(&session)).unwrap();
        assert_eq!(load_agent_sessions(&path).unwrap(), vec![session]);
    }

    #[test]
    fn tool_updates_merge_and_keep_the_structured_diff() {
        use agent_client_protocol::schema::v1::{
            Diff, ToolCallStatus, ToolCallUpdateFields, ToolKind,
        };

        let mut transcript = Vec::new();
        apply_event(
            &mut transcript,
            PiAcpEvent::SessionUpdate(Box::new(SessionUpdate::ToolCall(
                ToolCall::new("edit-1", "Edit note")
                    .kind(ToolKind::Edit)
                    .content(vec![
                        Diff::new("/vault/note.md", "after")
                            .old_text("before")
                            .into(),
                    ]),
            ))),
        );
        apply_event(
            &mut transcript,
            PiAcpEvent::SessionUpdate(Box::new(SessionUpdate::ToolCallUpdate(
                ToolCallUpdate::new(
                    "edit-1",
                    ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
                ),
            ))),
        );

        let [AgentTranscriptItem::Tool(tool)] = transcript.as_slice() else {
            panic!("expected one merged tool row");
        };
        assert_eq!(tool.status, "Completed");
        assert!(tool.display_text().contains("before"));
        assert!(tool.display_text().contains("after"));
    }

    #[test]
    fn history_chunks_rebuild_user_and_assistant_messages_in_order() {
        use agent_client_protocol::schema::v1::ContentChunk;

        let mut transcript = Vec::new();
        for update in [
            SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("hello"),
            ))),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("hi"),
            ))),
        ] {
            apply_event(&mut transcript, PiAcpEvent::SessionUpdate(Box::new(update)));
        }

        assert_eq!(
            transcript,
            vec![
                AgentTranscriptItem::User("hello".to_owned()),
                AgentTranscriptItem::Assistant("hi".to_owned()),
            ]
        );
    }

    #[test]
    fn provider_errors_explain_how_to_configure_pi() {
        let error = PiAcpError::Protocol("No model selected".to_owned()).to_string();
        assert!(error.contains("/login"));
        assert!(error.contains("/model"));
    }

    #[test]
    fn workspace_bridge_requires_its_token_and_parses_typed_input() {
        let body = r#"{"operation":"todo.create","text":"Ship Agent V1"}"#;
        let request = format!(
            "POST /v1/workspace HTTP/1.1\r\nAuthorization: Bearer secret\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(
            read_http_request(&mut bridge_stream(request), "secret").unwrap(),
            AgentWorkspaceRequest::TodoCreate {
                text: "Ship Agent V1".to_owned()
            }
        );

        let body = r#"{"operation":"todo.list"}"#;
        let request = format!(
            "POST /v1/workspace HTTP/1.1\r\nAuthorization: Bearer wrong\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let error = read_http_request(&mut bridge_stream(request), "secret").unwrap_err();
        assert_eq!(error.status, 401);
    }

    #[test]
    fn workspace_bridge_serves_the_bundled_pi_extension() {
        let directory = tempfile::tempdir().unwrap();
        let (bridge, mut requests) = AgentWorkspaceBridge::start(directory.path()).unwrap();
        let environment = bridge.environment();
        let address = environment.url.strip_prefix("http://").unwrap().to_owned();
        let token = environment.token.clone();
        let client = thread::spawn(move || {
            let body = r#"{"operation":"bookmark.list"}"#;
            let mut stream = TcpStream::connect(address).unwrap();
            write!(
                stream,
                "POST /v1/workspace HTTP/1.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        });
        let request = futures::executor::block_on(requests.next()).unwrap();
        assert_eq!(request.request, AgentWorkspaceRequest::BookmarkList);
        request.respond_with(|_| AgentWorkspaceResponse::success(json!([])));
        let response = client.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        let body: Value = serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body, json!({ "ok": true, "data": [] }));
        assert!(
            directory
                .path()
                .join("agent-runtime/synapse-tools.ts")
                .is_file()
        );
        assert!(Path::new(&environment.pi_command).is_file());
    }
}
