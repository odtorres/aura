//! MCP (Model Context Protocol) server for AURA.
//!
//! Exposes editor state as MCP tools and resources over a localhost TCP server.
//! Uses JSON-RPC 2.0 with Content-Length framing (same as LSP).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read as IoRead, Write as IoWrite};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

/// An MCP tool definition exposed to clients.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    /// The tool name used to invoke it.
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// An MCP resource definition.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceDefinition {
    /// URI identifying this resource (e.g. `editor://buffer`).
    pub uri: String,
    /// Human-readable name of the resource.
    pub name: String,
    /// Description of the resource content.
    pub description: String,
    /// MIME type of the resource (e.g. `text/plain`).
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// A request from the MCP server thread to the App event loop.
#[derive(Debug)]
pub struct McpAppRequest {
    /// The tool or resource being accessed.
    pub action: McpAction,
    /// Channel to send the response back to the server thread.
    pub response_tx: mpsc::Sender<McpAppResponse>,
}

/// Actions the MCP server can request from the App.
#[derive(Debug)]
pub enum McpAction {
    /// Read the full buffer content or specific lines.
    ReadBuffer {
        /// Optional first line to read (0-indexed).
        start_line: Option<usize>,
        /// Optional last line to read (exclusive).
        end_line: Option<usize>,
    },
    /// Edit the buffer: insert or replace text.
    EditBuffer {
        /// Line where the edit begins (0-indexed).
        start_line: usize,
        /// Column where the edit begins (0-indexed).
        start_col: usize,
        /// Optional line where the edit ends (for replacements).
        end_line: Option<usize>,
        /// Optional column where the edit ends (for replacements).
        end_col: Option<usize>,
        /// The replacement or insertion text.
        text: String,
        /// Identifier of the agent making the edit.
        agent_id: String,
    },
    /// Get current diagnostics.
    GetDiagnostics,
    /// Get current selection text.
    GetSelection,
    /// Get cursor context (position, surrounding code, semantic info).
    GetCursorContext,
    /// Get conversation history for a file/range.
    GetConversationHistory {
        /// Optional first line of the range to query.
        start_line: Option<usize>,
        /// Optional last line of the range to query.
        end_line: Option<usize>,
    },
    /// Get the list of connected agents.
    ListAgents,
    /// Register a new agent.
    RegisterAgent {
        /// Name of the agent to register.
        name: String,
    },
    /// Register a new agent with an optional role.
    RegisterAgentWithRole {
        /// Name of the agent to register.
        name: String,
        /// Optional role to assign at registration time.
        role: Option<String>,
    },
    /// Assign a role to an already-registered agent.
    AssignRole {
        /// Name of the target agent.
        name: String,
        /// Role to assign (e.g. "tests", "review").
        role: String,
    },
    /// Unregister an agent.
    UnregisterAgent {
        /// Name of the agent to remove.
        name: String,
    },
    /// Get buffer metadata (file path, language, line count, modified state).
    GetBufferInfo,
    /// Log a conversation message from an external tool (e.g. Claude Code).
    LogConversation {
        /// Identifier of the agent logging the message.
        agent_id: String,
        /// The conversation message text.
        message: String,
        /// Role of the speaker (e.g. "user", "assistant").
        role: String,
        /// Optional additional context for the message.
        context: Option<String>,
        /// Optional start line the message relates to.
        line_start: Option<usize>,
        /// Optional end line the message relates to.
        line_end: Option<usize>,
    },
    /// Report agent activity (what the agent is currently doing).
    ReportActivity {
        /// Agent identifier.
        agent_id: String,
        /// Activity type (e.g., "thinking", "tool_call", "editing").
        activity_type: String,
        /// Description of the activity.
        description: String,
    },
    /// Get the current editor state (open files, mode, diagnostics).
    GetEditorState,
}

/// Response from the App event loop back to the MCP server thread.
#[derive(Debug, Clone, Serialize)]
pub struct McpAppResponse {
    /// Whether the requested action succeeded.
    pub success: bool,
    /// JSON payload with the action result or error details.
    pub data: serde_json::Value,
}

/// JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Information about a connected agent.
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    /// Unique name/identifier for this agent.
    pub name: String,
    /// When the agent connected.
    pub connected_at: String,
    /// Number of edits made by this agent.
    pub edit_count: usize,
    /// Role assigned to this agent (e.g., "tests", "implementation", "review").
    pub role: Option<String>,
    /// Latest activity description (reported by the agent).
    pub last_activity: Option<String>,
    /// What the agent is currently working on.
    pub current_task: Option<String>,
    /// Total number of activity reports.
    pub activity_count: usize,
}

