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
}

/// A message in the conversation.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Request body for the Anthropic Messages API.
#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
    stream: bool,
}

/// Streaming event types from the API.
#[derive(Debug, Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<Delta>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    text: Option<String>,
}

/// Client for the Anthropic Messages API with streaming.
pub struct AnthropicClient {
    config: AiConfig,
    http: reqwest::Client,
}

impl AnthropicClient {
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
        let (tx, rx) = mpsc::channel();

        let request = MessagesRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            system: system_prompt.to_string(),
            messages,
            stream: true,
        };

        let http = self.http.clone();
        let base_url = self.config.base_url.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

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
            let mut accumulated = String::new();

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
                    if event.event_type == "content_block_delta" {
                        if let Some(delta) = &event.delta {
                            if let Some(text) = &delta.text {
                                accumulated.push_str(text);
                                let _ = tx.send(AiEvent::Token(text.clone()));
                            }
                        }
                    }
                }
            }

            let _ = tx.send(AiEvent::Done(accumulated));
            return Ok(());
        }
    }
}
