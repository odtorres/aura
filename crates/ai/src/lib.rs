#![warn(missing_docs)]
//! aura-ai: AI integration layer for AURA.
//!
//! This crate provides:
//! - Multi-provider AI support (Anthropic, OpenAI, Ollama)
//! - Claude Code CLI backend (no API key needed)
//! - Context assembly (buffer content, cursor, edit history)
//! - Response parsing (AI text → concrete edit operations)

pub mod claude_code;
pub mod client;
pub mod context;
pub mod ollama_client;
pub mod openai_client;

pub use claude_code::ClaudeCodeClient;
pub use client::{
    agent_tools, editor_tools, tool_permission, AiEvent, AnthropicClient, ContentBlock, Message,
    MessageContent, ToolDefinition, ToolPermission,
};
pub use context::{estimate_tokens, DiagnosticSummary, EditorContext};
pub use ollama_client::OllamaClient;
pub use openai_client::OpenAiClient;

/// Supported AI providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    /// Anthropic Claude API.
    Anthropic,
    /// OpenAI-compatible API (GPT-4, etc.).
    OpenAI,
    /// Ollama local inference server.
    Ollama,
}

impl ProviderType {
    /// Parse from a string label.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" | "gpt" => Some(Self::OpenAI),
            "ollama" | "local" => Some(Self::Ollama),
            _ => None,
        }
    }

    /// Display name.
    pub fn label(&self) -> &str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAI => "openai",
            Self::Ollama => "ollama",
        }
    }

    /// Common model names for this provider.
    pub fn common_models(&self) -> &[&str] {
        match self {
            Self::Anthropic => &[
                "claude-sonnet-4-20250514",
                "claude-haiku-4-5-20251001",
                "claude-opus-4-20250514",
            ],
            Self::OpenAI => &["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "gpt-3.5-turbo"],
            Self::Ollama => &[
                "llama3.1",
                "codellama",
                "mistral",
                "deepseek-coder",
                "qwen2.5-coder",
            ],
        }
    }
}

/// All available provider types.
pub const PROVIDERS: &[ProviderType] = &[
    ProviderType::Anthropic,
    ProviderType::OpenAI,
    ProviderType::Ollama,
];

/// Configuration for the AI integration.
#[derive(Debug, Clone)]
pub struct AiConfig {
    /// AI provider to use.
    pub provider: ProviderType,
    /// API key (Anthropic or OpenAI).
    pub api_key: String,
    /// Base URL for the API.
    pub base_url: String,
    /// Model identifier.
    pub model: String,
    /// Maximum tokens for the response.
    pub max_tokens: u32,
    /// Total context window token limit for the model.
    pub max_context_tokens: usize,
    /// OpenAI API key (if different from main api_key).
    pub openai_api_key: String,
    /// OpenAI base URL.
    pub openai_base_url: String,
    /// Ollama host URL.
    pub ollama_host: String,
}

impl AiConfig {
    /// Load config from environment variables, auto-detecting the provider.
    pub fn from_env() -> Option<Self> {
        // Try Anthropic first.
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            return Some(Self {
                provider: ProviderType::Anthropic,
                api_key: key,
                openai_api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
                ollama_host: std::env::var("OLLAMA_HOST")
                    .unwrap_or_else(|_| "http://localhost:11434".to_string()),
                ..Self::default()
            });
        }
        // Try OpenAI.
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            return Some(Self {
                provider: ProviderType::OpenAI,
                api_key: key.clone(),
                openai_api_key: key,
                ollama_host: std::env::var("OLLAMA_HOST")
                    .unwrap_or_else(|_| "http://localhost:11434".to_string()),
                model: "gpt-4o".to_string(),
                ..Self::default()
            });
        }
        // Try Ollama.
        if let Ok(host) = std::env::var("OLLAMA_HOST") {
            return Some(Self {
                provider: ProviderType::Ollama,
                ollama_host: host,
                model: "llama3.1".to_string(),
                ..Self::default()
            });
        }
        // Check if Ollama is running on default port.
        if std::net::TcpStream::connect("127.0.0.1:11434").is_ok() {
            return Some(Self {
                provider: ProviderType::Ollama,
                model: "llama3.1".to_string(),
                ..Self::default()
            });
        }
        None
    }

    /// Create a config for a specific provider.
    pub fn for_provider(&self, provider: ProviderType) -> Self {
        let mut config = self.clone();
        config.provider = provider;
        match provider {
            ProviderType::Anthropic => {
                config.base_url = "https://api.anthropic.com".to_string();
            }
            ProviderType::OpenAI => {
                config.api_key = self.openai_api_key.clone();
                config.base_url = if self.openai_base_url.is_empty() {
                    "https://api.openai.com/v1".to_string()
                } else {
                    self.openai_base_url.clone()
                };
            }
            ProviderType::Ollama => {
                config.base_url = self.ollama_host.clone();
            }
        }
        config
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: ProviderType::Anthropic,
            api_key: String::new(),
            base_url: "https://api.anthropic.com".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            max_context_tokens: 100_000,
            openai_api_key: String::new(),
            openai_base_url: "https://api.openai.com/v1".to_string(),
            ollama_host: "http://localhost:11434".to_string(),
        }
    }
}

