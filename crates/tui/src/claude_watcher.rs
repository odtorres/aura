//! Claude Code activity watcher.
//!
//! Tails Claude Code's JSONL conversation logs in real-time to observe
//! what Claude Code is doing (tool calls, responses, progress events).
//! Sends parsed activity events to the main event loop via mpsc channels.

use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// An activity event parsed from Claude Code's JSONL logs.
#[derive(Debug, Clone)]
pub enum ClaudeActivity {
    /// Claude Code received a user message.
    UserMessage {
        /// The message text.
        text: String,
        /// Session identifier.
        session_id: String,
    },
    /// Claude Code generated a response.
    AssistantMessage {
        /// Summary of the response (first 200 chars).
        text: String,
        /// Model used.
        model: String,
        /// Session identifier.
        session_id: String,
    },
    /// Claude Code invoked a tool.
    ToolCall {
        /// Tool name (e.g., "Read", "Write", "Bash").
        name: String,
        /// Brief description of the input.
        input_summary: String,
        /// Session identifier.
        session_id: String,
    },
    /// Progress update.
    Progress {
        /// Description of what's happening.
        message: String,
        /// Session identifier.
        session_id: String,
    },
}

/// Watches Claude Code's JSONL logs for real-time activity.
pub struct ClaudeWatcher {
    /// Receive activity events from the watcher thread.
    event_rx: mpsc::Receiver<ClaudeActivity>,
    /// Shutdown flag.
    shutdown: Arc<Mutex<bool>>,
    /// Latest activity description (for status bar).
    pub latest_activity: Option<String>,
}

impl ClaudeWatcher {
    /// Start watching Claude Code's logs for the given project directory.
    pub fn start(project_dir: &std::path::Path) -> Option<Self> {
        let claude_dir = dirs_home()?.join(".claude").join("projects");
        if !claude_dir.is_dir() {
            return None;
        }

        // Compute the project hash (path with / replaced by -).
        let project_hash = project_dir.to_string_lossy().replace('/', "-");
        let watch_dir = claude_dir.join(&project_hash);

        if !watch_dir.is_dir() {
            tracing::debug!("No Claude Code project dir: {}", watch_dir.display());
            return None;
        }

        let (event_tx, event_rx) = mpsc::channel();
        let shutdown = Arc::new(Mutex::new(false));
        let shutdown_clone = shutdown.clone();

        thread::Builder::new()
            .name("claude-watcher".to_string())
            .spawn(move || {
                watcher_loop(watch_dir, event_tx, shutdown_clone);
            })
            .ok()?;

        Some(Self {
            event_rx,
            shutdown,
            latest_activity: None,
        })
    }

