//! CRDT layer using automerge for conflict-free multi-author editing.
//!
//! This module wraps an automerge `AutoCommit` document and provides
//! text-editing operations that map to the rope buffer. Each [`AuthorId`]
//! is mapped to an automerge `ActorId`, ensuring change attribution is
//! preserved in the CRDT history.

use automerge::sync::SyncDoc;
use automerge::{transaction::Transactable, AutoCommit, ObjType, ReadDoc, ROOT};
use std::collections::HashMap;

use crate::author::AuthorId;

/// Wraps an automerge document for collaborative text editing.
///
/// The CRDT document mirrors the rope buffer contents. Each edit is
/// performed with the actor set to the corresponding author, so
/// automerge's change history carries full provenance information.
pub struct CrdtDoc {
    /// The automerge document.
    doc: AutoCommit,
    /// The object ID of the text object inside the document.
    text_id: automerge::ObjId,
    /// Mapping from AuthorId to automerge ActorId bytes.
    actor_map: HashMap<AuthorId, Vec<u8>>,
    /// The current active actor (to avoid unnecessary switches).
    current_actor: Option<AuthorId>,
}

impl CrdtDoc {
    /// Create a new CRDT document with an empty text object.
    pub fn new() -> Self {
        let mut doc = AutoCommit::new();
        let text_id = doc
            .put_object(ROOT, "text", ObjType::Text)
            .expect("failed to create text object");
        Self {
            doc,
            text_id,
            actor_map: HashMap::new(),
            current_actor: None,
        }
    }

    /// Create a CRDT document pre-loaded with text content.
    pub fn with_text(content: &str) -> Self {
        let mut crdt = Self::new();
        if !content.is_empty() {
            crdt.splice(0, 0, content, &AuthorId::human());
        }
        crdt
    }

    /// Ensure the document's actor is set to the given author.
    fn set_actor(&mut self, author: &AuthorId) {
        if self.current_actor.as_ref() == Some(author) {
            return;
        }

        let actor_bytes = self
            .actor_map
            .entry(author.clone())
            .or_insert_with(|| {
                // Generate a deterministic actor ID from the author.
                let label = match author {
                    AuthorId::Human => "human-0".to_string(),
                    AuthorId::Ai(name) => format!("ai-{name}"),
                    AuthorId::Peer { peer_id, .. } => format!("peer-{peer_id}"),
                };
                // Pad/hash to 16 bytes for automerge ActorId.
                let mut bytes = [0u8; 16];
                for (i, b) in label.bytes().enumerate() {
                    if i >= 16 {
                        break;
                    }
                    bytes[i] = b;
                }
                bytes.to_vec()
            })
            .clone();

        self.doc
            .set_actor(automerge::ActorId::from(actor_bytes.as_slice()));
        self.current_actor = Some(author.clone());
    }

    /// Splice text in the CRDT document (insert and/or delete).
    ///
    /// This mirrors `Rope::insert` + `Rope::remove` in a single operation.
    /// - `pos`: character position
    /// - `del`: number of characters to delete starting at `pos`
    /// - `insert`: text to insert at `pos`
    pub fn splice(&mut self, pos: usize, del: usize, insert: &str, author: &AuthorId) {
        self.set_actor(author);
        self.doc
            .splice_text(&self.text_id, pos, del as isize, insert)
            .expect("CRDT splice failed");
    }

    /// Get the current text content from the CRDT document.
    pub fn text(&self) -> String {
        self.doc.text(&self.text_id).expect("failed to read text")
    }

    /// Get the number of changes in the document history.
    pub fn change_count(&mut self) -> usize {
        self.doc.get_changes(&[]).len()
    }

    /// Get a reference to the inner automerge document.
    pub fn doc(&self) -> &AutoCommit {
        &self.doc
    }

    /// Get a mutable reference to the inner automerge document.
    pub fn doc_mut(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }

    // ----- Sync methods for collaborative editing -----

    /// Get the current change heads (used for tracking sync progress).
    pub fn get_heads(&mut self) -> Vec<automerge::ChangeHash> {
        self.doc.get_heads()
    }

    /// Generate a sync message to send to a remote peer.
    ///
    /// Returns `None` if there is nothing new to send (the peer is up to date
    /// or we are waiting for an acknowledgement).
    pub fn generate_sync_message(
        &mut self,
        sync_state: &mut crate::sync::SyncState,
    ) -> Option<crate::sync::SyncMessage> {
        self.doc.sync().generate_sync_message(sync_state)
    }

    /// Apply a sync message received from a remote peer.
    pub fn receive_sync_message(
        &mut self,
        sync_state: &mut crate::sync::SyncState,
        msg: crate::sync::SyncMessage,
    ) -> Result<(), automerge::AutomergeError> {
        self.doc.sync().receive_sync_message(sync_state, msg)
    }

