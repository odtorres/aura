//! Debug Adapter Protocol (DAP) client over stdio.
//!
//! Spawns a debug adapter as a child process and communicates via
//! Content-Length framed DAP messages. A background reader thread
//! sends [`DapEvent`]s to the main event loop through an `mpsc` channel.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

// ── DAP message primitives ──────────────────────────────────────

#[derive(Debug, Serialize)]
struct DapRequest {
    seq: u64,
    #[serde(rename = "type")]
    msg_type: &'static str,
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct DapMessage {
    #[allow(dead_code)]
    seq: u64,
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    request_seq: Option<u64>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    body: Option<serde_json::Value>,
    #[serde(default)]
    message: Option<String>,
}

// ── DAP data types ──────────────────────────────────────────────

/// A stack frame from the debug adapter.
#[derive(Debug, Clone)]
pub struct DapStackFrame {
    /// Frame identifier.
    pub id: u64,
    /// Display name (usually function name).
    pub name: String,
    /// Source file path (if available).
    pub source_path: Option<PathBuf>,
    /// 1-indexed line number in source.
    pub line: u64,
    /// 1-indexed column number.
    pub column: u64,
}

/// A scope within a stack frame.
#[derive(Debug, Clone)]
pub struct DapScope {
    /// Display name (e.g. "Locals", "Globals").
    pub name: String,
    /// Variables reference ID for fetching variables.
    pub variables_reference: u64,
    /// Whether this scope is expensive to evaluate.
    pub expensive: bool,
}

/// A variable from the debug adapter.
#[derive(Debug, Clone)]
pub struct DapVariable {
    /// Variable name.
    pub name: String,
    /// Value as a display string.
    pub value: String,
    /// Type name (if available).
    pub type_name: String,
    /// Non-zero if this variable has children (struct fields, array elements).
    pub variables_reference: u64,
}

/// Result of setting a breakpoint.
#[derive(Debug, Clone)]
pub struct DapBreakpointResult {
    /// Whether the adapter verified this breakpoint is valid.
    pub verified: bool,
    /// Actual line (may differ from requested).
    pub line: Option<u64>,
    /// Optional message from the adapter.
    pub message: Option<String>,
}

/// Configuration for a debug adapter.
#[derive(Debug, Clone)]
pub struct DapAdapterConfig {
    /// Executable command.
    pub command: String,
    /// Command-line arguments.
    pub args: Vec<String>,
}

// ── Events sent from reader thread to App ───────────────────────

/// Events from the debug adapter.
#[derive(Debug)]
pub enum DapEvent {
    /// Adapter initialization complete.
    Initialized,
    /// Execution stopped (breakpoint, step, etc.).
    Stopped {
        /// Thread that stopped.
        thread_id: u64,
        /// Reason (e.g. "breakpoint", "step", "pause").
        reason: String,
    },
    /// Execution continued.
    Continued {
        /// Thread that continued.
        thread_id: u64,
    },
    /// Debug session terminated.
    Terminated,
    /// Output from the debuggee.
    Output {
        /// Category: "console", "stdout", "stderr".
        category: String,
        /// The output text.
        output: String,
    },
    /// Stack trace response.
    StackTrace(Vec<DapStackFrame>),
    /// Scopes response.
    Scopes(Vec<DapScope>),
    /// Variables response.
    Variables {
        /// The variables_reference this response is for.
        reference: u64,
        /// The variables.
        vars: Vec<DapVariable>,
    },
    /// Breakpoints set response.
    BreakpointsSet(Vec<DapBreakpointResult>),
    /// Error from the adapter.
    Error(String),
}

/// Current session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugSessionState {
    /// Not yet launched.
    Inactive,
    /// Program is running.
    Running,
    /// Program is paused (hit breakpoint, step, etc.).
    Stopped,
    /// Debug session ended.
    Terminated,
}

// ── DapClient ───────────────────────────────────────────────────

/// Client handle for a DAP debug adapter process.
pub struct DapClient {
    writer_tx: mpsc::Sender<Vec<u8>>,
    event_rx: mpsc::Receiver<DapEvent>,
    next_seq: u64,
    child: Option<Child>,
    /// Current session state.
    pub state: DebugSessionState,
    /// Pending request tracking: seq -> command name.
    pending_requests: HashMap<u64, String>,
}