/// Tracks connected MCP agents.
#[derive(Debug, Clone, Default)]
pub struct AgentRegistry {
    /// Map from agent name to its connection info.
    pub agents: HashMap<String, AgentInfo>,
}

impl AgentRegistry {
    /// Register a new agent. Returns false if already registered.
    pub fn register(&mut self, name: &str) -> bool {
        if self.agents.contains_key(name) {
            return false;
        }
        self.agents.insert(
            name.to_string(),
            AgentInfo {
                name: name.to_string(),
                connected_at: now_iso(),
                edit_count: 0,
                role: None,
                last_activity: None,
                current_task: None,
                activity_count: 0,
            },
        );
        true
    }

    /// Register a new agent with an optional role. Returns false if already registered.
    pub fn register_with_role(&mut self, name: &str, role: Option<String>) -> bool {
        if self.agents.contains_key(name) {
            return false;
        }
        self.agents.insert(
            name.to_string(),
            AgentInfo {
                name: name.to_string(),
                connected_at: now_iso(),
                edit_count: 0,
                role,
                last_activity: None,
                current_task: None,
                activity_count: 0,
            },
        );
        true
    }

    /// Assign a role to an existing agent. Returns false if the agent is not registered.
    pub fn assign_role(&mut self, name: &str, role: String) -> bool {
        if let Some(agent) = self.agents.get_mut(name) {
            agent.role = Some(role);
            true
        } else {
            false
        }
    }

    /// Get the role of an agent, if any.
    pub fn agent_role(&self, name: &str) -> Option<&str> {
        self.agents.get(name)?.role.as_deref()
    }

    /// Unregister an agent.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.agents.remove(name).is_some()
    }

    /// Increment edit count for an agent.
    pub fn record_edit(&mut self, name: &str) {
        if let Some(agent) = self.agents.get_mut(name) {
            agent.edit_count += 1;
        }
    }

    /// Get the number of connected agents.
    pub fn count(&self) -> usize {
        self.agents.len()
    }
}

/// The MCP server handle. Owns the listener thread and provides
/// a channel for the App to receive requests.
pub struct McpServer {
    /// Port the server is listening on.
    pub port: u16,
    /// Receives requests from client handler threads.
    pub request_rx: mpsc::Receiver<McpAppRequest>,
    /// Flag to signal shutdown.
    shutdown: Arc<Mutex<bool>>,
}

impl McpServer {
    /// Start the MCP server on a random available port on localhost.
    /// Returns the server handle or an error.
    pub fn start() -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(false)?;

        let (request_tx, request_rx) = mpsc::channel();
        let shutdown = Arc::new(Mutex::new(false));
        let shutdown_clone = shutdown.clone();

        // Accept connections in a background thread.
        thread::Builder::new()
            .name("mcp-accept".to_string())
            .spawn(move || {
                // Set a timeout so we can check the shutdown flag periodically.
                if let Err(e) = listener.set_nonblocking(false) {
                    tracing::warn!("MCP: set_nonblocking failed: {e}");
                    return;
                }
                let _ = listener
                    .incoming()
                    .try_for_each(|stream_result| -> std::io::Result<()> {
                        // Check shutdown flag.
                        if *shutdown_clone.lock().unwrap() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Interrupted,
                                "shutdown",
                            ));
                        }

                        if let Ok(stream) = stream_result {
                            let tx = request_tx.clone();
                            thread::Builder::new()
                                .name("mcp-client".to_string())
                                .spawn(move || {
                                    handle_connection(stream, tx);
                                })
                                .ok();
                        }
                        Ok(())
                    });
            })?;

        Ok(Self {
            port,
            request_rx,
            shutdown,
        })
    }

    /// Poll for pending MCP requests (non-blocking).
    pub fn poll_requests(&self) -> Vec<McpAppRequest> {
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
        *self.shutdown.lock().unwrap() = true;
        // Connect to ourselves to unblock the accept loop.
        let _ = TcpStream::connect(format!("127.0.0.1:{}", self.port));
    }
}

