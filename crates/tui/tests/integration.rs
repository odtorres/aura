//! Cross-module integration tests for the aura-editor-tui crate.
//!
//! These tests exercise the public API the way the `aura` binary does —
//! constructing buffers, opening tabs, performing edits, and reading state
//! through the same paths the runtime uses. Their job is to catch
//! regressions where modules pass unit tests in isolation but break when
//! wired together (e.g. an `EditorTab` constructor that quietly stops
//! detecting language, a config loader that no longer surfaces the chosen
//! theme, a `TabManager` reorder that desyncs the active index against
//! some other state).
//!
//! Add new tests below as `#[test] fn name()`. Keep them fast (no PTYs,
//! no LSP servers, no network) and deterministic. Heavy stack pieces
//! (the full `App` runtime, real git repos, real LSP) belong elsewhere
//! or in their own integration suites.

use aura_core::{AuthorId, Buffer};
use aura_tui::app::App;
use aura_tui::tab::EditorTab;

/// Helper: build an `EditorTab` from a fresh in-memory buffer with the
/// dark theme. This is the cheapest possible tab to construct — no file
/// path, so LSP startup is skipped, no language detected, no semantic
/// indexer.
fn fresh_tab() -> EditorTab {
    EditorTab::new(Buffer::new(), None, &aura_tui::config::theme_dark())
}

#[test]
fn editor_tab_starts_unmodified_and_shows_a_title() {
    let tab = fresh_tab();
    assert!(!tab.is_modified());
    assert!(!tab.title().is_empty());
}

#[test]
fn editor_tab_buffer_text_round_trips_through_rope() {
    let mut tab = fresh_tab();
    tab.buffer.insert(0, "hello world", AuthorId::human());
    assert_eq!(tab.buffer.text(), "hello world");
    assert!(tab.is_modified(), "insert should mark buffer modified");
}

#[test]
fn editor_tab_undo_returns_to_original_state() {
    let mut tab = fresh_tab();
    tab.buffer.insert(0, "hello", AuthorId::human());
    assert_eq!(tab.buffer.text(), "hello");
    tab.buffer.undo();
    assert_eq!(tab.buffer.text(), "");
}

#[test]
fn app_constructs_with_an_empty_buffer() {
    // Smoke test: App::new wires up config, theme, conversation store,
    // MCP server registry, AI client, etc. If any of those panics on a
    // fresh buffer in a clean environment, this catches it. The test
    // does not enter the event loop — that requires a real terminal.
    let buffer = Buffer::new();
    let app = App::new(buffer);
    // Sanity: at least the initial tab is present.
    assert!(app.tabs.count() >= 1);
}
