#![warn(missing_docs)]
//! aura-ai: AI integration layer for AURA.
//!
//! This crate provides:
//! - Anthropic API streaming client for Claude
//! - Claude Code CLI backend (no API key needed)
//! - Context assembly (buffer content, cursor, edit history)
//! - Response parsing (AI text → concrete edit operations)

pub mod claude_code;
pub mod client;
pub mod context;

pub use claude_code::ClaudeCodeClient;
pub use client::{
    agent_tools, editor_tools, tool_permission, AiEvent, AnthropicClient, ContentBlock, Message,
    MessageContent, ToolDefinition, ToolPermission,
};
pub use context::{estimate_tokens, DiagnosticSummary, EditorContext};

/// Configuration for the AI integration.
#[derive(Debug, Clone)]
pub struct AiConfig {
    /// Anthropic API key.
    pub api_key: String,
    /// Base URL for the API.
    pub base_url: String,
    /// Model identifier.
    pub model: String,
    /// Maximum tokens for the response.
    pub max_tokens: u32,
    /// Total context window token limit for the model.
    ///
    /// Used by context assembly to truncate prompts so they fit within the
    /// model's context window. Defaults to 100_000 (claude-sonnet).
    pub max_context_tokens: usize,
}

impl AiConfig {
    /// Load config from environment variables.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok()?;
        Some(Self {
            api_key,
            ..Self::default()
        })
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.anthropic.com".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            max_context_tokens: 100_000,
        }
    }
}

/// Unified AI backend that dispatches to either the Anthropic API or Claude Code CLI.
pub enum AiBackend {
    /// Direct Anthropic API client (requires `ANTHROPIC_API_KEY`).
    Api(AnthropicClient),
    /// Claude Code CLI backend (uses existing Claude Code authentication).
    ClaudeCode(ClaudeCodeClient),
}

impl AiBackend {
    /// Try to create an AI backend, preferring the API client if an API key is set,
    /// otherwise falling back to Claude Code CLI if available.
    pub fn auto_detect() -> Option<Self> {
        // First try: direct API key.
        if let Some(config) = AiConfig::from_env() {
            if let Ok(client) = AnthropicClient::new(config) {
                tracing::info!("Using Anthropic API backend");
                return Some(AiBackend::Api(client));
            }
        }

        // Fallback: Claude Code CLI.
        if let Some(client) = ClaudeCodeClient::new() {
            tracing::info!("Using Claude Code CLI backend (no API key needed)");
            return Some(AiBackend::ClaudeCode(client));
        }

        tracing::warn!("No AI backend available (no API key and no claude CLI)");
        None
    }

    /// Return the configured context window token limit.
    pub fn max_context_tokens(&self) -> usize {
        match self {
            AiBackend::Api(client) => client.max_context_tokens(),
            AiBackend::ClaudeCode(client) => client.max_context_tokens(),
        }
    }

    /// Send a streaming completion request. Returns a receiver for streaming events.
    pub fn stream_completion(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
    ) -> std::sync::mpsc::Receiver<AiEvent> {
        match self {
            AiBackend::Api(client) => client.stream_completion(system_prompt, messages),
            AiBackend::ClaudeCode(client) => client.stream_completion(system_prompt, messages),
        }
    }

    /// Send a streaming completion request with tool definitions.
    ///
    /// Both backends support tool use: the API backend uses native tool_use
    /// content blocks, while the CLI backend encodes tools in the prompt
    /// and parses `TOOL_CALL:` directives from the response.
    pub fn stream_completion_with_tools(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> std::sync::mpsc::Receiver<AiEvent> {
        match self {
            AiBackend::Api(client) => {
                client.stream_completion_with_tools(system_prompt, messages, tools)
            }
            AiBackend::ClaudeCode(client) => {
                client.stream_completion_with_tools(system_prompt, messages, tools)
            }
        }
    }

    /// Send a streaming completion with a model override.
    pub fn stream_completion_with_model(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        model_override: &str,
    ) -> std::sync::mpsc::Receiver<AiEvent> {
        match self {
            AiBackend::Api(client) => {
                client.stream_completion_with_model(system_prompt, messages, model_override)
            }
            AiBackend::ClaudeCode(client) => {
                // CLI backend doesn't support model override; fall back to default.
                client.stream_completion(system_prompt, messages)
            }
        }
    }

    /// Send a streaming completion with tools and a model override.
    pub fn stream_completion_with_tools_and_model(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        model_override: &str,
    ) -> std::sync::mpsc::Receiver<AiEvent> {
        match self {
            AiBackend::Api(client) => client.stream_completion_with_tools_and_model(
                system_prompt,
                messages,
                tools,
                model_override,
            ),
            AiBackend::ClaudeCode(client) => {
                client.stream_completion_with_tools(system_prompt, messages, tools)
            }
        }
    }

    /// Whether this backend supports tool use.
    pub fn supports_tools(&self) -> bool {
        true
    }

    /// Return a label describing which backend is active.
    pub fn label(&self) -> &str {
        match self {
            AiBackend::Api(_) => "Anthropic API",
            AiBackend::ClaudeCode(_) => "Claude Code",
        }
    }
}