/// Handle a single MCP client connection.
fn handle_connection(stream: TcpStream, request_tx: mpsc::Sender<McpAppRequest>) {
    let cloned = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("MCP: stream clone failed: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(cloned);
    let mut writer = stream;

    let mut initialized = false;

    while let Ok(msg) = read_message(&mut reader) {
        let request: JsonRpcRequest = match serde_json::from_str(&msg) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {e}"),
                    }),
                };
                let _ = write_message(&mut writer, &serde_json::to_string(&resp).unwrap());
                continue;
            }
        };

        let response = match request.method.as_str() {
            "initialize" => {
                initialized = true;
                handle_initialize(&request)
            }
            "notifications/initialized" => {
                // Client acknowledged — no response needed for notifications.
                continue;
            }
            _ if !initialized => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.unwrap_or(serde_json::Value::Null),
                result: None,
                error: Some(JsonRpcError {
                    code: -32002,
                    message: "Server not initialized".to_string(),
                }),
            },
            "tools/list" => handle_tools_list(&request),
            "tools/call" => handle_tools_call(&request, &request_tx),
            "resources/list" => handle_resources_list(&request),
            "resources/read" => handle_resources_read(&request, &request_tx),
            "shutdown" => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.unwrap_or(serde_json::Value::Null),
                    result: Some(serde_json::Value::Null),
                    error: None,
                };
                let _ = write_message(&mut writer, &serde_json::to_string(&resp).unwrap());
                break;
            }
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.unwrap_or(serde_json::Value::Null),
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                }),
            },
        };

        let resp_str = serde_json::to_string(&response).unwrap();
        if write_message(&mut writer, &resp_str).is_err() {
            break;
        }
    }
}

/// Handle the `initialize` handshake.
fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone().unwrap_or(serde_json::Value::Null),
        result: Some(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {},
                "resources": {}
            },
            "serverInfo": {
                "name": "aura-editor",
                "version": "0.1.0"
            }
        })),
        error: None,
    }
}

/// Handle `tools/list`.
fn handle_tools_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    let tools = vec![
        ToolDefinition {
            name: "read_buffer".to_string(),
            description: "Read the current editor buffer content. Optionally specify line range."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "start_line": { "type": "integer", "description": "Start line (0-indexed, optional)" },
                    "end_line": { "type": "integer", "description": "End line (0-indexed, exclusive, optional)" }
                }
            }),
        },
        ToolDefinition {
            name: "edit_buffer".to_string(),
            description: "Edit the buffer. Replace text at a position or range.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "start_line": { "type": "integer", "description": "Start line (0-indexed)" },
                    "start_col": { "type": "integer", "description": "Start column (0-indexed)" },
                    "end_line": { "type": "integer", "description": "End line (0-indexed, optional for insert)" },
                    "end_col": { "type": "integer", "description": "End column (0-indexed, optional for insert)" },
                    "text": { "type": "string", "description": "Text to insert/replace" },
                    "agent_id": { "type": "string", "description": "Agent identifier for authorship tracking" }
                },
                "required": ["start_line", "start_col", "text", "agent_id"]
            }),
        },
        ToolDefinition {
            name: "get_diagnostics".to_string(),
            description: "Get current LSP diagnostics (errors, warnings).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "get_selection".to_string(),
            description: "Get the current visual selection text and range.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "get_cursor_context".to_string(),
            description: "Get cursor position, surrounding code, semantic info, and file metadata."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "get_conversation_history".to_string(),
            description: "Get conversation history for the current file or a line range."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "start_line": { "type": "integer", "description": "Start line (0-indexed, optional)" },
                    "end_line": { "type": "integer", "description": "End line (0-indexed, optional)" }
                }
            }),
        },
        ToolDefinition {
            name: "register_agent".to_string(),
            description: "Register as a collaborating agent. Required before editing.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Unique agent name/identifier" }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "unregister_agent".to_string(),
            description: "Unregister a collaborating agent.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Agent name to unregister" }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "register_agent_with_role".to_string(),
            description: "Register as a collaborating agent with an assigned role (e.g. 'tests', 'implementation', 'review').".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Unique agent name/identifier" },
                    "role": { "type": "string", "description": "Role for this agent (e.g. 'tests', 'implementation', 'review')" }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "assign_role".to_string(),
            description: "Assign a role to an already-registered agent. Used by the orchestrator to direct agents.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Agent name to assign role to" },
                    "role": { "type": "string", "description": "Role to assign (e.g. 'tests', 'implementation', 'review')" }
                },
                "required": ["name", "role"]
            }),
        },
        ToolDefinition {
            name: "list_agents".to_string(),
            description: "List all currently connected agents, including their roles and edit counts.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "log_conversation".to_string(),
            description: "Log a conversation message into AURA's persistent conversation store. Use this to bridge external tool interactions (e.g. Claude Code reasoning, decisions) into the editor's history.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Agent identifier (e.g. 'claude-code')" },
                    "message": { "type": "string", "description": "The message content to log" },
                    "role": { "type": "string", "enum": ["ai_response", "human_intent", "system"], "description": "Message role" },
                    "context": { "type": "string", "description": "Optional context (e.g. what was being worked on)" },
                    "line_start": { "type": "integer", "description": "Start line of relevant code range (0-indexed, optional)" },
                    "line_end": { "type": "integer", "description": "End line of relevant code range (0-indexed, optional)" }
                },
                "required": ["agent_id", "message", "role"]
            }),
        },
    ];

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone().unwrap_or(serde_json::Value::Null),
        result: Some(serde_json::json!({ "tools": tools })),
        error: None,
    }
}

