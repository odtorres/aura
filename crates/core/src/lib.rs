//! aura-core: Fundamental data structures for the AURA editor.
//!
//! This crate provides the text buffer (rope-based), authorship tracking,
//! cursor/selection management, and will eventually house the CRDT layer.

pub mod author;
pub mod buffer;
pub mod crdt;
pub mod cursor;
pub mod semantic;

pub use author::{Author, AuthorColor, AuthorId};
pub use buffer::Buffer;
pub use crdt::CrdtDoc;
pub use cursor::Cursor;
pub use semantic::SemanticGraph;
