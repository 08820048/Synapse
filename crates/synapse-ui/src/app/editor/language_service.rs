//! Background LSP transport for fenced code blocks.
//!
//! LSP I/O never runs on GPUI's executor. A server gets one private worker
//! thread, so typing always receives local candidates immediately.

use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
};

use futures::channel::oneshot;
use serde_json::{Value, json};

use super::{
    code_block::CodeLanguage,
    completion::{CodeCompletionItem, CompletionKind},
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum LanguageServer {
    RustAnalyzer,
    Clangd,
}

impl LanguageServer {
    fn for_language(language: CodeLanguage) -> Option<Self> {
        match language {
            CodeLanguage::Rust => Some(Self::RustAnalyzer),
            CodeLanguage::C | CodeLanguage::Cpp => Some(Self::Clangd),
            _ => None,
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust-analyzer",
            Self::Clangd => "clangd",
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::app) struct LspCompletionRequest {
    pub(in crate::app) language: CodeLanguage,
    pub(in crate::app) language_id: &'static str,
    pub(in crate::app) uri: String,
    pub(in crate::app) workspace_uri: String,
    pub(in crate::app) document_text: String,
    pub(in crate::app) line: usize,
    pub(in crate::app) utf16_column: usize,
}

#[derive(Clone, Default)]
pub(in crate::app) struct LanguageService {
    workers: Arc<Mutex<HashMap<LanguageServer, mpsc::Sender<WorkerRequest>>>>,
}

enum WorkerRequest {
    Completion {
        request: LspCompletionRequest,
        response: oneshot::Sender<Result<Vec<CodeCompletionItem>, String>>,
    },
}

struct LspConnection {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_request_id: u64,
    open_documents: HashMap<String, i64>,
}

impl LanguageService {
    /// `None` means this language has no server configured yet. Missing
    /// executables resolve as an asynchronous error and leave local candidates
    /// untouched.
    pub(in crate::app) fn request_completions(
        &self,
        request: LspCompletionRequest,
    ) -> Option<oneshot::Receiver<Result<Vec<CodeCompletionItem>, String>>> {
        let server = LanguageServer::for_language(request.language)?;
        let (response_tx, response_rx) = oneshot::channel();
        let sender = match self.workers.lock() {
            Ok(mut workers) => workers
                .entry(server)
                .or_insert_with(|| spawn_worker(server))
                .clone(),
            Err(_) => return Some(response_rx),
        };
        let _ = sender.send(WorkerRequest::Completion {
            request,
            response: response_tx,
        });
        Some(response_rx)
    }
}

fn spawn_worker(server: LanguageServer) -> mpsc::Sender<WorkerRequest> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name(format!("synapse-{}", server.command()))
        .spawn(move || run_worker(server, receiver))
        .expect("could not start language service worker");
    sender
}

fn run_worker(server: LanguageServer, receiver: mpsc::Receiver<WorkerRequest>) {
    let mut connection = None;
    while let Ok(WorkerRequest::Completion { request, response }) = receiver.recv() {
        let result = (|| {
            if connection.is_none() {
                connection = Some(start_connection(server, &request.workspace_uri)?);
            }
            connection
                .as_mut()
                .expect("connection initialized above")
                .completion(request)
        })();
        let failed = result.is_err();
        let _ = response.send(result);
        if failed {
            connection = None;
        }
    }
}

fn start_connection(server: LanguageServer, workspace_uri: &str) -> Result<LspConnection, String> {
    let mut child = Command::new(server.command())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", server.command()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("{} did not provide stdin", server.command()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{} did not provide stdout", server.command()))?;
    // Dropping the connection closes stdin as the app exits, so the server can
    // terminate without ever blocking rendering or input handling.
    let mut connection = LspConnection {
        stdin,
        stdout: BufReader::new(stdout),
        next_request_id: 1,
        open_documents: HashMap::new(),
    };
    let id = connection.next_id();
    connection.send(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": workspace_uri,
            "workspaceFolders": [{ "uri": workspace_uri, "name": "Synapse" }],
            "capabilities": {
                "textDocument": {
                    "completion": { "completionItem": { "snippetSupport": false } }
                }
            }
        }
    }))?;
    connection.wait_for_response(id)?;
    connection.send(json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }))?;
    Ok(connection)
}