/// Handle `tools/call` — dispatch to the App via the request channel.
fn handle_tools_call(
    request: &JsonRpcRequest,
    request_tx: &mpsc::Sender<McpAppRequest>,
) -> JsonRpcResponse {
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    let params = request.params.as_ref();

    let tool_name = params
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");

    let arguments = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let action = match tool_name {
        "read_buffer" => McpAction::ReadBuffer {
            start_line: arguments
                .get("start_line")
                .and_then(|v| v.as_u64().map(|n| n as usize)),
            end_line: arguments
                .get("end_line")
                .and_then(|v| v.as_u64().map(|n| n as usize)),
        },
        "edit_buffer" => {
            let start_line = arguments
                .get("start_line")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let start_col = arguments
                .get("start_col")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let end_line = arguments
                .get("end_line")
                .and_then(|v| v.as_u64().map(|n| n as usize));
            let end_col = arguments
                .get("end_col")
                .and_then(|v| v.as_u64().map(|n| n as usize));
            let text = arguments
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let agent_id = arguments
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            McpAction::EditBuffer {
                start_line,
                start_col,
                end_line,
                end_col,
                text,
                agent_id,
            }
        }
        "get_diagnostics" => McpAction::GetDiagnostics,
        "get_selection" => McpAction::GetSelection,
        "get_cursor_context" => McpAction::GetCursorContext,
        "get_conversation_history" => McpAction::GetConversationHistory {
            start_line: arguments
                .get("start_line")
                .and_then(|v| v.as_u64().map(|n| n as usize)),
            end_line: arguments
                .get("end_line")
                .and_then(|v| v.as_u64().map(|n| n as usize)),
        },
        "register_agent" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("agent")
                .to_string();
            McpAction::RegisterAgent { name }
        }
        "register_agent_with_role" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("agent")
                .to_string();
            let role = arguments
                .get("role")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            McpAction::RegisterAgentWithRole { name, role }
        }
        "assign_role" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let role = arguments
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            McpAction::AssignRole { name, role }
        }
        "unregister_agent" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            McpAction::UnregisterAgent { name }
        }
        "list_agents" => McpAction::ListAgents,
        "log_conversation" => {
            let agent_id = arguments
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let message = arguments
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let role = arguments
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("ai_response")
                .to_string();
            let context = arguments
                .get("context")
                .and_then(|v| v.as_str())
                .map(String::from);
            let line_start = arguments
                .get("line_start")
                .and_then(|v| v.as_u64().map(|n| n as usize));
            let line_end = arguments
                .get("line_end")
                .and_then(|v| v.as_u64().map(|n| n as usize));
            McpAction::LogConversation {
                agent_id,
                message,
                role,
                context,
                line_start,
                line_end,
            }
        }
        _ => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Unknown tool: {tool_name}"),
                }),
            };
        }
    };

    // Send request to App and wait for response.
    let (response_tx, response_rx) = mpsc::channel();
    let app_request = McpAppRequest {
        action,
        response_tx,
    };

    if request_tx.send(app_request).is_err() {
        return JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "Editor not responding".to_string(),
            }),
        };
    }

    // Wait for the App's response (with timeout).
    match response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(response) => {
            if response.success {
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&response.data).unwrap_or_default()
                        }]
                    })),
                    error: None,
                }
            } else {
                let error_msg = response
                    .data
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32603,
                        message: error_msg,
                    }),
                }
            }
        }
        Err(_) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "Request timed out".to_string(),
            }),
        },
    }
}

