//! Anthropic API streaming client for Claude.
//!
//! Implements the Messages API with streaming support, retry logic
//! with exponential backoff, and rate-limiting awareness.

use crate::AiConfig;
use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::sync::mpsc;
use tracing::{debug, warn};

/// Events emitted by the streaming API client.
#[derive(Debug, Clone)]
pub enum AiEvent {
    /// A chunk of text from the AI response.
    Token(String),
    /// The AI finished responding. Contains the full accumulated text.
    Done(String),
    /// An error occurred.
    Error(String),
    /// The AI wants to use a tool.
    ToolUse {
        /// Tool use ID (for sending result back).
        id: String,
        /// Tool name.
        name: String,
        /// Tool input parameters.
        input: serde_json::Value,
    },
    /// The streaming response is complete and contained tool use(s).
    /// Carries the full assistant content blocks for context assembly.
    ToolUseComplete {
        /// Text content accumulated before/between tool calls.
        text: String,
        /// All content blocks (text + tool_use) for the assistant turn.
        content_blocks: Vec<ContentBlock>,
    },
    /// Activity or status message from the backend (e.g. "Reading file...", "Searching...").
    /// Displayed as system info in the chat panel.
    Activity(String),
}

/// A content block within a message (Anthropic API format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    /// A text content block.
    #[serde(rename = "text")]
    Text {
        /// The text content.
        text: String,
    },
    /// A tool use content block.
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Unique ID for this tool use.
        id: String,
        /// Tool name.
        name: String,
        /// Tool input parameters.
        input: serde_json::Value,
    },
    /// A tool result content block (sent by user role).
    #[serde(rename = "tool_result")]
    ToolResult {
        /// The tool_use ID this result is for.
        tool_use_id: String,
        /// The result content.
        content: String,
        /// Whether this result represents an error.
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Message content: either a simple string or structured content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Simple text content (convenience for plain messages).
    Text(String),
    /// Structured content blocks (for tool use conversations).
    Blocks(Vec<ContentBlock>),
}

/// A message in the conversation.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    /// The role: "user" or "assistant".
    pub role: String,
    /// The message content.
    pub content: MessageContent,
}

impl Message {
    /// Create a simple text message.
    pub fn text(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: MessageContent::Text(content.to_string()),
        }
    }

    /// Create a message with structured content blocks.
    pub fn blocks(role: &str, blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: role.to_string(),
            content: MessageContent::Blocks(blocks),
        }
    }

    /// Get the text content of this message (for display/persistence).
    pub fn text_content(&self) -> String {
        match &self.content {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}

/// An Anthropic API tool definition.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,
    /// Description of what the tool does.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: serde_json::Value,
}

/// Permission level for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermission {
    /// Auto-approved, no user prompt needed (e.g., read_file, search).
    AutoApprove,
    /// Requires explicit Y/N from user (e.g., edit_file, run_command).
    RequiresApproval,
}

/// Get the permission level for a tool by name.
pub fn tool_permission(name: &str) -> ToolPermission {
    match name {
        "read_file" | "list_files" | "search_files" => ToolPermission::AutoApprove,
        // Subagent tools are auto-approved (agent mode already does its own gating).
        "spawn_subagent" | "check_subagent" | "cancel_subagent" => ToolPermission::AutoApprove,
        _ => ToolPermission::RequiresApproval,
    }
}

/// Return the editor tool definitions for the Anthropic API.
pub fn editor_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a file. Returns the file content as text.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to read (relative to project root)"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "list_files".to_string(),
            description: "List files and directories at the given path. Returns a list of entries.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to list (relative to project root, default '.')"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to list recursively (default false)"
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "search_files".to_string(),
            description: "Search for a pattern in files. Returns matching lines with file paths and line numbers.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The text or regex pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default '.')"
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "Glob pattern to filter files (e.g. '*.rs')"
                    }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "edit_file".to_string(),
            description: "Edit a file by replacing old_text with new_text. Creates the file if it doesn't exist and old_text is empty.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to edit"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "The exact text to find and replace (empty string to create a new file)"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "The replacement text"
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        },
        ToolDefinition {
            name: "run_command".to_string(),
            description: "Run a shell command and return its output. Use for builds, tests, git operations, etc.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds (default 30)"
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "create_directory".to_string(),
            description: "Create a directory (and parent directories if needed).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to create (relative to project root)"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "rename_file".to_string(),
            description: "Rename or move a file or directory.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "old_path": {
                        "type": "string",
                        "description": "Current path of the file or directory"
                    },
                    "new_path": {
                        "type": "string",
                        "description": "New path for the file or directory"
                    }
                },
                "required": ["old_path", "new_path"]
            }),
        },
    ]
}

