//! Lightweight LSP client over JSON-RPC / stdio.
//!
//! Spawns a language server as a child process and communicates via
//! Content-Length framed JSON-RPC messages. A background reader thread
//! sends [`LspEvent`]s to the main event loop through an `mpsc` channel.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

// ── JSON-RPC primitives ───────────────────────────────────────────

#[derive(Debug, Serialize)]
struct JsonRpcMessage {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

// ── LSP protocol types (minimal subset) ───────────────────────────

/// Position in a text document (0-indexed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

/// A range in a text document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

/// A location in a text document.
#[derive(Debug, Clone, Deserialize)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

/// A diagnostic from the language server.
#[derive(Debug, Clone, Deserialize)]
pub struct Diagnostic {
    pub range: LspRange,
    #[serde(default)]
    pub severity: Option<u32>,
    pub message: String,
    #[serde(default)]
    pub source: Option<String>,
}

impl Diagnostic {
    /// Whether this is an error (severity 1).
    pub fn is_error(&self) -> bool {
        self.severity == Some(1)
    }

    /// Whether this is a warning (severity 2).
    pub fn is_warning(&self) -> bool {
        self.severity == Some(2)
    }
}

/// Hover response from the language server.
#[derive(Debug, Clone, Deserialize)]
pub struct HoverResult {
    pub contents: serde_json::Value,
}

impl HoverResult {
    /// Extract plain text from the hover contents.
    pub fn to_text(&self) -> String {
        match &self.contents {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Object(obj) => {
                // MarkupContent: { kind, value }
                obj.get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            }
            serde_json::Value::Array(arr) => {
                // Array of MarkedString
                arr.iter()
                    .filter_map(|v| match v {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(o) => {
                            o.get("value").and_then(|v| v.as_str()).map(String::from)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => String::new(),
        }
    }
}

// ── Events from the LSP background thread to App ──────────────────

/// Events sent from the LSP reader thread to the main event loop.
#[derive(Debug)]
pub enum LspEvent {
    /// Server completed initialisation.
    Initialized,
    /// Diagnostics published for the open file.
    Diagnostics(Vec<Diagnostic>),
    /// Response to a go-to-definition request.
    Definition(Vec<LspLocation>),
    /// Response to a hover request.
    Hover(Option<HoverResult>),
    /// The server crashed or encountered a fatal error.
    ServerError(String),
}

// ── Server configuration ──────────────────────────────────────────

/// How to spawn a particular language server.
#[derive(Debug, Clone)]
pub struct LspServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub language_id: String,
}

/// Detect the appropriate language server for a file extension.
///
/// Returns `None` if the extension is unsupported or the binary is not on PATH.
pub fn detect_server(extension: &str) -> Option<LspServerConfig> {
    let (cmd, args, lang_id) = match extension {
        "rs" => ("rust-analyzer", vec![], "rust"),
        "py" => ("pyright-langserver", vec!["--stdio".to_string()], "python"),
        "ts" | "js" => (
            "typescript-language-server",
            vec!["--stdio".to_string()],
            "typescript",
        ),
        "tsx" | "jsx" => (
            "typescript-language-server",
            vec!["--stdio".to_string()],
            "typescriptreact",
        ),
        "go" => ("gopls", vec!["serve".to_string()], "go"),
        _ => return None,
    };

    // Check if the binary exists on PATH.
    let found = Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !found {
        return None;
    }

    Some(LspServerConfig {
        command: cmd.to_string(),
        args,
        language_id: lang_id.to_string(),
    })
}

// ── LspClient ─────────────────────────────────────────────────────

/// Handle to a running language server.
pub struct LspClient {
    /// Sender for outgoing JSON-RPC messages (to writer thread).
    writer_tx: mpsc::Sender<Vec<u8>>,
    /// Receiver for incoming events (from reader thread).
    event_rx: mpsc::Receiver<LspEvent>,
    /// Next request ID.
    next_id: u64,
    /// The file:// URI of the open document.
    document_uri: String,
    /// Document version counter.
    document_version: i32,
    /// Language ID for the open document (reserved for future use).
    _language_id: String,
    /// Child process handle.
    child: Option<Child>,
}

impl LspClient {
    /// Spawn a language server and begin the initialisation handshake.
    ///
    /// The handshake (initialize → initialized → didOpen) happens in
    /// the background. Poll events for `LspEvent::Initialized`.
    pub fn start(
        config: &LspServerConfig,
        workspace_root: &Path,
        file_path: &Path,
        file_content: &str,
    ) -> anyhow::Result<Self> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .current_dir(workspace_root)
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to open language server stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to open language server stdout"))?;

        let (writer_tx, writer_rx) = mpsc::channel::<Vec<u8>>();
        let (event_tx, event_rx) = mpsc::channel::<LspEvent>();

        // Writer thread: sends framed messages to the server's stdin.
        std::thread::spawn(move || {
            writer_thread(stdin, writer_rx);
        });

        // Reader thread: reads framed messages from stdout.
        std::thread::spawn(move || {
            reader_thread(stdout, event_tx);
        });

        let document_uri = path_to_uri(file_path);
        let workspace_uri = path_to_uri(workspace_root);

        let mut client = Self {
            writer_tx,
            event_rx,
            next_id: 1,
            document_uri: document_uri.clone(),
            document_version: 0,
            _language_id: config.language_id.clone(),
            child: Some(child),
        };

        // Send initialize request.
        let init_id = client.alloc_id();
        client.send_request(
            init_id,
            "initialize",
            serde_json::json!({
                "processId": std::process::id(),
                "rootUri": workspace_uri,
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": {
                            "relatedInformation": false
                        },
                        "definition": {
                            "dynamicRegistration": false
                        },
                        "hover": {
                            "dynamicRegistration": false,
                            "contentFormat": ["plaintext", "markdown"]
                        },
                        "synchronization": {
                            "didSave": true
                        }
                    }
                }
            }),
        );