impl DapClient {
    /// Spawn a debug adapter and perform the DAP handshake.
    pub fn start(config: &DapAdapterConfig, workspace_root: &Path) -> anyhow::Result<Self> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .current_dir(workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdout"))?;

        let (writer_tx, writer_rx) = mpsc::channel::<Vec<u8>>();
        let (event_tx, event_rx) = mpsc::channel::<DapEvent>();

        // Spawn writer thread.
        std::thread::Builder::new()
            .name("dap-writer".into())
            .spawn(move || dap_writer_thread(stdin, writer_rx))?;

        // Spawn reader thread.
        std::thread::Builder::new()
            .name("dap-reader".into())
            .spawn(move || dap_reader_thread(stdout, event_tx))?;

        let mut client = Self {
            writer_tx,
            event_rx,
            next_seq: 1,
            child: Some(child),
            state: DebugSessionState::Inactive,
            pending_requests: HashMap::new(),
        };

        // Send initialize request.
        client.send_request(
            "initialize",
            serde_json::json!({
                "clientID": "aura",
                "clientName": "AURA Editor",
                "adapterID": config.command,
                "pathFormat": "path",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "supportsVariableType": true,
                "supportsVariablePaging": false,
                "supportsRunInTerminalRequest": false,
                "locale": "en-US"
            }),
        );

        Ok(client)
    }

    /// Send a `launch` request to start debugging a program.
    pub fn launch(&mut self, program: &str, args: &[String], cwd: &Path) {
        self.send_request(
            "launch",
            serde_json::json!({
                "program": program,
                "args": args,
                "cwd": cwd.display().to_string(),
                "stopOnEntry": false,
                "type": "lldb",
                "request": "launch"
            }),
        );
    }

    /// Send an `attach` request.
    pub fn attach(&mut self, pid: u64) {
        self.send_request(
            "attach",
            serde_json::json!({
                "pid": pid
            }),
        );
    }

    /// Set breakpoints for a source file (replaces all breakpoints in that file).
    pub fn set_breakpoints(&mut self, source_path: &Path, lines: &[usize]) {
        let bps: Vec<(usize, Option<String>)> = lines.iter().map(|&l| (l, None)).collect();
        self.set_breakpoints_with_conditions(source_path, &bps);
    }

    /// Set breakpoints with optional conditions for a source file.
    pub fn set_breakpoints_with_conditions(
        &mut self,
        source_path: &Path,
        breakpoints: &[(usize, Option<String>)],
    ) {
        let bp_values: Vec<serde_json::Value> = breakpoints
            .iter()
            .map(|(line, condition)| {
                let mut bp = serde_json::json!({ "line": line + 1 }); // DAP uses 1-indexed
                if let Some(cond) = condition {
                    bp.as_object_mut()
                        .unwrap()
                        .insert("condition".into(), serde_json::json!(cond));
                }
                bp
            })
            .collect();

        self.send_request(
            "setBreakpoints",
            serde_json::json!({
                "source": {
                    "path": source_path.display().to_string()
                },
                "breakpoints": bp_values,
                "sourceModified": false
            }),
        );
    }

    /// Signal that configuration is done; the debuggee may start running.
    pub fn configuration_done(&mut self) {
        self.send_request("configurationDone", serde_json::json!({}));
    }

    /// Continue execution.
    pub fn continue_exec(&mut self, thread_id: u64) {
        self.send_request("continue", serde_json::json!({ "threadId": thread_id }));
        self.state = DebugSessionState::Running;
    }

    /// Step over (next line).
    pub fn next(&mut self, thread_id: u64) {
        self.send_request("next", serde_json::json!({ "threadId": thread_id }));
        self.state = DebugSessionState::Running;
    }

    /// Step into.
    pub fn step_in(&mut self, thread_id: u64) {
        self.send_request("stepIn", serde_json::json!({ "threadId": thread_id }));
        self.state = DebugSessionState::Running;
    }

    /// Step out.
    pub fn step_out(&mut self, thread_id: u64) {
        self.send_request("stepOut", serde_json::json!({ "threadId": thread_id }));
        self.state = DebugSessionState::Running;
    }

