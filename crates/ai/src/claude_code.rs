//! Claude Code CLI backend for AI completions.
//!
//! Uses the `claude` CLI tool (Claude Code) as an alternative to
//! direct Anthropic API access. This allows users who have Claude Code
//! authenticated to use AI features without a separate API key.
//!
//! The CLI is invoked in *print mode* (`claude -p`) which runs
//! non-interactively and streams text to stdout.  We use
//! `--output-format stream-json` so each chunk arrives as a JSON line
//! that we can parse for incremental token display.

use crate::AiEvent;
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
        messages: Vec<crate::Message>,
    ) -> mpsc::Receiver<AiEvent> {
        let (tx, rx) = mpsc::channel();

        // Build the full prompt: system context + user messages.
        let mut full_prompt = String::new();
        full_prompt.push_str(system_prompt);
        full_prompt.push_str("\n\n");
        for msg in &messages {
            if msg.role == "user" {
                full_prompt.push_str(&msg.content);
                full_prompt.push('\n');
            }
        }

        std::thread::spawn(move || {
            let result = Self::run_claude_cli(&full_prompt, &tx);
            if let Err(e) = result {
                let _ = tx.send(AiEvent::Error(e.to_string()));
            }
        });

        rx
    }

    /// Internal: spawn `claude -p` and stream its stdout.
    ///
    /// Uses `--output-format stream-json` for structured streaming.
    /// Each line is a JSON object; we extract text from `content_block_delta`
    /// events and fall back to raw text if JSON parsing fails.
    fn run_claude_cli(
        prompt: &str,
        tx: &mpsc::Sender<AiEvent>,
    ) -> anyhow::Result<()> {
        debug!("Spawning claude -p (prompt length: {} chars)", prompt.len());

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
        let mut accumulated = String::new();
        let mut result_text: Option<String> = None;

        for line in reader.lines() {
            match line {
                Ok(raw) => {
                    if raw.trim().is_empty() {
                        continue;
                    }
                    // Try to parse as stream-json event.
                    match Self::parse_stream_json_event(&raw) {
                        StreamJsonEvent::TextDelta(text) => {
                            accumulated.push_str(&text);
                            let _ = tx.send(AiEvent::Token(text));
                        }
                        StreamJsonEvent::Result(text) => {
                            result_text = Some(text);
                        }
                        StreamJsonEvent::Other => {}
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
            // Read stderr for error details.
            let stderr_msg = child
                .stderr
                .take()
                .and_then(|stderr| {
                    let reader = BufReader::new(stderr);
                    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
                    if lines.is_empty() {
                        None
                    } else {
                        Some(lines.join("\n"))
                    }
                })
                .unwrap_or_else(|| format!("claude exited with status {status}"));
            anyhow::bail!("Claude Code error: {stderr_msg}");
        }

        // Use the result event text as fallback when no tokens were accumulated
        // (e.g. if the stream format changed and text_delta events were not matched).
        let final_text = if accumulated.is_empty() {
            if let Some(ref text) = result_text {
                // Send the result as a single token so the proposal gets populated.
                let _ = tx.send(AiEvent::Token(text.clone()));
            }
            result_text.unwrap_or(accumulated)
        } else {
            accumulated
        };

        let _ = tx.send(AiEvent::Done(final_text));
        Ok(())
    }

    /// Parse a stream-json line into a categorized event.
    ///
    /// Claude Code `--output-format stream-json` emits JSON objects.
    /// We categorize them as text deltas (incremental tokens), result
    /// events (final complete text), or other (ignored).
    fn parse_stream_json_event(line: &str) -> StreamJsonEvent {
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return StreamJsonEvent::Other,
        };

        // Stream event wrapping: {"type":"stream_event","event":{...}}
        if let Some(event) = v.get("event") {
            // content_block_delta with text_delta
            if let Some(delta) = event.get("delta") {
                if delta.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                    if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                        return StreamJsonEvent::TextDelta(text.to_string());
                    }
                }
            }
        }

        // Top-level result event: {"type":"result","result":"..."}
        if v.get("type").and_then(|t| t.as_str()) == Some("result") {
            if let Some(text) = v.get("result").and_then(|r| r.as_str()) {
                return StreamJsonEvent::Result(text.to_string());
            }
        }

        StreamJsonEvent::Other
    }
}

/// Categorized stream-json event from the Claude Code CLI.
enum StreamJsonEvent {
    /// Incremental text token from a content_block_delta.
    TextDelta(String),
    /// Final complete result text.
    Result(String),
    /// Any other event type (ignored).
    Other,
}