/// Handle `resources/list`.
fn handle_resources_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    let resources = vec![
        ResourceDefinition {
            uri: "aura://buffer/current".to_string(),
            name: "Current Buffer".to_string(),
            description: "The full content of the currently open buffer.".to_string(),
            mime_type: "text/plain".to_string(),
        },
        ResourceDefinition {
            uri: "aura://buffer/info".to_string(),
            name: "Buffer Info".to_string(),
            description: "Metadata about the current buffer (path, language, line count)."
                .to_string(),
            mime_type: "application/json".to_string(),
        },
        ResourceDefinition {
            uri: "aura://diagnostics".to_string(),
            name: "Diagnostics".to_string(),
            description: "Current LSP diagnostics for the open file.".to_string(),
            mime_type: "application/json".to_string(),
        },
    ];

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone().unwrap_or(serde_json::Value::Null),
        result: Some(serde_json::json!({ "resources": resources })),
        error: None,
    }
}

/// Handle `resources/read`.
fn handle_resources_read(
    request: &JsonRpcRequest,
    request_tx: &mpsc::Sender<McpAppRequest>,
) -> JsonRpcResponse {
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    let params = request.params.as_ref();
    let uri = params
        .and_then(|p| p.get("uri"))
        .and_then(|u| u.as_str())
        .unwrap_or("");

    let action = match uri {
        "aura://buffer/current" => McpAction::ReadBuffer {
            start_line: None,
            end_line: None,
        },
        "aura://buffer/info" => McpAction::GetBufferInfo,
        "aura://diagnostics" => McpAction::GetDiagnostics,
        _ => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Unknown resource: {uri}"),
                }),
            };
        }
    };

    let (response_tx, response_rx) = mpsc::channel();
    let app_request = McpAppRequest {
        action,
        response_tx,
    };

    if request_tx.send(app_request).is_err() {
        return JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "Editor not responding".to_string(),
            }),
        };
    }

    match response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(response) => {
            let text = serde_json::to_string_pretty(&response.data).unwrap_or_default();
            let mime = if uri.ends_with("current") {
                "text/plain"
            } else {
                "application/json"
            };
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(serde_json::json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": mime,
                        "text": text
                    }]
                })),
                error: None,
            }
        }
        Err(_) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: "Request timed out".to_string(),
            }),
        },
    }
}

/// Read a Content-Length framed message from a reader.
fn read_message(reader: &mut BufReader<TcpStream>) -> std::io::Result<String> {
    let mut content_length: usize = 0;

    // Read headers.
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim();

        if header.is_empty() {
            break;
        }

        if let Some(len_str) = header.strip_prefix("Content-Length: ") {
            content_length = len_str
                .parse()
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad length"))?;
        }
    }

    if content_length == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "no content length",
        ));
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid utf8"))
}

/// Write a Content-Length framed message.
fn write_message(writer: &mut TcpStream, msg: &str) -> std::io::Result<()> {
    let header = format!("Content-Length: {}\r\n\r\n", msg.len());
    writer.write_all(header.as_bytes())?;
    writer.write_all(msg.as_bytes())?;
    writer.flush()
}

/// ISO 8601 timestamp without chrono.
fn now_iso() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Approximate date calculation.
    let mut year = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            366
        } else {
            365
        };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let is_leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if is_leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &md in &month_days {
        if remaining_days < md {
            break;
        }
        remaining_days -= md;
        month += 1;
    }
    let day = remaining_days + 1;

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_registry() {
        let mut reg = AgentRegistry::default();
        assert!(reg.register("claude-code"));
        assert!(!reg.register("claude-code")); // duplicate
        assert!(reg.register("copilot"));
        assert_eq!(reg.count(), 2);

        reg.record_edit("claude-code");
        assert_eq!(reg.agents["claude-code"].edit_count, 1);

        assert!(reg.unregister("copilot"));
        assert_eq!(reg.count(), 1);
        assert!(!reg.unregister("copilot")); // already gone
    }

    #[test]
    fn test_tool_definitions_are_valid_json() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };
        let response = handle_tools_list(&request);
        assert!(response.error.is_none());

        let tools = response.result.unwrap();
        let tool_list = tools.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tool_list.len(), 12);
    }

    #[test]
    fn test_resource_definitions() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "resources/list".to_string(),
            params: None,
        };
        let response = handle_resources_list(&request);
        assert!(response.error.is_none());

        let resources = response.result.unwrap();
        let list = resources.get("resources").unwrap().as_array().unwrap();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn test_initialize_response() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: None,
        };
        let response = handle_initialize(&request);
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "aura-editor");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[test]
    fn test_unknown_tool_returns_error() {
        let (tx, _rx) = mpsc::channel();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "nonexistent_tool",
                "arguments": {}
            })),
        };
        let response = handle_tools_call(&request, &tx);
        assert!(response.error.is_some());
        assert!(response.error.unwrap().message.contains("Unknown tool"));
    }
}
