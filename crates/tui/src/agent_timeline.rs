//! Agent activity timeline — tracks all actions during an agent session.
//!
//! Every tool execution, file change, subagent event, and error is recorded
//! as a `TimelineEntry`. The timeline is viewable via `:agent timeline` and
//! is shown in the chat panel area during agent mode.

/// Type of action recorded in the timeline.
#[derive(Debug, Clone)]
pub enum TimelineActionType {
    /// Agent created an execution plan.
    PlanCreated,
    /// A tool was executed.
    ToolExecuted {
        /// Tool name.
        name: String,
        /// Whether it succeeded.
        success: bool,
    },
    /// A file was changed.
    FileChanged {
        /// File path.
        path: String,
    },
    /// A shell command was run.
    CommandRun {
        /// The command string.
        command: String,
        /// Exit status (0 = success).
        exit_code: i32,
    },
    /// A subagent was spawned.
    SubagentSpawned {
        /// Subagent role description.
        role: String,
    },
    /// A subagent completed its task.
    SubagentCompleted {
        /// Summary of what it did.
        summary: String,
    },
    /// A checkpoint (e.g., plan step completed).
    Checkpoint {
        /// Step description.
        message: String,
    },
    /// An error occurred.
    Error(String),
}

/// A single entry in the agent activity timeline.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    /// When this action occurred.
    pub timestamp: std::time::Instant,
    /// Which agent performed the action (None = main agent).
    pub agent_id: Option<String>,
    /// What kind of action.
    pub action_type: TimelineActionType,
    /// Human-readable description.
    pub description: String,
}

impl TimelineEntry {
    /// Create a new timeline entry for the main agent.
    pub fn new(action_type: TimelineActionType, description: &str) -> Self {
        Self {
            timestamp: std::time::Instant::now(),
            agent_id: None,
            action_type,
            description: description.to_string(),
        }
    }

    /// Create a new timeline entry for a subagent.
    pub fn for_subagent(
        agent_id: &str,
        action_type: TimelineActionType,
        description: &str,
    ) -> Self {
        Self {
            timestamp: std::time::Instant::now(),
            agent_id: Some(agent_id.to_string()),
            action_type,
            description: description.to_string(),
        }
    }

    /// Short label for the action type (for compact display).
    pub fn action_label(&self) -> &str {
        match &self.action_type {
            TimelineActionType::PlanCreated => "PLAN",
            TimelineActionType::ToolExecuted { .. } => "TOOL",
            TimelineActionType::FileChanged { .. } => "FILE",
            TimelineActionType::CommandRun { .. } => "CMD",
            TimelineActionType::SubagentSpawned { .. } => "SPAWN",
            TimelineActionType::SubagentCompleted { .. } => "DONE",
            TimelineActionType::Checkpoint { .. } => "CHECK",
            TimelineActionType::Error(_) => "ERROR",
        }
    }
}

/// Filter for timeline display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineFilter {
    /// Show all entries.
    All,
    /// Show only main agent entries.
    MainAgentOnly,
    /// Show only a specific subagent's entries.
    SubagentOnly(String),
    /// Show only tool executions.
    ToolsOnly,
    /// Show only errors.
    ErrorsOnly,
}

/// Scrollable timeline panel state.
#[derive(Debug, Clone)]
pub struct AgentTimeline {
    /// All recorded entries.
    pub entries: Vec<TimelineEntry>,
    /// Current scroll offset.
    pub scroll: usize,
    /// Active filter.
    pub filter: TimelineFilter,
    /// Whether the timeline overlay is visible.
    pub visible: bool,
}

impl Default for AgentTimeline {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            scroll: 0,
            filter: TimelineFilter::All,
            visible: false,
        }
    }
}

impl AgentTimeline {
    /// Add a timeline entry.
    pub fn add(&mut self, entry: TimelineEntry) {
        self.entries.push(entry);
    }

    /// Add a tool execution entry.
    pub fn record_tool(&mut self, name: &str, success: bool) {
        let desc = if success {
            format!("✓ {name}")
        } else {
            format!("✗ {name} (failed)")
        };
        self.add(TimelineEntry::new(
            TimelineActionType::ToolExecuted {
                name: name.to_string(),
                success,
            },
            &desc,
        ));
    }

    /// Add a file-changed entry.
    pub fn record_file_change(&mut self, path: &str) {
        self.add(TimelineEntry::new(
            TimelineActionType::FileChanged {
                path: path.to_string(),
            },
            &format!("Modified {path}"),
        ));
    }

    /// Add a command-run entry.
    pub fn record_command(&mut self, command: &str, exit_code: i32) {
        let desc = if exit_code == 0 {
            format!("$ {command}")
        } else {
            format!("$ {command} (exit {exit_code})")
        };
        self.add(TimelineEntry::new(
            TimelineActionType::CommandRun {
                command: command.to_string(),
                exit_code,
            },
            &desc,
        ));
    }

    /// Add an error entry.
    pub fn record_error(&mut self, message: &str) {
        self.add(TimelineEntry::new(
            TimelineActionType::Error(message.to_string()),
            message,
        ));
    }

    /// Return filtered entries based on the current filter.
    pub fn filtered_entries(&self) -> Vec<&TimelineEntry> {
        self.entries
            .iter()
            .filter(|e| match &self.filter {
                TimelineFilter::All => true,
                TimelineFilter::MainAgentOnly => e.agent_id.is_none(),
                TimelineFilter::SubagentOnly(id) => e.agent_id.as_deref() == Some(id.as_str()),
                TimelineFilter::ToolsOnly => {
                    matches!(e.action_type, TimelineActionType::ToolExecuted { .. })
                }
                TimelineFilter::ErrorsOnly => {
                    matches!(e.action_type, TimelineActionType::Error(_))
                }
            })
            .collect()
    }

    /// Scroll up by one line.
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Scroll down by one line.
    pub fn scroll_down(&mut self) {
        let max = self.filtered_entries().len().saturating_sub(1);
        if self.scroll < max {
            self.scroll += 1;
        }
    }

    /// Total entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the timeline is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
