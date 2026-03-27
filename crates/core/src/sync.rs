//! Collaborative sync primitives.
//!
//! Re-exports automerge's sync types and provides per-peer sync state
//! tracking for real-time collaborative editing.

pub use automerge::sync::Message as SyncMessage;
pub use automerge::sync::State as SyncState;

use crate::author::AuthorId;

/// Tracks the sync state for a single remote peer.
pub struct PeerSyncState {
    /// Unique identifier for this peer.
    pub peer_id: u64,
    /// Human-readable display name.
    pub peer_name: String,
    /// Automerge sync state for incremental sync with this peer.
    pub sync_state: SyncState,
    /// The AuthorId used for edits from this peer.
    pub author_id: AuthorId,
}

impl PeerSyncState {
    /// Create a new peer sync state.
    pub fn new(peer_id: u64, peer_name: impl Into<String>) -> Self {
        let name = peer_name.into();
        Self {
            peer_id,
            peer_name: name.clone(),
            sync_state: SyncState::new(),
            author_id: AuthorId::peer(name, peer_id),
        }
    }
}