    /// Pause execution.
    pub fn pause(&mut self, thread_id: u64) {
        self.send_request("pause", serde_json::json!({ "threadId": thread_id }));
    }

    /// Request stack trace for a thread.
    pub fn request_stack_trace(&mut self, thread_id: u64) {
        self.send_request(
            "stackTrace",
            serde_json::json!({
                "threadId": thread_id,
                "startFrame": 0,
                "levels": 50
            }),
        );
    }

    /// Request scopes for a stack frame.
    pub fn request_scopes(&mut self, frame_id: u64) {
        self.send_request("scopes", serde_json::json!({ "frameId": frame_id }));
    }

    /// Request variables for a scope or parent variable.
    pub fn request_variables(&mut self, variables_reference: u64) {
        self.send_request(
            "variables",
            serde_json::json!({ "variablesReference": variables_reference }),
        );
    }

    /// Disconnect from the debug adapter.
    pub fn disconnect(&mut self) {
        self.send_request(
            "disconnect",
            serde_json::json!({ "restart": false, "terminateDebuggee": true }),
        );
        self.state = DebugSessionState::Terminated;
    }

    /// Poll for events from the debug adapter (non-blocking).
    pub fn poll_events(&mut self) -> Vec<DapEvent> {
        let mut events = Vec::new();
        loop {
            match self.event_rx.try_recv() {
                Ok(e) => {
                    // Update our state from events.
                    match &e {
                        DapEvent::Stopped { .. } => self.state = DebugSessionState::Stopped,
                        DapEvent::Continued { .. } => self.state = DebugSessionState::Running,
                        DapEvent::Terminated => self.state = DebugSessionState::Terminated,
                        _ => {}
                    }
                    events.push(e);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    events.push(DapEvent::Error(
                        "DAP reader thread disconnected".to_string(),
                    ));
                    self.state = DebugSessionState::Terminated;
                    break;
                }
            }
        }
        events
    }

    fn alloc_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    fn send_request(&mut self, command: &str, arguments: serde_json::Value) {
        let seq = self.alloc_seq();
        self.pending_requests.insert(seq, command.to_string());
        let msg = DapRequest {
            seq,
            msg_type: "request",
            command: command.to_string(),
            arguments: Some(arguments),
        };
        if let Ok(bytes) = serde_json::to_vec(&msg) {
            let _ = self.writer_tx.send(bytes);
        }
    }
}

