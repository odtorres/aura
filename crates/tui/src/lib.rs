#![warn(missing_docs)]
//! aura-tui: Terminal rendering layer for AURA.
//!
//! Handles drawing the buffer, status bar, command bar, gutter,
//! and will eventually render ghost-text overlays and diff views.

// Public surface consumed outside the crate.
// - `app`, `render` are used by both the editor binary (aura) and the
//   render benches.
// - `tab`, `config` are used by the integration test scaffold in
//   `crates/tui/tests/integration.rs`.
// Everything else is internal — kept `pub(crate)` so the dead-code lint
// can spot unused public items inside this crate.
pub mod app;
pub mod config;
pub mod render;
pub mod tab;

pub(crate) mod acp_server;
pub(crate) mod agent_plan;
pub(crate) mod agent_timeline;
pub(crate) mod ai_visor;
pub(crate) mod apply_model;
pub(crate) mod bookmarks;
pub(crate) mod branch_picker;
pub(crate) mod chat_panel;
pub(crate) mod chat_tools;
pub(crate) mod checkpoints;
pub(crate) mod claude_watcher;
pub(crate) mod collab;
pub(crate) mod command_palette;
pub(crate) mod context_menu;
pub(crate) mod context_pin;
pub(crate) mod conversation_history;
pub(crate) mod dap;
pub(crate) mod debug_panel;
pub(crate) mod diff_view;
pub(crate) mod embedded_terminal;
pub(crate) mod file_picker;
pub(crate) mod file_tree;
pub(crate) mod file_watcher;
pub(crate) mod git;
pub(crate) mod git_graph;
pub(crate) mod git_worker;
pub(crate) mod help;
pub(crate) mod highlight;
pub(crate) mod http_client;
pub(crate) mod image_preview;
pub(crate) mod input;
pub(crate) mod local_history;
pub(crate) mod lsp;
pub(crate) mod markdown_preview;
pub(crate) mod marketplace;
pub(crate) mod mcp_client;
pub(crate) mod mcp_server;
pub(crate) mod merge_view;
pub(crate) mod notebook;
pub(crate) mod plugin;
pub(crate) mod project_search;
pub(crate) mod rag_index;
pub(crate) mod rebase_modal;
pub(crate) mod remote;
pub(crate) mod semantic_index;
pub(crate) mod session;
pub(crate) mod settings_modal;
pub(crate) mod snippets;
pub(crate) mod source_control;
pub(crate) mod speculative;
pub(crate) mod subagent;
pub(crate) mod todo_panel;
pub(crate) mod token_tracker;
pub(crate) mod undo_tree;
pub(crate) mod update;