/// Unified AI backend that dispatches to the appropriate provider.
pub enum AiBackend {
    /// Direct Anthropic API client (requires `ANTHROPIC_API_KEY`).
    Api(AnthropicClient),
    /// OpenAI-compatible API client (requires `OPENAI_API_KEY`).
    OpenAi(OpenAiClient),
    /// Ollama local inference server.
    Ollama(OllamaClient),
    /// Claude Code CLI backend (uses existing Claude Code authentication).
    ClaudeCode(ClaudeCodeClient),
}

impl AiBackend {
    /// Try to create an AI backend, auto-detecting from environment.
    pub fn auto_detect() -> Option<Self> {
        if let Some(config) = AiConfig::from_env() {
            match config.provider {
                ProviderType::Anthropic => {
                    if let Ok(client) = AnthropicClient::new(config) {
                        tracing::info!("Using Anthropic API backend");
                        return Some(AiBackend::Api(client));
                    }
                }
                ProviderType::OpenAI => {
                    if let Ok(client) = OpenAiClient::new(config) {
                        tracing::info!("Using OpenAI API backend");
                        return Some(AiBackend::OpenAi(client));
                    }
                }
                ProviderType::Ollama => {
                    if let Ok(client) = OllamaClient::new(&config.ollama_host, &config.model) {
                        tracing::info!("Using Ollama backend");
                        return Some(AiBackend::Ollama(client));
                    }
                }
            }
        }

        // Fallback: Claude Code CLI.
        if let Some(client) = ClaudeCodeClient::new() {
            tracing::info!("Using Claude Code CLI backend (no API key needed)");
            return Some(AiBackend::ClaudeCode(client));
        }

        tracing::warn!("No AI backend available");
        None
    }

    /// Return the configured context window token limit.
    pub fn max_context_tokens(&self) -> usize {
        match self {
            AiBackend::Api(c) => c.max_context_tokens(),
            AiBackend::OpenAi(c) => c.max_context_tokens(),
            AiBackend::Ollama(c) => c.max_context_tokens(),
            AiBackend::ClaudeCode(c) => c.max_context_tokens(),
        }
    }

    /// Send a streaming completion request.
    pub fn stream_completion(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
    ) -> std::sync::mpsc::Receiver<AiEvent> {
        match self {
            AiBackend::Api(c) => c.stream_completion(system_prompt, messages),
            AiBackend::OpenAi(c) => c.stream_completion(system_prompt, messages),
            AiBackend::Ollama(c) => c.stream_completion(system_prompt, messages),
            AiBackend::ClaudeCode(c) => c.stream_completion(system_prompt, messages),
        }
    }

    /// Send a streaming completion with tool definitions.
    pub fn stream_completion_with_tools(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> std::sync::mpsc::Receiver<AiEvent> {
        match self {
            AiBackend::Api(c) => c.stream_completion_with_tools(system_prompt, messages, tools),
            AiBackend::OpenAi(c) => c.stream_completion_with_tools(system_prompt, messages, tools),
            AiBackend::Ollama(c) => c.stream_completion_with_tools(system_prompt, messages, tools),
            AiBackend::ClaudeCode(c) => {
                c.stream_completion_with_tools(system_prompt, messages, tools)
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
            AiBackend::Api(c) => {
                c.stream_completion_with_model(system_prompt, messages, model_override)
            }
            AiBackend::OpenAi(c) => {
                c.stream_completion_with_model(system_prompt, messages, model_override)
            }
            AiBackend::Ollama(c) => {
                c.stream_completion_with_model(system_prompt, messages, model_override)
            }
            AiBackend::ClaudeCode(c) => c.stream_completion(system_prompt, messages),
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
            AiBackend::Api(c) => c.stream_completion_with_tools_and_model(
                system_prompt,
                messages,
                tools,
                model_override,
            ),
            AiBackend::OpenAi(c) => c.stream_completion_with_tools_and_model(
                system_prompt,
                messages,
                tools,
                model_override,
            ),
            AiBackend::Ollama(c) => c.stream_completion_with_tools_and_model(
                system_prompt,
                messages,
                tools,
                model_override,
            ),
            AiBackend::ClaudeCode(c) => {
                c.stream_completion_with_tools(system_prompt, messages, tools)
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
            AiBackend::OpenAi(_) => "OpenAI API",
            AiBackend::Ollama(_) => "Ollama (local)",
            AiBackend::ClaudeCode(_) => "Claude Code",
        }
    }
}