        // Send initialized notification (no id).
        client.send_notification("initialized", serde_json::json!({}));

        // Send textDocument/didOpen.
        client.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": document_uri,
                    "languageId": config.language_id,
                    "version": 0,
                    "text": file_content
                }
            }),
        );

        Ok(client)
    }

    /// Notify the server that the document content changed.
    pub fn did_change(&mut self, text: &str) {
        self.document_version += 1;
        self.send_notification(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": {
                    "uri": self.document_uri,
                    "version": self.document_version
                },
                "contentChanges": [{ "text": text }]
            }),
        );
    }

    /// Request go-to-definition at the given position.
    pub fn goto_definition(&mut self, line: u32, character: u32) {
        let id = self.alloc_id();
        self.send_request(
            id,
            "textDocument/definition",
            serde_json::json!({
                "textDocument": { "uri": self.document_uri },
                "position": { "line": line, "character": character }
            }),
        );
    }

    /// Request hover information at the given position.
    pub fn hover(&mut self, line: u32, character: u32) {
        let id = self.alloc_id();
        self.send_request(
            id,
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": self.document_uri },
                "position": { "line": line, "character": character }
            }),
        );
    }

    /// Send shutdown + exit to the server.
    pub fn shutdown(&mut self) {
        let id = self.alloc_id();
        self.send_request(id, "shutdown", serde_json::json!(null));
        self.send_notification("exit", serde_json::json!(null));
    }

    /// Poll for events from the language server (non-blocking).
    pub fn poll_events(&self) -> Vec<LspEvent> {
        let mut events = Vec::new();
        loop {
            match self.event_rx.try_recv() {
                Ok(e) => events.push(e),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    events.push(LspEvent::ServerError(
                        "LSP reader thread disconnected".to_string(),
                    ));
                    break;
                }
            }
        }
        events
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn send_request(&self, id: u64, method: &str, params: serde_json::Value) {
        let msg = JsonRpcMessage {
            jsonrpc: "2.0",
            id: Some(id),
            method: method.to_string(),
            params: Some(params),
        };
        if let Ok(bytes) = serde_json::to_vec(&msg) {
            let _ = self.writer_tx.send(bytes);
        }
    }

    fn send_notification(&self, method: &str, params: serde_json::Value) {
        let msg = JsonRpcMessage {
            jsonrpc: "2.0",
            id: None,
            method: method.to_string(),
            params: Some(params),
        };
        if let Ok(bytes) = serde_json::to_vec(&msg) {
            let _ = self.writer_tx.send(bytes);
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Best-effort shutdown.
        let id = self.alloc_id();
        self.send_request(id, "shutdown", serde_json::json!(null));
        self.send_notification("exit", serde_json::json!(null));
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// ── Background threads ────────────────────────────────────────────

/// Write Content-Length framed messages to the server's stdin.
fn writer_thread(mut stdin: std::process::ChildStdin, rx: mpsc::Receiver<Vec<u8>>) {
    for body in rx {
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        if stdin.write_all(header.as_bytes()).is_err() {
            break;
        }
        if stdin.write_all(&body).is_err() {
            break;
        }
        if stdin.flush().is_err() {
            break;
        }
    }
}

/// Read Content-Length framed messages from the server's stdout and
/// dispatch them as [`LspEvent`]s.
fn reader_thread(stdout: std::process::ChildStdout, tx: mpsc::Sender<LspEvent>) {
    let mut reader = BufReader::new(stdout);

    loop {
        let msg = match read_message(&mut reader) {
            Ok(m) => m,
            Err(e) => {
                let _ = tx.send(LspEvent::ServerError(e.to_string()));
                return;
            }
        };

        // Server notification (no id).
        if let Some(method) = &msg.method {
            if method == "textDocument/publishDiagnostics" {
                if let Some(params) = msg.params {
                    if let Ok(diags) = serde_json::from_value::<PublishDiagnosticsParams>(params) {
                        let _ = tx.send(LspEvent::Diagnostics(diags.diagnostics));
                    }
                }
            }
            // Ignore other notifications (window/logMessage, etc.).
            continue;
        }

        // Response to a request.
        if msg.id.is_some() {
            if let Some(error) = msg.error {
                tracing::warn!("LSP error: {}", error.message);
                continue;
            }

            // We don't track which id maps to which method, so we infer
            // from the response shape. This is fine for our small protocol
            // surface (definition returns Location[], hover returns Hover).
            if let Some(result) = msg.result {
                if result.is_null() {
                    // Could be a shutdown response or empty result.
                    continue;
                }

                // Try as definition (array of Location).
                if let Ok(locations) = serde_json::from_value::<Vec<LspLocation>>(result.clone()) {
                    let _ = tx.send(LspEvent::Definition(locations));
                    continue;
                }
                // Single location.
                if let Ok(loc) = serde_json::from_value::<LspLocation>(result.clone()) {
                    let _ = tx.send(LspEvent::Definition(vec![loc]));
                    continue;
                }
                // Try as hover.
                if let Ok(hover) = serde_json::from_value::<HoverResult>(result.clone()) {
                    let _ = tx.send(LspEvent::Hover(Some(hover)));
                    continue;
                }
                // Initialize response — emit Initialized event.
                if result.get("capabilities").is_some() {
                    let _ = tx.send(LspEvent::Initialized);
                    continue;
                }
            }
        }
    }
}

/// Read a single Content-Length framed JSON-RPC message.
fn read_message(reader: &mut impl BufRead) -> anyhow::Result<JsonRpcResponse> {
    // Read headers until the empty line.
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(anyhow::anyhow!("server closed connection"));
        }
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some(len_str) = header.strip_prefix("Content-Length: ") {
            content_length = Some(len_str.parse()?);
        }
        // Ignore other headers (Content-Type, etc.).
    }

    let length = content_length.ok_or_else(|| anyhow::anyhow!("missing Content-Length"))?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    let msg: JsonRpcResponse = serde_json::from_slice(&body)?;
    Ok(msg)
}

