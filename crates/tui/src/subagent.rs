//! Subagent system — parallel specialized agents with isolated contexts.
//!
//! The main agent can spawn sub-agents via the `spawn_subagent` tool.
//! Each subagent runs in an isolated conversation context with its own
//! streaming receiver, tool restrictions, and iteration limits.

use aura_ai::{AiEvent, ContentBlock, Message};
use std::collections::HashMap;
use std::sync::mpsc;

use crate::agent_timeline::TimelineEntry;

/// Unique identifier for a subagent.
pub type SubagentId = String;

/// Role/specialization for a subagent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentRole {
    /// Read-only codebase exploration and analysis.
    Explorer,
    /// Test execution and verification.
    TestRunner,
    /// Focused code refactoring.
    Refactorer,
    /// Code review and feedback.
    Reviewer,
    /// Custom role with a description.
    Custom(String),
}

impl SubagentRole {
    /// Parse a role from a string.
    pub fn parse_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "explorer" => Self::Explorer,
            "test_runner" | "testrunner" => Self::TestRunner,
            "refactorer" => Self::Refactorer,
            "reviewer" => Self::Reviewer,
            other => Self::Custom(other.to_string()),
        }
    }

    /// Display label for this role.
    pub fn label(&self) -> &str {
        match self {
            Self::Explorer => "Explorer",
            Self::TestRunner => "TestRunner",
            Self::Refactorer => "Refactorer",
            Self::Reviewer => "Reviewer",
            Self::Custom(s) => s.as_str(),
        }
    }

    /// Build a role-specific system prompt.
    pub fn system_prompt(&self, task: &str) -> String {
        let role_context = match self {
            Self::Explorer => {
                "You are a codebase exploration agent. Your job is to READ and ANALYZE code \
                 to gather information. Do NOT modify any files. Use read_file, list_files, \
                 and search_files to explore the codebase. Provide a thorough summary of \
                 your findings."
            }
            Self::TestRunner => {
                "You are a test execution agent. Your job is to run tests and report results. \
                 Use run_command to execute tests and read_file to examine test output. \
                 Report which tests pass, which fail, and any error details."
            }
            Self::Refactorer => {
                "You are a code refactoring agent. Your job is to make focused code changes. \
                 Read the relevant files, make targeted edits using edit_file, and verify \
                 the changes compile. Be precise and minimal in your edits."
            }
            Self::Reviewer => {
                "You are a code review agent. Your job is to READ code and provide review \
                 feedback. Do NOT modify any files. Use read_file and search_files to examine \
                 the code. Provide specific, actionable feedback on code quality, bugs, and \
                 improvements."
            }
            Self::Custom(_) => {
                "You are a focused sub-agent. Complete your assigned task efficiently \
                 using the available tools."
            }
        };

        format!(
            "{role_context}\n\n\
             Your task: {task}\n\n\
             Work efficiently and provide a clear summary when done."
        )
    }

    /// Default tool restrictions for this role.
    pub fn default_tool_restrictions(&self) -> ToolRestrictions {
        match self {
            Self::Explorer | Self::Reviewer => ToolRestrictions {
                allowed_tools: vec![
                    "read_file".into(),
                    "list_files".into(),
                    "search_files".into(),
                ],
            },
            Self::TestRunner => ToolRestrictions {
                allowed_tools: vec![
                    "read_file".into(),
                    "list_files".into(),
                    "search_files".into(),
                    "run_command".into(),
                ],
            },
            Self::Refactorer | Self::Custom(_) => ToolRestrictions {
                allowed_tools: vec![], // all tools allowed
            },
        }
    }
}

/// Which tools a subagent is allowed to use.
#[derive(Debug, Clone)]
pub struct ToolRestrictions {
    /// Allowed tool names. Empty means all tools are allowed.
    pub allowed_tools: Vec<String>,
}

impl ToolRestrictions {
    /// Check whether a tool is allowed.
    pub fn allows(&self, tool_name: &str) -> bool {
        self.allowed_tools.is_empty() || self.allowed_tools.iter().any(|t| t == tool_name)
    }
}

/// Current status of a subagent.
#[derive(Debug, Clone)]
pub enum SubagentStatus {
    /// Actively streaming/executing.
    Running,
    /// Waiting for a tool result.
    WaitingTool,
    /// Successfully completed with a summary.
    Completed(String),
    /// Failed with an error message.
    Failed(String),
    /// Cancelled by the user or main agent.
    Cancelled,
}

