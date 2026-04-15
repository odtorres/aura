//! Claude Code CLI backend for AI completions.
//!
//! Uses the `claude` CLI tool (Claude Code) as an alternative to
//! direct Anthropic API access. This allows users who have Claude Code
//! authenticated to use AI features without a separate API key.
//!
//! The CLI is invoked in *print mode* (`claude -p`) which runs
//! non-interactively and streams text to stdout.  We use
//! `--output-format stream-json` so each chunk arrives as a JSON line
//! that we can parse for incremental token display and activity
//! notifications.
//!
//! **Important**: Claude Code executes its own built-in tools (Read, Grep,
//! Edit, Bash, etc.) internally. These tool events are displayed as
//! informational activity in the chat panel. For custom editor tools,
//! the model is instructed to output `TOOL_CALL:` directives which go
//! through the editor's approval flow.

use crate::{AiEvent, ContentBlock, Message, MessageContent, ToolDefinition};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use tracing::{debug, warn};

/// Client that delegates AI requests to the Claude Code CLI.
pub struct ClaudeCodeClient {
    /// Maximum context tokens (used for context assembly budgeting).
    max_context_tokens: usize,
}

impl ClaudeCodeClient {
    /// Create a new Claude Code client.
    ///
    /// Returns `None` if the `claude` CLI is not found on `PATH`.
    pub fn new() -> Option<Self> {
        // Verify that `claude` is available.
        let status = Command::new("claude")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match status {
            Ok(s) if s.success() => {
                debug!("Claude Code CLI found");
                Some(Self {
                    max_context_tokens: 200_000,
                })
            }
            Ok(s) => {
                warn!("Claude Code CLI exited with status: {s}");
                None
            }
            Err(e) => {
                debug!("Claude Code CLI not found: {e}");
                None
            }
        }
    }

    /// Return the configured context window token limit.
    pub fn max_context_tokens(&self) -> usize {
        self.max_context_tokens
    }

    /// Send a completion request via `claude -p`. Returns a receiver for streaming events.
    ///
    /// The CLI is executed on a background thread. Output is streamed line-by-line
    /// back to the caller via the channel.
    pub fn stream_completion(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
    ) -> mpsc::Receiver<AiEvent> {
        let (tx, rx) = mpsc::channel();

        // Build the full prompt: system context + user messages.
        let mut full_prompt = String::new();
        full_prompt.push_str(system_prompt);
        full_prompt.push_str("\n\n");
        for msg in &messages {
            if msg.role == "user" {
                full_prompt.push_str(&msg.text_content());
                full_prompt.push('\n');
            }
        }

        std::thread::spawn(move || {
            let result = Self::run_claude_cli(&full_prompt, &tx, false);
            if let Err(e) = result {
                let _ = tx.send(AiEvent::Error(e.to_string()));
            }
        });

        rx
    }

