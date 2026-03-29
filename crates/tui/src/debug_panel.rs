//! Debug panel state for the integrated DAP debugger.
//!
//! Manages the UI state for the debug panel (bottom panel showing call stack,
//! variables, and program output) and the overall debug session state.

use crate::dap::{DapScope, DapStackFrame};
use std::path::PathBuf;

/// Which sub-tab is active in the debug panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugTab {
    /// Call stack view.
    CallStack,
    /// Variables view.
    Variables,
    /// Program output view.
    Output,
}

/// Current debug session status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    /// No debug session active.
    Inactive,
    /// Program is running.
    Running,
    /// Program is paused with a reason (e.g. "breakpoint", "step").
    Stopped(String),
    /// Debug session has ended.
    Terminated,
}

/// A variable node in the flattened variable tree.
#[derive(Debug, Clone)]
pub struct VariableNode {
    /// Variable name.
    pub name: String,
    /// Display value.
    pub value: String,
    /// Type name.
    pub type_name: String,
    /// Indentation level (0 = top-level, 1 = child, etc.).
    pub indent: usize,
    /// Whether this variable has children that can be expanded.
    pub expandable: bool,
    /// Whether children are currently shown.
    pub expanded: bool,
    /// DAP variables reference for fetching children.
    pub variables_reference: u64,
}

/// Debug session state (shared between DapClient events and UI).
#[derive(Debug)]
pub struct DebugState {
    /// Overall session status.
    pub status: SessionStatus,
    /// File where execution is currently paused.
    pub stopped_file: Option<PathBuf>,
    /// 0-indexed line where execution is paused.
    pub stopped_line: Option<usize>,
    /// Thread ID that is currently stopped.
    pub stopped_thread_id: Option<u64>,
    /// Stack frames for the stopped thread.
    pub stack_frames: Vec<DapStackFrame>,
    /// Currently selected stack frame index.
    pub selected_frame: usize,
    /// Scopes for the selected stack frame.
    pub scopes: Vec<DapScope>,
    /// Flattened variable tree for display.
    pub variables: Vec<VariableNode>,
    /// Currently selected variable index.
    pub selected_var: usize,
    /// Program output lines.
    pub output_lines: Vec<String>,
    /// Scroll offset for output view.
    pub output_scroll: usize,
}

impl Default for DebugState {
    fn default() -> Self {
        Self {
            status: SessionStatus::Inactive,
            stopped_file: None,
            stopped_line: None,
            stopped_thread_id: None,
            stack_frames: Vec::new(),
            selected_frame: 0,
            scopes: Vec::new(),
            variables: Vec::new(),
            selected_var: 0,
            output_lines: Vec::new(),
            output_scroll: 0,
        }
    }
}

impl DebugState {
    /// Reset all state for a new session.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Clear stopped location and frame data (when continuing).
    pub fn clear_stopped(&mut self) {
        self.stopped_file = None;
        self.stopped_line = None;
        self.stopped_thread_id = None;
        self.stack_frames.clear();
        self.selected_frame = 0;
        self.scopes.clear();
        self.variables.clear();
        self.selected_var = 0;
    }
}

/// The debug panel UI state.
#[derive(Debug)]
pub struct DebugPanel {
    /// Whether the debug panel is visible.
    pub visible: bool,
    /// Panel height in terminal rows.
    pub height: u16,
    /// Which sub-tab is active.
    pub active_tab: DebugTab,
    /// Debug session state.
    pub state: DebugState,
}

impl Default for DebugPanel {
    fn default() -> Self {
        Self {
            visible: false,
            height: 12,
            active_tab: DebugTab::CallStack,
            state: DebugState::default(),
        }
    }
}

impl DebugPanel {
    /// Create a new debug panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the debug panel.
    pub fn open(&mut self) {
        self.visible = true;
    }

    /// Hide the debug panel.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Toggle panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Navigate up in the current tab's list.
    pub fn select_up(&mut self) {
        match self.active_tab {
            DebugTab::CallStack => {
                if self.state.selected_frame > 0 {
                    self.state.selected_frame -= 1;
                }
            }
            DebugTab::Variables => {
                if self.state.selected_var > 0 {
                    self.state.selected_var -= 1;
                }
            }
            DebugTab::Output => {
                self.state.output_scroll = self.state.output_scroll.saturating_sub(1);
            }
        }
    }

    /// Navigate down in the current tab's list.
    pub fn select_down(&mut self) {
        match self.active_tab {
            DebugTab::CallStack => {
                let max = self.state.stack_frames.len().saturating_sub(1);
                if self.state.selected_frame < max {
                    self.state.selected_frame += 1;
                }
            }
            DebugTab::Variables => {
                let max = self.state.variables.len().saturating_sub(1);
                if self.state.selected_var < max {
                    self.state.selected_var += 1;
                }
            }
            DebugTab::Output => {
                let max = self.state.output_lines.len().saturating_sub(1);
                if self.state.output_scroll < max {
                    self.state.output_scroll += 1;
                }
            }
        }
    }
}
