//! MCP client for connecting to external MCP servers.
//!
//! AURA can connect to external MCP servers (filesystem, git, custom tools)
//! and invoke their tools. Communication is over stdio (spawn child process)
//! using JSON-RPC 2.0 with Content-Length framing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;

/// Configuration for an external MCP server connection.
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    /// Display name for the server.
    pub name: String,
    /// Command to launch the server.
    pub command: String,
    /// Arguments to pass to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to set.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// A tool definition from an external MCP server.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExternalTool {
    /// Unique name of the tool.
    pub name: String,
    /// Human-readable description of the tool.
    #[serde(default)]
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    #[serde(default, rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// Events received from an external MCP server.
#[derive(Debug)]
pub enum McpClientEvent {
    /// Server initialized successfully.
    Initialized {
        /// Name of the server that was initialized.
        server_name: String,
        /// List of tools provided by the server.
        tools: Vec<ExternalTool>,
    },
    /// Result from a tool call.
    ToolResult {
        /// ID of the original request that produced this result.
        request_id: u64,
        /// Tool execution result, or an error message.
        result: Result<serde_json::Value, String>,
    },
    /// Server error.
    Error(String),
}

/// An active connection to an external MCP server.
pub struct McpClientConnection {
    /// Name of the server.
    pub server_name: String,
    /// Available tools from this server.
    pub tools: Vec<ExternalTool>,
    /// Channel to send messages to the writer thread.
    writer_tx: mpsc::Sender<String>,
    /// Channel to receive events from the reader thread.
    event_rx: mpsc::Receiver<McpClientEvent>,
    /// Next request ID.
    next_id: u64,
    /// Child process handle.
    _child: Child,
}

impl McpClientConnection {
    /// Connect to an external MCP server by spawning its process.
    pub fn connect(config: &McpServerConfig) -> anyhow::Result<Self> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| {
            anyhow::anyhow!("Failed to capture stdin of MCP server: {}", config.name)
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            anyhow::anyhow!("Failed to capture stdout of MCP server: {}", config.name)
        })?;

        let (writer_tx, writer_rx) = mpsc::channel::<String>();
        let (event_tx, event_rx) = mpsc::channel();

        // Writer thread.
        thread::Builder::new()
            .name(format!("mcp-client-writer-{}", config.name))
            .spawn(move || {
                writer_thread(stdin, writer_rx);
            })?;

        // Reader thread.
        let server_name = config.name.clone();
        thread::Builder::new()
            .name(format!("mcp-client-reader-{}", config.name))
            .spawn(move || {
                reader_thread(stdout, event_tx, server_name);
            })?;

        // Send initialize request.
        let init_msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "aura-editor",
                    "version": "0.1.0"
                }
            }
        });
        writer_tx.send(serde_json::to_string(&init_msg)?)?;

        Ok(Self {
            server_name: config.name.clone(),
            tools: Vec::new(),
            writer_tx,
            event_rx,
            next_id: 1,
            _child: child,
        })
    }

    /// Poll for events from the server (non-blocking).
    pub fn poll_events(&mut self) -> Vec<McpClientEvent> {
        let mut events = Vec::new();
        loop {
            match self.event_rx.try_recv() {
                Ok(event) => {
                    // Capture tool list from initialization.
                    if let McpClientEvent::Initialized { tools, .. } = &event {
                        self.tools = tools.clone();
                    }
                    events.push(event);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    events.push(McpClientEvent::Error(format!(
                        "MCP server {} disconnected",
                        self.server_name
                    )));
                    break;
                }
            }
        }
        events
    }

    /// Call a tool on the external server.
    pub fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> anyhow::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        self.writer_tx.send(serde_json::to_string(&msg)?)?;
        Ok(id)
    }

    /// Send the initialized notification after receiving the init response.
    pub fn send_initialized(&self) {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let _ = self.writer_tx.send(serde_json::to_string(&msg).unwrap());
    }

    /// List available tools (cached from initialization).
    pub fn available_tools(&self) -> &[ExternalTool] {
        &self.tools
    }

    /// Shut down the connection.
    pub fn shutdown(&self) {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 999999,
            "method": "shutdown"
        });
        let _ = self.writer_tx.send(serde_json::to_string(&msg).unwrap());
    }
}