/// Return agent-mode-only tool definitions (subagent orchestration).
///
/// These are appended to the standard tools when the editor is in agent mode.
pub fn agent_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "spawn_subagent".to_string(),
            description: "Spawn a focused sub-agent to work on a specific subtask in parallel. \
                          The subagent runs independently with its own conversation context. \
                          Use check_subagent to retrieve results when done.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "What the subagent should do"
                    },
                    "role": {
                        "type": "string",
                        "enum": ["explorer", "test_runner", "refactorer", "reviewer", "custom"],
                        "description": "Subagent specialization: explorer (read-only analysis), test_runner (run tests), refactorer (code changes), reviewer (code review)"
                    },
                    "tools": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Tool names this subagent is allowed to use (empty = role default)"
                    }
                },
                "required": ["task", "role"]
            }),
        },
        ToolDefinition {
            name: "check_subagent".to_string(),
            description: "Check the status and result of a previously spawned subagent. \
                          Returns the subagent's current status and, if completed, its result summary.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "The subagent ID returned by spawn_subagent"
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "cancel_subagent".to_string(),
            description: "Cancel a running subagent.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "The subagent ID to cancel"
                    }
                },
                "required": ["id"]
            }),
        },
    ]
}

/// Request body for the Anthropic Messages API.
#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ToolDefinition>,
}

/// Streaming event types from the API.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct StreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<Delta>,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    content_block: Option<ContentBlockStart>,
}

/// Content block start data from the SSE stream.
#[derive(Debug, Deserialize)]
struct ContentBlockStart {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// Message delta from the SSE stream (message_delta event).
#[derive(Debug, Deserialize)]
struct MessageDelta {
    #[serde(default)]
    stop_reason: Option<String>,
}

/// Full stream event for message_delta.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MessageDeltaEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<MessageDelta>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Delta {
    #[serde(default)]
    text: Option<String>,
    #[serde(default, rename = "type")]
    delta_type: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
}

/// Client for the Anthropic Messages API with streaming.
pub struct AnthropicClient {
    config: AiConfig,
    http: reqwest::Client,
}

impl AnthropicClient {
    /// Return the configured context window token limit.
    pub fn max_context_tokens(&self) -> usize {
        self.config.max_context_tokens
    }

