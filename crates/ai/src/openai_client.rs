//! OpenAI-compatible API streaming client.
//!
//! Supports OpenAI's chat completions API and any OpenAI-compatible
//! endpoint (Azure, Together, Groq, etc.). Uses curl for HTTP.

use crate::client::{AiEvent, Message, MessageContent, ToolDefinition};
use crate::AiConfig;
use std::sync::mpsc;

/// OpenAI-compatible API client.
pub struct OpenAiClient {
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    max_context_tokens: usize,
}

impl OpenAiClient {
    /// Create a new OpenAI client from config.
    pub fn new(config: AiConfig) -> anyhow::Result<Self> {
        Ok(Self {
            api_key: config.api_key,
            base_url: config.base_url,
            model: config.model,
            max_tokens: config.max_tokens,
            max_context_tokens: config.max_context_tokens,
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

    /// Stream a completion from the OpenAI API.
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
        let url = format!("{}/chat/completions", self.base_url);
        let api_key = self.api_key.clone();
        let max_tokens = self.max_tokens;
        let model = model.to_string();

        // Build OpenAI messages array.
        let mut oai_messages = vec![serde_json::json!({
            "role": "system",
            "content": system_prompt,
        })];
        for msg in &messages {
            let role = if msg.role == "user" {
                "user"
            } else {
                "assistant"
            };
            oai_messages.push(serde_json::json!({
                "role": role,
                "content": Self::message_text(&msg.content),
            }));
        }

        let body = serde_json::json!({
            "model": model,
            "messages": oai_messages,
            "max_tokens": max_tokens,
            "stream": false,
        });

        std::thread::Builder::new()
            .name("openai-request".to_string())
            .spawn(move || {
                let body_str = serde_json::to_string(&body).unwrap_or_default();
                let output = std::process::Command::new("curl")
                    .args([
                        "-sS",
                        "--max-time",
                        "120",
                        "-X",
                        "POST",
                        &url,
                        "-H",
                        &format!("Authorization: Bearer {}", api_key),
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
                                .get("choices")
                                .and_then(|c| c.get(0))
                                .and_then(|c| c.get("message"))
                                .and_then(|m| m.get("content"))
                                .and_then(|c| c.as_str())
                            {
                                let _ = tx.send(AiEvent::Token(content.to_string()));
                                let _ = tx.send(AiEvent::Done(content.to_string()));
                            } else if let Some(err) = val
                                .get("error")
                                .and_then(|e| e.get("message"))
                                .and_then(|m| m.as_str())
                            {
                                let _ = tx.send(AiEvent::Error(format!("OpenAI: {}", err)));
                            } else {
                                let _ = tx.send(AiEvent::Error(format!(
                                    "OpenAI: unexpected response: {}",
                                    text
                                )));
                            }
                        } else {
                            let _ =
                                tx.send(AiEvent::Error(format!("OpenAI: invalid JSON: {}", text)));
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
