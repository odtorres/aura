// Some ACP response fields are populated for protocol round-tripping but
// not yet consumed by callers (e.g. `success` on async ACK responses).
// Allowed at module level to reflect that this is a protocol surface, not
// dead code waiting to be deleted.
#![allow(dead_code)]

//! ACP (Agent Client Protocol) server for AURA.
//!
//! Implements the Agent Client Protocol, enabling external AI agents
//! (Claude Code, Copilot CLI, Gemini CLI, Codex, etc.) to drive the editor.
//! Uses JSON-RPC 2.0 with Content-Length framing over localhost TCP.
//!
//! ACP is the emerging standard created by Zed + JetBrains for agent-editor
//! communication. AURA is one of the first terminal editors with native support.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

// ── Protocol types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
}

// ── ACP capabilities ────────────────────────────────────────────

/// Actions an ACP agent can request from the editor.
#[derive(Debug)]
pub enum AcpAction {
    /// Get editor info (name, version, capabilities).
    GetEditorInfo,
    /// Read the current document content.
    ReadDocument,
    /// Read a specific file by path.
    ReadFile {
        /// File path to read.
        path: String,
    },
    /// Apply an edit to the current document.
    ApplyEdit {
        /// Start line (0-indexed).
        start_line: usize,
        /// Start column (0-indexed).
        start_col: usize,
        /// End line (0-indexed).
        end_line: usize,
        /// End column (0-indexed).
        end_col: usize,
        /// Replacement text.
        text: String,
    },
    /// Get the current cursor position and context.
    GetCursorContext,
    /// Get LSP diagnostics.
    GetDiagnostics,
    /// Get the current selection.
    GetSelection,
    /// List open files.
    ListOpenFiles,
    /// Open a file.
    OpenFile {
        /// File path to open.
        path: String,
    },
    /// Run a shell command.
    RunCommand {
        /// The command string to execute.
        command: String,
    },
    /// Get project structure (file tree).
    GetProjectStructure,
}

/// A request from the ACP server thread to the App event loop.
#[derive(Debug)]
pub struct AcpAppRequest {
    /// The action being requested.
    pub action: AcpAction,
    /// Channel to send the response back.
    pub response_tx: mpsc::Sender<AcpAppResponse>,
}

/// Response from the App to an ACP request.
#[derive(Debug)]
pub struct AcpAppResponse {
    /// Whether the action succeeded.
    pub success: bool,
    /// JSON result data.
    pub data: serde_json::Value,
}

// ── ACP Server ──────────────────────────────────────────────────

/// ACP server handle.
pub struct AcpServer {
    /// The port the server is listening on.
    pub port: u16,
    /// Receiver for requests from agent connections.
    request_rx: mpsc::Receiver<AcpAppRequest>,
    /// Shutdown flag.
    shutdown: Arc<Mutex<bool>>,
}

impl AcpServer {
    /// Start the ACP server on a random available port.
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(false)?;

        let (request_tx, request_rx) = mpsc::channel::<AcpAppRequest>();
        let shutdown = Arc::new(Mutex::new(false));
        let shutdown_accept = shutdown.clone();