    /// Poll for new activity events (non-blocking).
    pub fn poll_events(&mut self) -> Vec<ClaudeActivity> {
        let mut events = Vec::new();
        loop {
            match self.event_rx.try_recv() {
                Ok(event) => {
                    // Update latest activity description.
                    self.latest_activity = Some(match &event {
                        ClaudeActivity::UserMessage { text, .. } => {
                            format!("CC: User: {}", truncate(text, 40))
                        }
                        ClaudeActivity::AssistantMessage { text, .. } => {
                            format!("CC: {}", truncate(text, 40))
                        }
                        ClaudeActivity::ToolCall {
                            name,
                            input_summary,
                            ..
                        } => {
                            format!("CC: {name}: {}", truncate(input_summary, 30))
                        }
                        ClaudeActivity::Progress { message, .. } => {
                            format!("CC: {}", truncate(message, 40))
                        }
                    });
                    events.push(event);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        events
    }

    /// Shut down the watcher thread.
    pub fn shutdown(&self) {
        *self.shutdown.lock().expect("lock poisoned") = true;
    }
}

/// Find the most recently modified .jsonl file in a directory.
fn find_latest_jsonl(dir: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
        .map(|e| e.path())
}

/// Main watcher loop running on a background thread.
fn watcher_loop(
    watch_dir: PathBuf,
    event_tx: mpsc::Sender<ClaudeActivity>,
    shutdown: Arc<Mutex<bool>>,
) {
    let mut current_file: Option<PathBuf> = None;
    let mut file_pos: u64 = 0;

    loop {
        if *shutdown.lock().expect("lock poisoned") {
            break;
        }

        // Find the latest JSONL file.
        let latest = find_latest_jsonl(&watch_dir);

        // If the file changed, reset position.
        if latest != current_file {
            if let Some(ref path) = latest {
                // Seek to end of new file (only read new lines).
                file_pos = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                current_file = latest;
            }
        }

        // Read new lines from the current file.
        if let Some(ref path) = current_file {
            if let Ok(mut file) = std::fs::File::open(path) {
                if file.seek(SeekFrom::Start(file_pos)).is_ok() {
                    let reader = BufReader::new(&mut file);
                    for line in reader.lines() {
                        match line {
                            Ok(line) if !line.trim().is_empty() => {
                                if let Some(activity) = parse_jsonl_line(&line) {
                                    if event_tx.send(activity).is_err() {
                                        return; // Channel closed.
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    // Update position.
                    file_pos = file.stream_position().unwrap_or(file_pos);
                }
            }
        }

        // Sleep before next poll.
        thread::sleep(Duration::from_millis(500));
    }
}

/// Parse a single JSONL line into a ClaudeActivity event.
fn parse_jsonl_line(line: &str) -> Option<ClaudeActivity> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let msg_type = v.get("type")?.as_str()?;
    let session_id = v
        .get("sessionId")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();

    match msg_type {
        "user" => {
            let message = v.get("message")?;
            let content = message.get("content")?;
            // Extract text from content blocks.
            let text = if let Some(arr) = content.as_array() {
                arr.iter()
                    .filter_map(|block| {
                        if block.get("type")?.as_str()? == "text" {
                            block.get("text").and_then(|t| t.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else if let Some(s) = content.as_str() {
                s.to_string()
            } else {
                return None;
            };

            if text.is_empty() {
                return None;
            }
            Some(ClaudeActivity::UserMessage { text, session_id })
        }
        "assistant" => {
            let message = v.get("message")?;
            let model = message
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string();
            let content = message.get("content")?;

            if let Some(arr) = content.as_array() {
                // Check for tool_use blocks.
                for block in arr {
                    if let Some("tool_use") = block.get("type").and_then(|t| t.as_str()) {
                        let name = block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let input = block.get("input").cloned().unwrap_or_default();
                        let input_summary = summarize_tool_input(&name, &input);
                        return Some(ClaudeActivity::ToolCall {
                            name,
                            input_summary,
                            session_id,
                        });
                    }
                }
                // Check for text blocks.
                let text: String = arr
                    .iter()
                    .filter_map(|block| {
                        if block.get("type")?.as_str()? == "text" {
                            block.get("text").and_then(|t| t.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");

                if !text.is_empty() {
                    return Some(ClaudeActivity::AssistantMessage {
                        text,
                        model,
                        session_id,
                    });
                }
            }
            None
        }
        "progress" => {
            let data = v.get("data")?;
            let progress_type = data.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let message = match progress_type {
                "hook_progress" => {
                    let hook = data
                        .get("hookName")
                        .and_then(|h| h.as_str())
                        .unwrap_or("hook");
                    format!("Hook: {hook}")
                }
                "agent_progress" => {
                    let msg = data.get("message").and_then(|m| {
                        m.get("message")
                            .and_then(|mm| mm.get("content"))
                            .and_then(|c| c.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|b| b.get("text"))
                            .and_then(|t| t.as_str())
                    });
                    msg.map(|t| truncate(t, 60).to_string())
                        .unwrap_or_else(|| "Agent working...".to_string())
                }
                _ => return None,
            };
            Some(ClaudeActivity::Progress {
                message,
                session_id,
            })
        }
        _ => None,
    }
}

/// Summarize a tool's input for display.
fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Read" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
            .unwrap_or_default(),
        "Write" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
            .unwrap_or_default(),
        "Edit" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
            .unwrap_or_default(),
        "Bash" => input
            .get("command")
            .and_then(|c| c.as_str())
            .map(|c| truncate(c, 40).to_string())
            .unwrap_or_default(),
        "Grep" => input
            .get("pattern")
            .and_then(|p| p.as_str())
            .map(|p| format!("/{p}/"))
            .unwrap_or_default(),
        "Glob" => input
            .get("pattern")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

/// Truncate a string to max characters (char-boundary safe).
fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}...")
    }
}

/// Get home directory.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_assistant_tool_call() {
        let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-6","role":"assistant","content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/tmp/test.rs"}}]},"sessionId":"abc123","timestamp":"2026-03-28T10:00:00Z"}"#;
        let activity = parse_jsonl_line(line).unwrap();
        match activity {
            ClaudeActivity::ToolCall {
                name,
                input_summary,
                session_id,
            } => {
                assert_eq!(name, "Read");
                assert_eq!(input_summary, "test.rs");
                assert_eq!(session_id, "abc123");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_user_message() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"fix the bug"}]},"sessionId":"abc123"}"#;
        let activity = parse_jsonl_line(line).unwrap();
        match activity {
            ClaudeActivity::UserMessage { text, .. } => {
                assert_eq!(text, "fix the bug");
            }
            _ => panic!("Expected UserMessage"),
        }
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello...");
    }
}