impl LspConnection {
    fn next_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        id
    }

    fn completion(
        &mut self,
        request: LspCompletionRequest,
    ) -> Result<Vec<CodeCompletionItem>, String> {
        let version = {
            let version = self.open_documents.entry(request.uri.clone()).or_insert(0);
            *version += 1;
            *version
        };
        if version == 1 {
            self.send(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": request.uri,
                    "languageId": request.language_id,
                    "version": version,
                    "text": request.document_text,
                }}
            }))?;
        } else {
            self.send(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": request.uri, "version": version },
                    "contentChanges": [{ "text": request.document_text }]
                }
            }))?;
        }
        let id = self.next_id();
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": request.uri },
                "position": { "line": request.line, "character": request.utf16_column },
                "context": { "triggerKind": 1 }
            }
        }))?;
        completion_items_from_response(self.wait_for_response(id)?)
    }

    fn send(&mut self, value: Value) -> Result<(), String> {
        let body = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())
            .map_err(|error| error.to_string())?;
        self.stdin
            .write_all(&body)
            .map_err(|error| error.to_string())?;
        self.stdin.flush().map_err(|error| error.to_string())
    }

    fn wait_for_response(&mut self, id: u64) -> Result<Value, String> {
        loop {
            let message = read_message(&mut self.stdout)?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(format!("language server error: {error}"));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }
}

fn read_message(reader: &mut BufReader<ChildStdout>) -> Result<Value, String> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("language server closed its output stream".to_owned());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| format!("invalid LSP content length: {error}"))?,
            );
        }
    }
    let length = content_length.ok_or_else(|| "missing LSP content length".to_owned())?;
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&body).map_err(|error| format!("invalid LSP response: {error}"))
}

fn completion_items_from_response(result: Value) -> Result<Vec<CodeCompletionItem>, String> {
    let values = match result {
        Value::Null => return Ok(Vec::new()),
        Value::Array(items) => items,
        Value::Object(mut object) => object
            .remove("items")
            .and_then(|items| items.as_array().cloned())
            .unwrap_or_default(),
        _ => return Err("invalid completion response".to_owned()),
    };
    let mut seen = HashSet::new();
    Ok(values
        .into_iter()
        .filter_map(|value| completion_item_from_lsp(&value))
        .filter(|item| seen.insert((item.label.to_lowercase(), item.insert_text.clone())))
        .collect())
}

fn completion_item_from_lsp(value: &Value) -> Option<CodeCompletionItem> {
    let label = value.get("label")?.as_str()?.to_owned();
    let insert_text = value
        .get("textEdit")
        .and_then(|edit| edit.get("newText"))
        .and_then(Value::as_str)
        .or_else(|| value.get("insertText").and_then(Value::as_str))
        .unwrap_or(&label);
    let insert_text = if value
        .get("insertTextFormat")
        .and_then(Value::as_u64)
        .is_some_and(|format| format == 2)
    {
        strip_lsp_snippet_placeholders(insert_text)
    } else {
        insert_text.to_owned()
    };
    Some(CodeCompletionItem {
        label,
        detail: value
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("language server")
            .to_owned(),
        cursor_offset: insert_text.chars().count(),
        insert_text,
        kind: CompletionKind::Lsp,
    })
}

fn strip_lsp_snippet_placeholders(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '$' {
            output.push(character);
            continue;
        }
        if characters.peek().is_some_and(|next| next.is_ascii_digit()) {
            while characters.peek().is_some_and(|next| next.is_ascii_digit()) {
                characters.next();
            }
        } else if characters.peek() == Some(&'{') {
            characters.next();
            let mut placeholder = String::new();
            for next in characters.by_ref() {
                if next == '}' {
                    break;
                }
                placeholder.push(next);
            }
            if let Some((_, default)) = placeholder.split_once(':') {
                output.push_str(default);
            }
        } else {
            output.push(character);
        }
    }
    output
}

pub(in crate::app) fn file_uri(path: PathBuf) -> String {
    let raw = path.to_string_lossy();
    let mut uri = String::from("file://");
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{completion_items_from_response, strip_lsp_snippet_placeholders};

    #[test]
    fn lsp_snippets_degrade_to_editable_plain_text() {
        assert_eq!(
            strip_lsp_snippet_placeholders("fn ${1:name}(${2}) { $0 }"),
            "fn name() {  }"
        );
    }

    #[test]
    fn completion_list_accepts_lsp_arrays_and_text_edits() {
        let items = completion_items_from_response(json!({
            "isIncomplete": false,
            "items": [{
                "label": "println!",
                "detail": "macro",
                "textEdit": { "newText": "println!($0);" },
                "insertTextFormat": 2
            }]
        }))
        .unwrap();
        assert_eq!(items[0].insert_text, "println!();");
    }
}