impl Drop for DapClient {
    fn drop(&mut self) {
        // Best-effort disconnect.
        let seq = self.alloc_seq();
        let msg = DapRequest {
            seq,
            msg_type: "request",
            command: "disconnect".to_string(),
            arguments: Some(serde_json::json!({ "terminateDebuggee": true })),
        };
        if let Ok(bytes) = serde_json::to_vec(&msg) {
            let _ = self.writer_tx.send(bytes);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// ── Background threads ──────────────────────────────────────────

/// Write Content-Length framed messages to the adapter's stdin.
fn dap_writer_thread(mut stdin: std::process::ChildStdin, rx: mpsc::Receiver<Vec<u8>>) {
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

/// Read Content-Length framed messages from the adapter's stdout and
/// dispatch them as [`DapEvent`]s.
fn dap_reader_thread(stdout: std::process::ChildStdout, tx: mpsc::Sender<DapEvent>) {
    let mut reader = BufReader::new(stdout);

    loop {
        let msg = match read_dap_message(&mut reader) {
            Ok(m) => m,
            Err(e) => {
                let _ = tx.send(DapEvent::Error(e.to_string()));
                return;
            }
        };

        match msg.msg_type.as_str() {
            "event" => {
                if let Some(event_name) = &msg.event {
                    let evt = parse_dap_event(event_name, msg.body.as_ref());
                    if let Some(e) = evt {
                        if tx.send(e).is_err() {
                            return;
                        }
                    }
                }
            }
            "response" => {
                if let Some(false) = msg.success {
                    let err_msg = msg.message.unwrap_or_else(|| "unknown error".to_string());
                    let _ = tx.send(DapEvent::Error(err_msg));
                    continue;
                }
                if let Some(command) = &msg.command {
                    let evt = parse_dap_response(command, msg.body.as_ref());
                    if let Some(e) = evt {
                        if tx.send(e).is_err() {
                            return;
                        }
                    }
                }
            }
            _ => {} // Ignore unknown message types.
        }
    }
}

/// Parse a DAP event into a [`DapEvent`].
fn parse_dap_event(event: &str, body: Option<&serde_json::Value>) -> Option<DapEvent> {
    match event {
        "initialized" => Some(DapEvent::Initialized),
        "stopped" => {
            let body = body?;
            let thread_id = body.get("threadId")?.as_u64().unwrap_or(1);
            let reason = body
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            Some(DapEvent::Stopped { thread_id, reason })
        }
        "continued" => {
            let body = body?;
            let thread_id = body.get("threadId")?.as_u64().unwrap_or(1);
            Some(DapEvent::Continued { thread_id })
        }
        "terminated" | "exited" => Some(DapEvent::Terminated),
        "output" => {
            let body = body?;
            let category = body
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("console")
                .to_string();
            let output = body
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(DapEvent::Output { category, output })
        }
        _ => None,
    }
}

/// Parse a DAP response into a [`DapEvent`].
fn parse_dap_response(command: &str, body: Option<&serde_json::Value>) -> Option<DapEvent> {
    match command {
        "initialize" => Some(DapEvent::Initialized),
        "stackTrace" => {
            let body = body?;
            let frames_val = body.get("stackFrames")?.as_array()?;
            let frames = frames_val
                .iter()
                .filter_map(|f| {
                    Some(DapStackFrame {
                        id: f.get("id")?.as_u64()?,
                        name: f.get("name")?.as_str()?.to_string(),
                        source_path: f
                            .get("source")
                            .and_then(|s| s.get("path"))
                            .and_then(|p| p.as_str())
                            .map(PathBuf::from),
                        line: f.get("line")?.as_u64()?,
                        column: f.get("column").and_then(|c| c.as_u64()).unwrap_or(1),
                    })
                })
                .collect();
            Some(DapEvent::StackTrace(frames))
        }
        "scopes" => {
            let body = body?;
            let scopes_val = body.get("scopes")?.as_array()?;
            let scopes = scopes_val
                .iter()
                .filter_map(|s| {
                    Some(DapScope {
                        name: s.get("name")?.as_str()?.to_string(),
                        variables_reference: s.get("variablesReference")?.as_u64()?,
                        expensive: s
                            .get("expensive")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false),
                    })
                })
                .collect();
            Some(DapEvent::Scopes(scopes))
        }
        "variables" => {
            let body = body?;
            let vars_val = body.get("variables")?.as_array()?;
            let vars: Vec<DapVariable> = vars_val
                .iter()
                .filter_map(|v| {
                    Some(DapVariable {
                        name: v.get("name")?.as_str()?.to_string(),
                        value: v.get("value")?.as_str()?.to_string(),
                        type_name: v
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string(),
                        variables_reference: v
                            .get("variablesReference")
                            .and_then(|r| r.as_u64())
                            .unwrap_or(0),
                    })
                })
                .collect();
            // We don't have the original reference here, but the App can track it.
            Some(DapEvent::Variables { reference: 0, vars })
        }
        "setBreakpoints" => {
            let body = body?;
            let bps = body.get("breakpoints")?.as_array()?;
            let results = bps
                .iter()
                .filter_map(|b| {
                    Some(DapBreakpointResult {
                        verified: b.get("verified")?.as_bool()?,
                        line: b.get("line").and_then(|l| l.as_u64()),
                        message: b.get("message").and_then(|m| m.as_str()).map(String::from),
                    })
                })
                .collect();
            Some(DapEvent::BreakpointsSet(results))
        }
        _ => None,
    }
}

/// Read a single Content-Length framed DAP message.
fn read_dap_message(reader: &mut impl BufRead) -> anyhow::Result<DapMessage> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(anyhow::anyhow!("adapter closed connection"));
        }
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some(len_str) = header.strip_prefix("Content-Length: ") {
            content_length = Some(len_str.parse()?);
        }
    }

    let length = content_length.ok_or_else(|| anyhow::anyhow!("missing Content-Length"))?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    let msg: DapMessage = serde_json::from_slice(&body)?;
    Ok(msg)
}

// ── Auto-detection ──────────────────────────────────────────────