impl SubagentStatus {
    /// Whether the subagent is still active (running or waiting).
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::WaitingTool)
    }

    /// Short label for display.
    pub fn label(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::WaitingTool => "waiting",
            Self::Completed(_) => "done",
            Self::Failed(_) => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// A pending tool call within a subagent context.
#[derive(Debug, Clone)]
pub struct SubagentToolCall {
    /// Tool use ID from the API.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Tool input parameters.
    pub input: serde_json::Value,
}

/// A running subagent instance with isolated conversation context.
pub struct Subagent {
    /// Unique identifier.
    pub id: SubagentId,
    /// Specialization role.
    pub role: SubagentRole,
    /// Task description.
    pub task: String,
    /// System prompt used for this subagent.
    pub system_prompt: String,
    /// Current status.
    pub status: SubagentStatus,
    /// Tool access restrictions.
    pub tool_restrictions: ToolRestrictions,

    // Isolated conversation state.
    /// Message history for this subagent's conversation.
    pub context_messages: Vec<Message>,
    /// Content blocks from the current assistant turn.
    pub current_assistant_blocks: Vec<ContentBlock>,
    /// Tools pending execution.
    pub pending_tool_calls: Vec<SubagentToolCall>,

    // Progress tracking.
    /// Actions performed by this subagent.
    pub actions: Vec<TimelineEntry>,
    /// Current iteration count.
    pub iteration: usize,
    /// Maximum iterations before stopping.
    pub max_iterations: usize,

    // Communication.
    /// Receiver for streaming AI events.
    pub receiver: Option<mpsc::Receiver<AiEvent>>,
    /// Accumulated streaming text.
    pub streaming_text: String,
}

impl Subagent {
    /// Create a new subagent.
    pub fn new(
        id: SubagentId,
        role: SubagentRole,
        task: String,
        tool_restrictions: ToolRestrictions,
        max_iterations: usize,
    ) -> Self {
        let system_prompt = role.system_prompt(&task);
        Self {
            id,
            role,
            task,
            system_prompt,
            status: SubagentStatus::Running,
            tool_restrictions,
            context_messages: Vec::new(),
            current_assistant_blocks: Vec::new(),
            pending_tool_calls: Vec::new(),
            actions: Vec::new(),
            iteration: 0,
            max_iterations,
            receiver: None,
            streaming_text: String::new(),
        }
    }

    /// Get the final result summary (if completed).
    pub fn result_summary(&self) -> Option<&str> {
        match &self.status {
            SubagentStatus::Completed(summary) => Some(summary.as_str()),
            _ => None,
        }
    }
}

/// Orchestrator managing all subagents.
pub struct SubagentManager {
    /// Active and completed subagents.
    pub subagents: HashMap<SubagentId, Subagent>,
    /// Maximum number of concurrent subagents.
    pub max_concurrent: usize,
}

impl Default for SubagentManager {
    fn default() -> Self {
        Self {
            subagents: HashMap::new(),
            max_concurrent: 3,
        }
    }
}

impl SubagentManager {
    /// Number of currently active (running/waiting) subagents.
    pub fn active_count(&self) -> usize {
        self.subagents
            .values()
            .filter(|s| s.status.is_active())
            .count()
    }

    /// Whether we can spawn another subagent.
    pub fn can_spawn(&self) -> bool {
        self.active_count() < self.max_concurrent
    }

    /// Register a new subagent. Returns false if at capacity.
    pub fn add(&mut self, subagent: Subagent) -> bool {
        if !self.can_spawn() {
            return false;
        }
        self.subagents.insert(subagent.id.clone(), subagent);
        true
    }

    /// Cancel a subagent by ID.
    pub fn cancel(&mut self, id: &str) -> bool {
        if let Some(agent) = self.subagents.get_mut(id) {
            agent.status = SubagentStatus::Cancelled;
            agent.receiver = None;
            true
        } else {
            false
        }
    }

    /// Cancel all active subagents.
    pub fn cancel_all(&mut self) {
        for agent in self.subagents.values_mut() {
            if agent.status.is_active() {
                agent.status = SubagentStatus::Cancelled;
                agent.receiver = None;
            }
        }
    }

    /// Get a subagent by ID.
    pub fn get(&self, id: &str) -> Option<&Subagent> {
        self.subagents.get(id)
    }

    /// Get a mutable reference to a subagent.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Subagent> {
        self.subagents.get_mut(id)
    }

    /// Return IDs of all active subagents.
    pub fn active_ids(&self) -> Vec<SubagentId> {
        self.subagents
            .iter()
            .filter(|(_, s)| s.status.is_active())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Return summaries of completed subagents.
    pub fn completed_results(&self) -> Vec<(&str, &str)> {
        self.subagents
            .iter()
            .filter_map(|(id, s)| s.result_summary().map(|summary| (id.as_str(), summary)))
            .collect()
    }

    /// Total number of subagents (active + completed + cancelled).
    pub fn total_count(&self) -> usize {
        self.subagents.len()
    }
}
