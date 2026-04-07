//! Ollama local inference server client.
//!
//! Connects to a local Ollama instance for AI inference without
//! cloud API keys. Uses curl for HTTP.

use crate::client::{AiEvent, Message, MessageContent, ToolDefinition};
use std::sync::mpsc;

/// Ollama client for local inference.
pub struct OllamaClient {
    host: String,
    model: String,
    max_context_tokens: usize,
}

impl OllamaClient {
    /// Create a new Ollama client.
    pub fn new(host: &str, model: &str) -> anyhow::Result<Self> {
        Ok(Self {
            host: host.to_string(),
            model: model.to_string(),
            max_context_tokens: 8192,
        })
    }

    /// Get the context window limit.
    pub fn max_context_tokens(&self) -> usize {
        self.max_context_tokens
    }

    /// Extract text from MessageContent.
    fn message_text(content: &MessageContent) -> String {
        match content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| {
                    if let crate::client::ContentBlock::Text { text } = b {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Stream a completion from Ollama.
    pub fn stream_completion(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
    ) -> mpsc::Receiver<AiEvent> {
        self.stream_completion_with_model(system_prompt, messages, &self.model)
    }

    /// Stream a completion with tool definitions.
    pub fn stream_completion_with_tools(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        _tools: Vec<ToolDefinition>,
    ) -> mpsc::Receiver<AiEvent> {
        self.stream_completion(system_prompt, messages)
    }

    /// Stream a completion with a model override.
    pub fn stream_completion_with_model(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        model: &str,
    ) -> mpsc::Receiver<AiEvent> {
        let (tx, rx) = mpsc::channel();
        let url = format!("{}/api/chat", self.host);
        let model = model.to_string();

        // Build Ollama messages array.
        let mut ollama_messages = vec![serde_json::json!({
            "role": "system",
            "content": system_prompt,
        })];
        for msg in &messages {
            let role = if msg.role == "user" {
                "user"
            } else {
                "assistant"
            };
            ollama_messages.push(serde_json::json!({
                "role": role,
                "content": Self::message_text(&msg.content),
            }));
        }

        let body = serde_json::json!({
            "model": model,
            "messages": ollama_messages,
            "stream": false,
        });

        std::thread::Builder::new()
            .name("ollama-request".to_string())
            .spawn(move || {
                let body_str = serde_json::to_string(&body).unwrap_or_default();
                let output = std::process::Command::new("curl")
                    .args([
                        "-sS",
                        "--max-time",
                        "300",
                        "-X",
                        "POST",
                        &url,
                        "-H",
                        "Content-Type: application/json",
                        "-d",
                        &body_str,
                    ])
                    .output();

                match output {
                    Ok(out) => {
                        let text = String::from_utf8_lossy(&out.stdout);
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(content) = val
                                .get("message")
                                .and_then(|m| m.get("content"))
                                .and_then(|c| c.as_str())
                            {
                                let _ = tx.send(AiEvent::Token(content.to_string()));
                                let _ = tx.send(AiEvent::Done(content.to_string()));
                            } else if let Some(err) = val.get("error").and_then(|e| e.as_str()) {
                                let _ = tx.send(AiEvent::Error(format!("Ollama: {}", err)));
                            } else {
                                let _ = tx
                                    .send(AiEvent::Error(format!("Ollama: unexpected: {}", text)));
                            }
                        } else {
                            let _ =
                                tx.send(AiEvent::Error(format!("Ollama: invalid JSON: {}", text)));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(AiEvent::Error(format!("curl failed: {e}")));
                    }
                }
            })
            .ok();

        rx
    }

    /// Stream with tools and model override.
    pub fn stream_completion_with_tools_and_model(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        _tools: Vec<ToolDefinition>,
        model: &str,
    ) -> mpsc::Receiver<AiEvent> {
        self.stream_completion_with_model(system_prompt, messages, model)
    }
}