    /// Create a new client from config.
    pub fn new(config: AiConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&config.api_key).context("Invalid API key format")?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self { config, http })
    }

    /// Send a streaming completion request. Returns a receiver for streaming events.
    ///
    /// The request is executed on a background thread with its own tokio runtime.
    /// This allows the synchronous TUI event loop to poll for results.
    pub fn stream_completion(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
    ) -> mpsc::Receiver<AiEvent> {
        self.stream_completion_with_tools(system_prompt, messages, Vec::new())
    }

    /// Send a streaming completion request with tool definitions.
    ///
    /// When tools are provided, the AI may respond with tool_use content blocks
    /// which are emitted as `AiEvent::ToolUse` events.
    pub fn stream_completion_with_tools(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> mpsc::Receiver<AiEvent> {
        let (tx, rx) = mpsc::channel();

        let request = MessagesRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system: system_prompt.to_string(),
            messages,
            stream: true,
            tools,
        };

        let http = self.http.clone();
        let base_url = self.config.base_url.clone();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(AiEvent::Error(format!("Failed to create runtime: {e}")));
                    return;
                }
            };

            rt.block_on(async {
                let result = Self::do_stream_request(&http, &base_url, &request, &tx).await;
                if let Err(e) = result {
                    let _ = tx.send(AiEvent::Error(e.to_string()));
                }
            });
        });

        rx
    }

    /// Internal: perform the streaming HTTP request with retry logic.
    async fn do_stream_request(
        http: &reqwest::Client,
        base_url: &str,
        request: &MessagesRequest,
        tx: &mpsc::Sender<AiEvent>,
    ) -> Result<()> {
        let url = format!("{base_url}/v1/messages");
        let mut retries = 0u32;
        let max_retries = 3u32;

        loop {
            debug!("Sending API request (attempt {})", retries + 1);

            let response = http
                .post(&url)
                .json(request)
                .send()
                .await
                .context("Failed to send request")?;

            let status = response.status();

            // Rate limiting: back off and retry.
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                retries += 1;
                if retries > max_retries {
                    anyhow::bail!("API request failed after {max_retries} retries: {status}");
                }
                let delay = std::time::Duration::from_millis(1000 * 2u64.pow(retries - 1));
                warn!("Rate limited or server error ({status}), retrying in {delay:?}");
                tokio::time::sleep(delay).await;
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("API error {status}: {body}");
            }

            // Parse the SSE stream.
            let body = response.text().await?;
            let mut accumulated_text = String::new();
            let mut content_blocks: Vec<ContentBlock> = Vec::new();
            let mut has_tool_use = false;

            // Track current tool_use block being built.
            let mut current_tool_id: Option<String> = None;
            let mut current_tool_name: Option<String> = None;
            let mut current_tool_json = String::new();
            let mut stop_reason: Option<String> = None;

            for line in body.lines() {
                let line = line.trim();
                if !line.starts_with("data: ") {
                    continue;
                }
                let data = &line["data: ".len()..];
                if data == "[DONE]" {
                    break;
                }

                if let Ok(event) = serde_json::from_str::<StreamEvent>(data) {
                    match event.event_type.as_str() {
                        "content_block_start" => {
                            if let Some(cb) = &event.content_block {
                                if cb.block_type == "tool_use" {
                                    has_tool_use = true;
                                    current_tool_id = cb.id.clone();
                                    current_tool_name = cb.name.clone();
                                    current_tool_json.clear();
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Some(delta) = &event.delta {
                                // Text delta.
                                if let Some(text) = &delta.text {
                                    accumulated_text.push_str(text);
                                    let _ = tx.send(AiEvent::Token(text.clone()));
                                }
                                // Tool input JSON delta.
                                if let Some(partial) = &delta.partial_json {
                                    current_tool_json.push_str(partial);
                                }
                            }
                        }
                        "content_block_stop" => {
                            // Finalize current tool_use block if any.
                            if let (Some(id), Some(name)) =
                                (current_tool_id.take(), current_tool_name.take())
                            {
                                let input: serde_json::Value = serde_json::from_str(
                                    &current_tool_json,
                                )
                                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                                current_tool_json.clear();

                                // Add text block first if we have accumulated text.
                                if !accumulated_text.is_empty() {
                                    content_blocks.push(ContentBlock::Text {
                                        text: accumulated_text.clone(),
                                    });
                                }
                                content_blocks.push(ContentBlock::ToolUse {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                });
                                let _ = tx.send(AiEvent::ToolUse { id, name, input });
                            }
                        }
                        "message_delta" => {
                            // Check for stop_reason.
                            if let Ok(md) = serde_json::from_str::<MessageDeltaEvent>(data) {
                                if let Some(delta) = &md.delta {
                                    stop_reason = delta.stop_reason.clone();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            if has_tool_use {
                // Ensure any trailing text is captured.
                if !accumulated_text.is_empty()
                    && !content_blocks.iter().any(
                        |b| matches!(b, ContentBlock::Text { text } if text == &accumulated_text),
                    )
                {
                    content_blocks.insert(
                        0,
                        ContentBlock::Text {
                            text: accumulated_text.clone(),
                        },
                    );
                }
                let _ = tx.send(AiEvent::ToolUseComplete {
                    text: accumulated_text,
                    content_blocks,
                });
            } else {
                let _ = tx.send(AiEvent::Done(accumulated_text));
            }

            let _ = stop_reason; // consumed above
            return Ok(());
        }
    }
}
