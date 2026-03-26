//! Authorship tracking for edits.
//!
//! Every edit in AURA carries an [`AuthorId`] so the editor knows whether
//! a change came from the human or from an AI agent. This is the foundation
//! for per-author undo and the CRDT layer.

use serde::{Deserialize, Serialize};

/// Identifies who made an edit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthorId {
    /// The human user.
    Human,
    /// An AI agent, identified by name.
    Ai(String),
}

impl AuthorId {
    /// Create a human author ID.
    pub fn human() -> Self {
        Self::Human
    }

    /// Create an AI agent author ID.
    pub fn ai(name: impl Into<String>) -> Self {
        Self::Ai(name.into())
    }

    /// Returns true if this is the human author.
    pub fn is_human(&self) -> bool {
        matches!(self, Self::Human)
    }

    /// Returns true if this is an AI agent author.
    pub fn is_ai(&self) -> bool {
        matches!(self, Self::Ai(_))
    }

    /// Human-readable name ("you" or the agent name).
    pub fn display_name(&self) -> &str {
        match self {
            Self::Human => "you",
            Self::Ai(name) => name.as_str(),
        }
    }
}

impl std::fmt::Display for AuthorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Metadata about an author for display in the UI.
#[derive(Debug, Clone)]
pub struct Author {
    /// The underlying author identifier.
    pub id: AuthorId,
    /// Color hint for the TUI (e.g., gutter markers).
    pub color: AuthorColor,
}

/// Simple color enum — the TUI layer maps these to actual terminal colors.
#[derive(Debug, Clone, Copy)]
pub enum AuthorColor {
    /// Green — default for human authors.
    Green,
    /// Blue — default for AI authors.
    Blue,
    /// Purple — for additional agents.
    Purple,
    /// Yellow — extra author slot.
    Yellow,
}

impl Author {
    /// Create an Author representing the human user.
    pub fn human() -> Self {
        Self {
            id: AuthorId::Human,
            color: AuthorColor::Green,
        }
    }

    /// Create an Author representing an AI agent.
    pub fn ai(name: impl Into<String>) -> Self {
        Self {
            id: AuthorId::ai(name),
            color: AuthorColor::Blue,
        }
    }
}
