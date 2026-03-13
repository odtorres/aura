//! CRDT layer using automerge for conflict-free multi-author editing.
//!
//! This module wraps an automerge `AutoCommit` document and provides
//! text-editing operations that map to the rope buffer. Each [`AuthorId`]
//! is mapped to an automerge `ActorId`, ensuring change attribution is
//! preserved in the CRDT history.

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
}