    /// Send a completion request with tool definitions via `claude -p`.
    ///
    /// Tool definitions are encoded into the prompt text. The response is
    /// parsed for `TOOL_CALL:` text directives which go through the editor's
    /// approval flow. Claude Code's own native tool use is shown as activity.
    pub fn stream_completion_with_tools(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> mpsc::Receiver<AiEvent> {
        let (tx, rx) = mpsc::channel();

        let full_prompt = Self::build_tool_prompt(system_prompt, &messages, &tools);

        std::thread::spawn(move || {
            let result = Self::run_claude_cli(&full_prompt, &tx, true);
            if let Err(e) = result {
                let _ = tx.send(AiEvent::Error(e.to_string()));
            }
        });

        rx
    }

    /// Build a prompt that includes tool definitions and full conversation history.
    fn build_tool_prompt(
        system_prompt: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> String {
        let mut prompt = String::new();
        prompt.push_str(system_prompt);
        prompt.push_str("\n\n");

        // Encode tool definitions as instructions.
        if !tools.is_empty() {
            prompt.push_str("# Available Tools\n\n");
            prompt.push_str(
                "You can call tools by outputting a line in this exact format:\n\
                 TOOL_CALL: {\"name\": \"tool_name\", \"input\": {parameters}}\n\n\
                 IMPORTANT RULES:\n\
                 - Each TOOL_CALL must be on its own line, starting with 'TOOL_CALL: '\n\
                 - The JSON must be valid and on a single line\n\
                 - You may call multiple tools in one response\n\
                 - After outputting tool calls, stop and wait for results\n\
                 - Do NOT wrap tool calls in code blocks\n\
                 - Do NOT ask for permission — just call the tool directly\n\n",
            );

            for tool in tools {
                prompt.push_str(&format!("## {}\n", tool.name));
                prompt.push_str(&format!("{}\n", tool.description));
                prompt.push_str(&format!("Parameters: {}\n\n", tool.input_schema));
            }
        }

        // Add conversation messages with proper formatting.
        for msg in messages {
            match &msg.content {
                MessageContent::Text(text) => {
                    if msg.role == "user" {
                        prompt.push_str(&format!("User: {text}\n\n"));
                    } else {
                        prompt.push_str(&format!("Assistant: {text}\n\n"));
                    }
                }
                MessageContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text } => {
                                if msg.role == "user" {
                                    prompt.push_str(&format!("User: {text}\n\n"));
                                } else {
                                    prompt.push_str(&format!("Assistant: {text}\n\n"));
                                }
                            }
                            ContentBlock::ToolUse { id, name, input } => {
                                prompt.push_str(&format!(
                                    "[Tool called: {name} (id: {id})]\nInput: {input}\n\n"
                                ));
                            }
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                            } => {
                                let label = if is_error.unwrap_or(false) {
                                    "Tool error"
                                } else {
                                    "Tool result"
                                };
                                prompt.push_str(&format!(
                                    "[{label} for {tool_use_id}]:\n{content}\n\n"
                                ));
                            }
                        }
                    }
                }
            }
        }

        prompt
    }

    /// Internal: spawn `claude -p` and stream its stdout.
    ///
    /// Uses `--output-format stream-json` for structured streaming.
    /// Parses Anthropic API events for text deltas and displays Claude Code's
    /// native tool use as informational activity. When `with_tools` is true,
    /// also checks the final text for `TOOL_CALL:` directives.
    fn run_claude_cli(
        prompt: &str,
        tx: &mpsc::Sender<AiEvent>,
        with_tools: bool,
    ) -> anyhow::Result<()> {
        debug!(
            "Spawning claude -p (prompt length: {} chars, tools: {})",
            prompt.len(),
            with_tools
        );

        let mut child = Command::new("claude")
            .arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn claude CLI: {e}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture claude stdout"))?;

        let reader = BufReader::new(stdout);
        let mut accumulated_text = String::new();
        let mut result_text: Option<String> = None;

        // Track native Claude Code tool use (for activity display only).
        let mut current_native_tool_name: Option<String> = None;
        let mut current_native_tool_json = String::new();

        for line in reader.lines() {
            match line {
                Ok(raw) => {
                    if raw.trim().is_empty() {
                        continue;
                    }

                    let v: serde_json::Value = match serde_json::from_str(&raw) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Determine the API event payload: either nested under an
                    // `"event"` wrapper (older stream-json format) or at the
                    // top level (current format).
                    let event_type_raw = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let (api_event, event_type) = if let Some(inner) = v.get("event") {
                        let et = inner.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        (inner, et)
                    } else {
                        (&v, event_type_raw)
                    };

                    // ── Handle Anthropic streaming API events ──
                    match event_type {
                        "content_block_start" => {
                            if let Some(cb) = api_event.get("content_block") {
                                let block_type =
                                    cb.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                if block_type == "tool_use" {
                                    // Claude Code's own tool — display as activity only.
                                    current_native_tool_name =
                                        cb.get("name").and_then(|v| v.as_str()).map(String::from);
                                    current_native_tool_json.clear();

                                    if let Some(name) = &current_native_tool_name {
                                        let _ = tx.send(AiEvent::Activity(format!(
                                            "Using tool: {name}..."
                                        )));
                                    }
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Some(delta) = api_event.get("delta") {
                                let delta_type =
                                    delta.get("type").and_then(|t| t.as_str()).unwrap_or("");

                                match delta_type {
                                    "text_delta" => {
                                        if let Some(text) =
                                            delta.get("text").and_then(|t| t.as_str())
                                        {
                                            accumulated_text.push_str(text);
                                            let _ = tx.send(AiEvent::Token(text.to_string()));
                                        }
                                    }
                                    "input_json_delta" => {
                                        // Accumulate native tool input for activity display.
                                        if let Some(partial) =
                                            delta.get("partial_json").and_then(|t| t.as_str())
                                        {
                                            current_native_tool_json.push_str(partial);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "content_block_stop" => {
                            // Finalize native tool use — show activity with input summary.
                            if let Some(name) = current_native_tool_name.take() {
                                let input_summary =
                                    Self::summarize_tool_input(&current_native_tool_json);
                                current_native_tool_json.clear();
                                if !input_summary.is_empty() {
                                    let _ = tx.send(AiEvent::Activity(format!(
                                        "Tool {name}: {input_summary}"
                                    )));
                                }
                            }
                        }

                        // ── Top-level Claude Code events ──
                        "result" => {
                            // The `result` field may be a string or an object with
                            // a nested text field — handle both.
                            if let Some(text) = v.get("result").and_then(|r| r.as_str()) {
                                result_text = Some(text.to_string());
                            } else if let Some(text) = v
                                .get("result")
                                .and_then(|r| r.get("text"))
                                .and_then(|t| t.as_str())
                            {
                                result_text = Some(text.to_string());
                            }
                        }
                        "system" => {
                            let message = v
                                .get("message")
                                .and_then(|m| m.as_str())
                                .or_else(|| v.get("subtype").and_then(|s| s.as_str()))
                                .unwrap_or("system event");
                            let _ = tx.send(AiEvent::Activity(message.to_string()));
                        }
                        "assistant" => {
                            // Assistant message — extract tool use as activity info.
                            if let Some(message) = v.get("message") {
                                Self::parse_assistant_activity(message, tx);
                            }
                        }
                        "user" => {
                            // Tool result from Claude Code's own tool execution.
                            if let Some(message) = v.get("message") {
                                Self::parse_tool_result_activity(message, tx);
                            }
                        }
                        _ => {
                            debug!("Unhandled stream-json event type: {event_type}");
                        }
                    }
                }
                Err(e) => {
                    warn!("Error reading claude stdout: {e}");
                    break;
                }
            }
        }

        let status = child.wait()?;
        if !status.success() {
            let stderr_msg = child
                .stderr
                .take()
                .and_then(|stderr| {
                    let reader = BufReader::new(stderr);
                    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
                    if lines.is_empty() {
                        None
                    } else {
                        Some(lines.join("\n"))
                    }
                })
                .unwrap_or_else(|| format!("claude exited with status {status}"));
            anyhow::bail!("Claude Code error: {stderr_msg}");
        }

        // Use the result event text as fallback.
        if accumulated_text.is_empty() {
            if let Some(ref text) = result_text {
                accumulated_text = text.clone();
                let _ = tx.send(AiEvent::Token(text.clone()));
            }
        }

        if with_tools {
            // Check for TOOL_CALL: text directives in the response.
            let (display_text, tool_calls) = Self::extract_tool_calls(&accumulated_text);

            if tool_calls.is_empty() {
                let _ = tx.send(AiEvent::Done(accumulated_text));
            } else {
                let mut blocks = Vec::new();
                if !display_text.is_empty() {
                    blocks.push(ContentBlock::Text {
                        text: display_text.clone(),
                    });
                }

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();

                for (i, (name, input)) in tool_calls.iter().enumerate() {
                    let id = format!("cli_toolu_{timestamp}_{i}");
                    blocks.push(ContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    });
                    let _ = tx.send(AiEvent::ToolUse {
                        id,
                        name: name.clone(),
                        input: input.clone(),
                    });
                }

                let _ = tx.send(AiEvent::ToolUseComplete {
                    text: display_text,
                    content_blocks: blocks,
                });
            }
        } else {
            let _ = tx.send(AiEvent::Done(accumulated_text));
        }

        Ok(())
    }

    /// Parse an assistant message event and emit activity notifications.
    ///
    /// Claude Code's `{"type":"assistant","message":{...}}` events show
    /// what the AI is doing. We display tool use as informational activity.
    fn parse_assistant_activity(message: &serde_json::Value, tx: &mpsc::Sender<AiEvent>) {
        // Check for content array.
        if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
            for block in content {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if block_type == "tool_use" {
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let input = block.get("input").cloned().unwrap_or_default();
                    let summary = Self::summarize_tool_input_value(&input);
                    let _ = tx.send(AiEvent::Activity(format!("Tool: {name} {summary}")));
                }
            }
        }

        // Check for single tool_use message format.
        if let Some(msg_type) = message.get("type").and_then(|t| t.as_str()) {
            if msg_type == "tool_use" {
                let name = message
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let input = message.get("input").cloned().unwrap_or_default();
                let summary = Self::summarize_tool_input_value(&input);
                let _ = tx.send(AiEvent::Activity(format!("Tool: {name} {summary}")));
            }
        }
    }

    /// Parse a tool result event and emit an activity notification.
    fn parse_tool_result_activity(message: &serde_json::Value, tx: &mpsc::Sender<AiEvent>) {
        let content = message
            .get("content")
            .and_then(|c| {
                if let Some(s) = c.as_str() {
                    Some(s.to_string())
                } else if let Some(arr) = c.as_array() {
                    let texts: Vec<String> = arr
                        .iter()
                        .filter_map(|b| {
                            b.get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect();
                    if texts.is_empty() {
                        None
                    } else {
                        Some(texts.join("\n"))
                    }
                } else {
                    None
                }
            })
            .unwrap_or_default();
        if !content.is_empty() {
            let summary = if content.len() > 150 {
                format!("{}...", &content[..150])
            } else {
                content
            };
            let _ = tx.send(AiEvent::Activity(format!("Result: {summary}")));
        }
    }

    /// Create a short summary of tool input JSON for display.
    fn summarize_tool_input(json_str: &str) -> String {
        if json_str.is_empty() {
            return String::new();
        }
        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(v) => Self::summarize_tool_input_value(&v),
            Err(_) => {
                // Show raw truncated.
                if json_str.len() > 100 {
                    format!("{}...", &json_str[..100])
                } else {
                    json_str.to_string()
                }
            }
        }
    }

    /// Summarize a tool input Value for compact display.
    fn summarize_tool_input_value(input: &serde_json::Value) -> String {
        if let Some(obj) = input.as_object() {
            // Show key=value pairs compactly.
            let parts: Vec<String> = obj
                .iter()
                .take(3)
                .map(|(k, v)| {
                    let val_str = match v {
                        serde_json::Value::String(s) => {
                            if s.len() > 60 {
                                format!("\"{}...\"", &s[..60])
                            } else {
                                format!("\"{s}\"")
                            }
                        }
                        other => {
                            let s = other.to_string();
                            if s.len() > 60 {
                                format!("{}...", &s[..60])
                            } else {
                                s
                            }
                        }
                    };
                    format!("{k}={val_str}")
                })
                .collect();
            let suffix = if obj.len() > 3 { ", ..." } else { "" };
            format!("({}{})", parts.join(", "), suffix)
        } else {
            let s = input.to_string();
            if s.len() > 100 {
                format!("{}...", &s[..100])
            } else {
                s
            }
        }
    }

    /// Extract tool calls from the model's response text.
    ///
    /// Looks for lines matching `TOOL_CALL: {...}` and extracts them.
    /// Returns the remaining display text (with tool call lines removed)
    /// and a list of `(name, input)` pairs.
    fn extract_tool_calls(text: &str) -> (String, Vec<(String, serde_json::Value)>) {
        let mut display_lines = Vec::new();
        let mut tool_calls = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(json_str) = trimmed.strip_prefix("TOOL_CALL:") {
                let json_str = json_str.trim();
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let (Some(name), Some(input)) =
                        (v.get("name").and_then(|n| n.as_str()), v.get("input"))
                    {
                        tool_calls.push((name.to_string(), input.clone()));
                        continue;
                    }
                }
            }
            display_lines.push(line);
        }

        // Trim trailing empty lines from display text.
        while display_lines.last().is_some_and(|l| l.trim().is_empty()) {
            display_lines.pop();
        }

        (display_lines.join("\n"), tool_calls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tool_calls_no_tools() {
        let text = "Here is some regular text.\nNo tools here.";
        let (display, tools) = ClaudeCodeClient::extract_tool_calls(text);
        assert_eq!(display, "Here is some regular text.\nNo tools here.");
        assert!(tools.is_empty());
    }

    #[test]
    fn test_extract_tool_calls_with_tools() {
        let text = "Let me read that file.\n\
                     TOOL_CALL: {\"name\": \"read_file\", \"input\": {\"path\": \"src/main.rs\"}}\n";
        let (display, tools) = ClaudeCodeClient::extract_tool_calls(text);
        assert_eq!(display, "Let me read that file.");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].0, "read_file");
        assert_eq!(
            tools[0].1.get("path").and_then(|v| v.as_str()),
            Some("src/main.rs")
        );
    }

    #[test]
    fn test_extract_tool_calls_multiple() {
        let text = "I'll search and then read.\n\
                     TOOL_CALL: {\"name\": \"search_files\", \"input\": {\"pattern\": \"fn main\"}}\n\
                     TOOL_CALL: {\"name\": \"read_file\", \"input\": {\"path\": \"Cargo.toml\"}}";
        let (display, tools) = ClaudeCodeClient::extract_tool_calls(text);
        assert_eq!(display, "I'll search and then read.");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].0, "search_files");
        assert_eq!(tools[1].0, "read_file");
    }

    #[test]
    fn test_extract_tool_calls_invalid_json_kept_as_text() {
        let text = "Some text\nTOOL_CALL: not valid json\nMore text";
        let (display, tools) = ClaudeCodeClient::extract_tool_calls(text);
        assert_eq!(display, "Some text\nTOOL_CALL: not valid json\nMore text");
        assert!(tools.is_empty());
    }

    #[test]
    fn test_summarize_tool_input_value() {
        let input = serde_json::json!({"path": "src/main.rs", "pattern": "fn main"});
        let summary = ClaudeCodeClient::summarize_tool_input_value(&input);
        assert!(summary.contains("path="));
        assert!(summary.contains("pattern="));
    }

    #[test]
    fn test_summarize_tool_input_empty() {
        let summary = ClaudeCodeClient::summarize_tool_input("");
        assert!(summary.is_empty());
    }
}
