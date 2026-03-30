#![warn(missing_docs)]
//! aura-tui: Terminal rendering layer for AURA.
//!
//! Handles drawing the buffer, status bar, command bar, gutter,
//! and will eventually render ghost-text overlays and diff views.

pub mod acp_server;
pub mod ai_visor;
pub mod app;
pub mod branch_picker;
pub mod chat_panel;
pub mod chat_tools;
pub mod claude_watcher;
pub mod collab;
pub mod command_palette;
pub mod config;
pub mod conversation_history;
pub mod dap;
pub mod debug_panel;
pub mod diff_view;
pub mod embedded_terminal;
pub mod file_picker;
pub mod file_tree;
pub mod git;
pub mod git_graph;
pub mod help;
pub mod highlight;
pub mod input;
pub mod lsp;
pub mod mcp_client;
pub mod mcp_server;
pub mod merge_view;
pub mod plugin;
pub mod project_search;
pub mod render;
pub mod semantic_index;
pub mod session;
pub mod settings_modal;
pub mod snippets;
pub mod source_control;
pub mod speculative;
pub mod tab;
pub mod undo_tree;
pub mod update;