/// Writer thread: sends Content-Length framed messages to the server's stdin.
fn writer_thread(mut stdin: std::process::ChildStdin, rx: mpsc::Receiver<String>) {
    while let Ok(msg) = rx.recv() {
        let header = format!("Content-Length: {}\r\n\r\n", msg.len());
        if stdin.write_all(header.as_bytes()).is_err() {
            break;
        }
        if stdin.write_all(msg.as_bytes()).is_err() {
            break;
        }
        if stdin.flush().is_err() {
            break;
        }
    }
}

/// Reader thread: reads Content-Length framed messages from the server's stdout.
fn reader_thread(
    stdout: std::process::ChildStdout,
    event_tx: mpsc::Sender<McpClientEvent>,
    server_name: String,
) {
    let mut reader = BufReader::new(stdout);
    let mut pending_init = true;

    while let Ok(msg) = read_message(&mut reader) {
        let value: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                let _ = event_tx.send(McpClientEvent::Error(format!("Parse error: {e}")));
                continue;
            }
        };

        // Check if this is a response (has "id") or notification.
        if let Some(id) = value.get("id") {
            if let Some(error) = value.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error");
                let _ = event_tx.send(McpClientEvent::Error(msg.to_string()));
                continue;
            }

            let result = value
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            if pending_init && id.as_u64() == Some(0) {
                // Initialize response — extract tools.
                pending_init = false;

                // Send tools/list request to get available tools.
                // The init response has capabilities, but we need to request tools separately.
                // For now, extract server info and send Initialized event with empty tools.
                // The tools will be populated by a subsequent tools/list call.
                let _ = event_tx.send(McpClientEvent::Initialized {
                    server_name: server_name.clone(),
                    tools: Vec::new(),
                });
            } else {
                // Tool call result.
                let request_id = id.as_u64().unwrap_or(0);

                // Check if this is a tools/list response.
                if let Some(tools_array) = result.get("tools").and_then(|t| t.as_array()) {
                    let tools: Vec<ExternalTool> = tools_array
                        .iter()
                        .filter_map(|t| serde_json::from_value(t.clone()).ok())
                        .collect();
                    let _ = event_tx.send(McpClientEvent::Initialized {
                        server_name: server_name.clone(),
                        tools,
                    });
                } else {
                    let _ = event_tx.send(McpClientEvent::ToolResult {
                        request_id,
                        result: Ok(result),
                    });
                }
            }
        }
        // Notifications (no id) are ignored for now.
    }
}

/// Read a Content-Length framed message.
fn read_message<R: BufRead>(reader: &mut R) -> std::io::Result<String> {
    let mut content_length: usize = 0;

    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "EOF",
            ));
        }
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

/// Load MCP server configurations from an aura.toml file.
pub fn load_config(path: &std::path::Path) -> Vec<McpServerConfig> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Parse TOML looking for [mcp_servers.name] sections.
    let table: toml::Table = match content.parse() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    let servers = match table.get("mcp_servers").and_then(|s| s.as_table()) {
        Some(s) => s,
        None => return Vec::new(),
    };

    servers
        .iter()
        .filter_map(|(name, value)| {
            let command = value.get("command")?.as_str()?.to_string();
            let args = value
                .get("args")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let env = value
                .get("env")
                .and_then(|e| e.as_table())
                .map(|t| {
                    t.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default();

            Some(McpServerConfig {
                name: name.clone(),
                command,
                args,
                env,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_empty_config() {
        let path = std::path::Path::new("/nonexistent/aura.toml");
        let configs = load_config(path);
        assert!(configs.is_empty());
    }

    #[test]
    fn test_parse_config() {
        let toml_str = r#"
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

[mcp_servers.git]
command = "mcp-server-git"
args = ["--repo", "."]
env = { GIT_DIR = ".git" }
"#;
        let table: toml::Table = toml_str.parse().unwrap();
        let servers = table.get("mcp_servers").unwrap().as_table().unwrap();
        assert_eq!(servers.len(), 2);
    }

    #[test]
    fn test_read_message() {
        let input = "Content-Length: 13\r\n\r\n{\"test\":true}";
        let mut reader = BufReader::new(input.as_bytes());
        let msg = read_message(&mut reader).unwrap();
        assert_eq!(msg, "{\"test\":true}");
    }

    #[test]
    fn test_external_tool_deserialize() {
        let json = r#"{
            "name": "read_file",
            "description": "Read a file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }
        }"#;
        let tool: ExternalTool = serde_json::from_str(json).unwrap();
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.description, "Read a file");
    }
}