        thread::Builder::new()
            .name("acp-accept".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if *shutdown_accept.lock().expect("lock poisoned") {
                        break;
                    }
                    let stream = match stream {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let tx = request_tx.clone();
                    let sd = shutdown_accept.clone();
                    thread::Builder::new()
                        .name("acp-agent".into())
                        .spawn(move || {
                            handle_agent_connection(stream, tx, sd);
                        })
                        .ok();
                }
            })?;

        Ok(Self {
            port,
            request_rx,
            shutdown,
        })
    }

    /// Poll for pending requests (non-blocking).
    pub fn poll_requests(&self) -> Vec<AcpAppRequest> {
        let mut requests = Vec::new();
        loop {
            match self.request_rx.try_recv() {
                Ok(req) => requests.push(req),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        requests
    }

    /// Shut down the server.
    pub fn shutdown(&self) {
        *self.shutdown.lock().expect("lock poisoned") = true;
        // Poke the listener to unblock accept.
        let _ = TcpStream::connect(format!("127.0.0.1:{}", self.port));
    }
}

// ── Connection handler ──────────────────────────────────────────

fn handle_agent_connection(
    stream: TcpStream,
    request_tx: mpsc::Sender<AcpAppRequest>,
    shutdown: Arc<Mutex<bool>>,
) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut writer = stream;

    loop {
        if *shutdown.lock().expect("lock poisoned") {
            break;
        }

        let msg = match read_message(&mut reader) {
            Ok(m) => m,
            Err(_) => break,
        };

        let request: JsonRpcRequest = match serde_json::from_str(&msg) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(serde_json::json!({
                        "code": -32700,
                        "message": format!("Parse error: {e}")
                    })),
                };
                let _ = write_response(&mut writer, &resp);
                continue;
            }
        };

        let response = match request.method.as_str() {
            "initialize" => JsonRpcResponse {
                jsonrpc: "2.0",
                id: request.id,
                result: Some(serde_json::json!({
                    "name": "AURA Editor",
                    "version": env!("CARGO_PKG_VERSION"),
                    "protocol": "acp",
                    "capabilities": {
                        "documentSync": true,
                        "diagnostics": true,
                        "edit": true,
                        "selection": true,
                        "fileOperations": true,
                        "terminal": true,
                        "projectStructure": true
                    }
                })),
                error: None,
            },
            "document/read" => dispatch_action(&request, &request_tx, AcpAction::ReadDocument),
            "document/edit" => {
                let p = &request.params;
                let action = AcpAction::ApplyEdit {
                    start_line: p["startLine"].as_u64().unwrap_or(0) as usize,
                    start_col: p["startColumn"].as_u64().unwrap_or(0) as usize,
                    end_line: p["endLine"].as_u64().unwrap_or(0) as usize,
                    end_col: p["endColumn"].as_u64().unwrap_or(0) as usize,
                    text: p["text"].as_str().unwrap_or("").to_string(),
                };
                dispatch_action(&request, &request_tx, action)
            }
            "cursor/context" => dispatch_action(&request, &request_tx, AcpAction::GetCursorContext),
            "diagnostics/get" => dispatch_action(&request, &request_tx, AcpAction::GetDiagnostics),
            "selection/get" => dispatch_action(&request, &request_tx, AcpAction::GetSelection),
            "file/read" => {
                let path = request.params["path"].as_str().unwrap_or("").to_string();
                dispatch_action(&request, &request_tx, AcpAction::ReadFile { path })
            }
            "file/list" => dispatch_action(&request, &request_tx, AcpAction::ListOpenFiles),
            "file/open" => {
                let path = request.params["path"].as_str().unwrap_or("").to_string();
                dispatch_action(&request, &request_tx, AcpAction::OpenFile { path })
            }
            "editor/info" => dispatch_action(&request, &request_tx, AcpAction::GetEditorInfo),
            "terminal/run" => {
                let command = request.params["command"].as_str().unwrap_or("").to_string();
                dispatch_action(&request, &request_tx, AcpAction::RunCommand { command })
            }
            "project/structure" => {
                dispatch_action(&request, &request_tx, AcpAction::GetProjectStructure)
            }
            "shutdown" => break,
            _ => JsonRpcResponse {
                jsonrpc: "2.0",
                id: request.id,
                result: None,
                error: Some(serde_json::json!({
                    "code": -32601,
                    "message": format!("Method not found: {}", request.method)
                })),
            },
        };

        if write_response(&mut writer, &response).is_err() {
            break;
        }
    }
}

/// Dispatch an action to the app and wait for response.
fn dispatch_action(
    request: &JsonRpcRequest,
    request_tx: &mpsc::Sender<AcpAppRequest>,
    action: AcpAction,
) -> JsonRpcResponse {
    let (response_tx, response_rx) = mpsc::channel();
    let app_request = AcpAppRequest {
        action,
        response_tx,
    };

    if request_tx.send(app_request).is_err() {
        return JsonRpcResponse {
            jsonrpc: "2.0",
            id: request.id.clone(),
            result: None,
            error: Some(serde_json::json!({
                "code": -32603,
                "message": "Editor not responding"
            })),
        };
    }

    match response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(resp) => JsonRpcResponse {
            jsonrpc: "2.0",
            id: request.id.clone(),
            result: Some(resp.data),
            error: None,
        },
        Err(_) => JsonRpcResponse {
            jsonrpc: "2.0",
            id: request.id.clone(),
            result: None,
            error: Some(serde_json::json!({
                "code": -32603,
                "message": "Request timed out"
            })),
        },
    }
}

// ── Framing ─────────────────────────────────────────────────────

fn read_message(reader: &mut impl BufRead) -> std::io::Result<String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "connection closed",
            ));
        }
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some(len) = header.strip_prefix("Content-Length: ") {
            content_length = len.parse().ok();
        }
    }
    let length = content_length.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
    })?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn write_response(writer: &mut impl IoWrite, response: &JsonRpcResponse) -> std::io::Result<()> {
    let body = serde_json::to_string(response).map_err(std::io::Error::other)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes())?;
    writer.write_all(body.as_bytes())?;
    writer.flush()
}
