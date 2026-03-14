#![warn(missing_docs)]
//! aura-ai: AI integration layer for AURA.
//!
//! This crate provides:
//! - Anthropic API streaming client for Claude
//! - Context assembly (buffer content, cursor, edit history)
//! - Response parsing (AI text → concrete edit operations)

pub mod client;
pub mod context;

pub use client::{AiEvent, AnthropicClient, Message};
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