/// Helper for deserialising publishDiagnostics params.
#[derive(Debug, Deserialize)]
struct PublishDiagnosticsParams {
    #[allow(dead_code)]
    uri: String,
    diagnostics: Vec<Diagnostic>,
}

/// Convert a file path to a file:// URI.
fn path_to_uri(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    format!("file://{}", abs.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_message() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}"#;
        let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut reader = Cursor::new(framed.as_bytes());
        let msg = read_message(&mut reader).unwrap();
        assert_eq!(msg.id, Some(1));
        assert!(msg.result.is_some());
    }

    #[test]
    fn test_path_to_uri() {
        let uri = path_to_uri(Path::new("/foo/bar.rs"));
        assert_eq!(uri, "file:///foo/bar.rs");
    }

    #[test]
    fn test_diagnostic_severity() {
        let d = Diagnostic {
            range: LspRange {
                start: LspPosition {
                    line: 0,
                    character: 0,
                },
                end: LspPosition {
                    line: 0,
                    character: 5,
                },
            },
            severity: Some(1),
            message: "error".to_string(),
            source: None,
        };
        assert!(d.is_error());
        assert!(!d.is_warning());
    }

    #[test]
    fn test_hover_result_to_text() {
        // String contents.
        let hr = HoverResult {
            contents: serde_json::json!("hello"),
        };
        assert_eq!(hr.to_text(), "hello");

        // MarkupContent.
        let hr = HoverResult {
            contents: serde_json::json!({"kind": "plaintext", "value": "world"}),
        };
        assert_eq!(hr.to_text(), "world");
    }

    #[test]
    fn test_detect_server_unknown_ext() {
        assert!(detect_server("xyz").is_none());
    }
}
