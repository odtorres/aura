//! Authorship tracking for edits.
//!
//! Every edit in AURA carries an [`AuthorId`] so the editor knows whether
//! a change came from the human or from an AI agent. This is the foundation
//! for per-author undo and the CRDT layer.

use serde::{Deserialize, Serialize};

/// Identifies who made an edit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthorId {
    /// The local human user.
    Human,
    /// An AI agent, identified by name.
    Ai(String),
    /// A remote human peer in a collaborative session.
    Peer {
        /// Human-readable display name.
        name: String,
        /// Unique peer identifier (derived from automerge ActorId).
        peer_id: u64,
    },
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

    /// Create a remote peer author ID.
    pub fn peer(name: impl Into<String>, peer_id: u64) -> Self {
        Self::Peer {
            name: name.into(),
            peer_id,
        }
    }

    /// Returns true if this is the local human author.
    pub fn is_human(&self) -> bool {
        matches!(self, Self::Human)
    }

    /// Returns true if this is an AI agent author.
    pub fn is_ai(&self) -> bool {
        matches!(self, Self::Ai(_))
    }

    /// Returns true if this is a remote peer author.
    pub fn is_peer(&self) -> bool {
        matches!(self, Self::Peer { .. })
    }

    /// Human-readable name ("you", agent name, or peer name).
    pub fn display_name(&self) -> &str {
        match self {
            Self::Human => "you",
            Self::Ai(name) => name.as_str(),
            Self::Peer { name, .. } => name.as_str(),
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
    /// Cyan — for remote peers.
    Cyan,
    /// Magenta — for remote peers.
    Magenta,
    /// Orange — for remote peers.
    Orange,
    /// Teal — for remote peers.
    Teal,
}

impl AuthorColor {
    /// Assign a color to a peer based on their ID (rotating palette).
    pub fn for_peer(peer_id: u64) -> Self {
        const PEER_COLORS: &[AuthorColor] = &[
            AuthorColor::Cyan,
            AuthorColor::Magenta,
            AuthorColor::Orange,
            AuthorColor::Teal,
            AuthorColor::Purple,
            AuthorColor::Yellow,
        ];
        PEER_COLORS[(peer_id as usize) % PEER_COLORS.len()]
    }
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

    /// Create an Author representing a remote peer.
    pub fn peer(name: impl Into<String>, peer_id: u64) -> Self {
        Self {
            id: AuthorId::peer(name, peer_id),
            color: AuthorColor::for_peer(peer_id),
        }
    }
}
