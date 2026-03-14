//! aura-tui: Terminal rendering layer for AURA.
//!
//! Handles drawing the buffer, status bar, command bar, gutter,
//! and will eventually render ghost-text overlays and diff views.

pub mod app;
pub mod config;
pub mod embedded_terminal;
pub mod file_picker;
pub mod file_tree;
pub mod git;
pub mod highlight;
pub mod input;
pub mod lsp;
pub mod mcp_client;
pub mod mcp_server;
pub mod plugin;
pub mod render;
pub mod semantic_index;
pub mod speculative;