    /// Serialize the full document to bytes (for snapshots / initial sync).
    pub fn save_bytes(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// Load a document from serialized bytes.
    pub fn load_bytes(bytes: &[u8]) -> Result<Self, automerge::AutomergeError> {
        let doc = AutoCommit::load(bytes)?;
        let text_id = match doc.get(ROOT, "text")? {
            Some((_, id)) => id,
            None => {
                return Err(automerge::AutomergeError::InvalidObjId(
                    "missing text object".to_string(),
                ))
            }
        };
        Ok(Self {
            doc,
            text_id,
            actor_map: HashMap::new(),
            current_actor: None,
        })
    }

    /// Fork the document (creates an independent copy sharing the same history).
    ///
    /// Useful for creating a snapshot to send to a newly connected peer.
    pub fn fork(&mut self) -> Self {
        let forked = self.doc.fork();
        // The forked doc shares the same object IDs.
        let text_id = self.text_id.clone();
        Self {
            doc: forked,
            text_id,
            actor_map: HashMap::new(),
            current_actor: None,
        }
    }

    /// Merge another document's changes into this one.
    pub fn merge(
        &mut self,
        other: &mut AutoCommit,
    ) -> Result<Vec<automerge::ChangeHash>, automerge::AutomergeError> {
        self.doc.merge(other)
    }

    /// Compact the CRDT history by saving and reloading the document.
    ///
    /// This reduces memory usage by collapsing the change history into
    /// a single compacted state. Call after saving the file.
    pub fn compact(&mut self) {
        let bytes = self.doc.save();
        if let Ok(loaded) = AutoCommit::load(&bytes) {
            // Re-resolve the text object ID.
            if let Ok(Some((_, text_id))) = loaded.get(ROOT, "text") {
                self.text_id = text_id;
                self.doc = loaded;
                self.current_actor = None; // Reset actor after reload.
                tracing::debug!("CRDT history compacted ({} bytes)", bytes.len());
            }
        }
    }
}

impl Default for CrdtDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::author::AuthorId;

    #[test]
    fn test_crdt_insert() {
        let mut crdt = CrdtDoc::new();
        crdt.splice(0, 0, "hello", &AuthorId::human());
        assert_eq!(crdt.text(), "hello");
    }

    #[test]
    fn test_crdt_multi_author() {
        let mut crdt = CrdtDoc::new();
        crdt.splice(0, 0, "hello ", &AuthorId::human());
        crdt.splice(6, 0, "world", &AuthorId::ai("agent-1"));
        assert_eq!(crdt.text(), "hello world");
    }

    #[test]
    fn test_crdt_delete() {
        let mut crdt = CrdtDoc::new();
        crdt.splice(0, 0, "hello world", &AuthorId::human());
        crdt.splice(5, 6, "", &AuthorId::human());
        assert_eq!(crdt.text(), "hello");
    }

    #[test]
    fn test_crdt_with_text() {
        let crdt = CrdtDoc::with_text("pre-loaded content");
        assert_eq!(crdt.text(), "pre-loaded content");
    }

    #[test]
    fn test_sync_two_docs_one_direction() {
        // Doc A makes edits, syncs to Doc B.
        let mut doc_a = CrdtDoc::with_text("hello");
        let mut doc_b = doc_a.fork();

        let mut state_a = crate::sync::SyncState::new();
        let mut state_b = crate::sync::SyncState::new();

        // A makes an edit.
        doc_a.splice(5, 0, " world", &AuthorId::human());
        assert_eq!(doc_a.text(), "hello world");
        assert_eq!(doc_b.text(), "hello");

        // Sync: A → B.
        loop {
            let msg = doc_a.generate_sync_message(&mut state_a);
            if let Some(m) = msg {
                doc_b.receive_sync_message(&mut state_b, m).unwrap();
            }
            let msg = doc_b.generate_sync_message(&mut state_b);
            if let Some(m) = msg {
                doc_a.receive_sync_message(&mut state_a, m).unwrap();
            } else {
                break;
            }
        }

        assert_eq!(doc_b.text(), "hello world");
    }

    #[test]
    fn test_sync_bidirectional_concurrent_edits() {
        // Both docs start from the same content, make concurrent edits, then sync.
        let mut doc_a = CrdtDoc::with_text("hello");
        let mut doc_b = doc_a.fork();

        // Concurrent edits.
        doc_a.splice(5, 0, " world", &AuthorId::human());
        doc_b.splice(0, 0, "oh ", &AuthorId::peer("bob", 42));

        // Sync until both converge.
        let mut state_a = crate::sync::SyncState::new();
        let mut state_b = crate::sync::SyncState::new();

        for _ in 0..10 {
            if let Some(m) = doc_a.generate_sync_message(&mut state_a) {
                doc_b.receive_sync_message(&mut state_b, m).unwrap();
            }
            if let Some(m) = doc_b.generate_sync_message(&mut state_b) {
                doc_a.receive_sync_message(&mut state_a, m).unwrap();
            }
        }

        // Both should converge to the same text.
        assert_eq!(doc_a.text(), doc_b.text());
        // The result should contain both edits.
        let text = doc_a.text();
        assert!(text.contains("world"), "missing 'world' in: {text}");
        assert!(text.contains("oh"), "missing 'oh' in: {text}");
    }

    #[test]
    fn test_save_load_roundtrip() {
        let mut doc = CrdtDoc::with_text("test content");
        doc.splice(12, 0, " here", &AuthorId::ai("agent"));

        let bytes = doc.save_bytes();
        let loaded = CrdtDoc::load_bytes(&bytes).unwrap();
        assert_eq!(loaded.text(), "test content here");
    }

    #[test]
    fn test_fork_produces_independent_copy() {
        let mut doc = CrdtDoc::with_text("original");
        let mut forked = doc.fork();

        doc.splice(8, 0, " modified", &AuthorId::human());
        assert_eq!(doc.text(), "original modified");
        assert_eq!(forked.text(), "original");

        forked.splice(0, 0, "the ", &AuthorId::peer("alice", 1));
        assert_eq!(forked.text(), "the original");
        assert_eq!(doc.text(), "original modified");
    }
}