/// Attempt to detect a debug adapter for the given file extension.
pub fn detect_debug_adapter(ext: &str) -> Option<DapAdapterConfig> {
    match ext {
        "rs" | "c" | "cpp" | "cxx" | "cc" | "h" | "hpp" => {
            // Try codelldb first, then lldb-dap (formerly lldb-vscode).
            if command_exists("codelldb") {
                Some(DapAdapterConfig {
                    command: "codelldb".to_string(),
                    args: vec!["--port".to_string(), "0".to_string()],
                })
            } else if command_exists("lldb-dap") {
                Some(DapAdapterConfig {
                    command: "lldb-dap".to_string(),
                    args: vec![],
                })
            } else {
                None
            }
        }
        "py" => {
            if command_exists("python3") {
                Some(DapAdapterConfig {
                    command: "python3".to_string(),
                    args: vec!["-m".to_string(), "debugpy.adapter".to_string()],
                })
            } else if command_exists("python") {
                Some(DapAdapterConfig {
                    command: "python".to_string(),
                    args: vec!["-m".to_string(), "debugpy.adapter".to_string()],
                })
            } else {
                None
            }
        }
        "go" => {
            if command_exists("dlv") {
                Some(DapAdapterConfig {
                    command: "dlv".to_string(),
                    args: vec!["dap".to_string()],
                })
            } else {
                None
            }
        }
        "js" | "ts" | "mjs" | "cjs" | "mts" => {
            if command_exists("node") {
                Some(DapAdapterConfig {
                    command: "node".to_string(),
                    args: vec![],
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if a command exists on `$PATH`.
fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_dap_message() {
        let body = r#"{"seq":1,"type":"event","event":"initialized"}"#;
        let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut reader = Cursor::new(framed.as_bytes());
        let msg = read_dap_message(&mut reader).unwrap();
        assert_eq!(msg.msg_type, "event");
        assert_eq!(msg.event.as_deref(), Some("initialized"));
    }

    #[test]
    fn test_parse_stopped_event() {
        let body = serde_json::json!({
            "reason": "breakpoint",
            "threadId": 1
        });
        let evt = parse_dap_event("stopped", Some(&body));
        assert!(matches!(evt, Some(DapEvent::Stopped { thread_id: 1, .. })));
    }

    #[test]
    fn test_parse_stack_trace_response() {
        let body = serde_json::json!({
            "stackFrames": [
                {
                    "id": 1,
                    "name": "main",
                    "source": { "path": "/tmp/test.rs" },
                    "line": 42,
                    "column": 1
                }
            ]
        });
        let evt = parse_dap_response("stackTrace", Some(&body));
        match evt {
            Some(DapEvent::StackTrace(frames)) => {
                assert_eq!(frames.len(), 1);
                assert_eq!(frames[0].name, "main");
                assert_eq!(frames[0].line, 42);
            }
            _ => panic!("expected StackTrace event"),
        }
    }

    #[test]
    fn test_parse_variables_response() {
        let body = serde_json::json!({
            "variables": [
                {
                    "name": "x",
                    "value": "42",
                    "type": "i32",
                    "variablesReference": 0
                },
                {
                    "name": "items",
                    "value": "Vec<i32>(3)",
                    "type": "Vec<i32>",
                    "variablesReference": 5
                }
            ]
        });
        let evt = parse_dap_response("variables", Some(&body));
        match evt {
            Some(DapEvent::Variables { vars, .. }) => {
                assert_eq!(vars.len(), 2);
                assert_eq!(vars[0].name, "x");
                assert_eq!(vars[1].variables_reference, 5);
            }
            _ => panic!("expected Variables event"),
        }
    }

    #[test]
    fn test_parse_breakpoints_response() {
        let body = serde_json::json!({
            "breakpoints": [
                { "verified": true, "line": 10 },
                { "verified": false, "line": 20, "message": "not a valid location" }
            ]
        });
        let evt = parse_dap_response("setBreakpoints", Some(&body));
        match evt {
            Some(DapEvent::BreakpointsSet(results)) => {
                assert_eq!(results.len(), 2);
                assert!(results[0].verified);
                assert!(!results[1].verified);
            }
            _ => panic!("expected BreakpointsSet event"),
        }
    }
}
