//! AI Checkpoints — automatic snapshots before every AI edit with rollback.
//!
//! Creates a checkpoint before each AI operation. Users can view a timeline
//! of AI edits and roll back to any prior state.

use std::path::PathBuf;
use std::time::Instant;

/// A single checkpoint snapshot.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Unique checkpoint ID.
    pub id: usize,
    /// Description of what triggered this checkpoint.
    pub description: String,
    /// File path.
    pub file: PathBuf,
    /// Full file content at this checkpoint.
    pub content: String,
    /// Cursor position (row, col).
    pub cursor: (usize, usize),
    /// When the checkpoint was created.
    pub created: Instant,
}

/// Manages checkpoints for AI edit history.
pub struct CheckpointManager {
    /// All checkpoints, newest last.
    pub checkpoints: Vec<Checkpoint>,
    /// Next checkpoint ID.
    next_id: usize,
    /// Maximum checkpoints to retain.
    max_checkpoints: usize,
}

impl CheckpointManager {
    /// Create a new checkpoint manager.
    pub fn new() -> Self {
        Self {
            checkpoints: Vec::new(),
            next_id: 0,
            max_checkpoints: 100,
        }
    }

    /// Create a checkpoint before an AI edit.
    pub fn create(
        &mut self,
        description: &str,
        file: PathBuf,
        content: String,
        cursor: (usize, usize),
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.checkpoints.push(Checkpoint {
            id,
            description: description.to_string(),
            file,
            content,
            cursor,
            created: Instant::now(),
        });
        // Trim old checkpoints.
        while self.checkpoints.len() > self.max_checkpoints {
            self.checkpoints.remove(0);
        }
        id
    }

    /// Get a checkpoint by ID.
    pub fn get(&self, id: usize) -> Option<&Checkpoint> {
        self.checkpoints.iter().find(|c| c.id == id)
    }

    /// List all checkpoints for a file, newest first.
    pub fn for_file(&self, file: &std::path::Path) -> Vec<&Checkpoint> {
        let mut cps: Vec<&Checkpoint> =
            self.checkpoints.iter().filter(|c| c.file == file).collect();
        cps.reverse();
        cps
    }

    /// List all checkpoints, newest first.
    pub fn list(&self) -> Vec<&Checkpoint> {
        let mut cps: Vec<&Checkpoint> = self.checkpoints.iter().collect();
        cps.reverse();
        cps
    }

    /// Get the most recent checkpoint for a file.
    pub fn latest_for_file(&self, file: &std::path::Path) -> Option<&Checkpoint> {
        self.checkpoints.iter().rev().find(|c| c.file == file)
    }

    /// Format checkpoint age as human-readable string.
    pub fn age_display(cp: &Checkpoint) -> String {
        let secs = cp.created.elapsed().as_secs();
        if secs < 60 {
            format!("{}s ago", secs)
        } else if secs < 3600 {
            format!("{}m ago", secs / 60)
        } else {
            format!("{}h ago", secs / 3600)
        }
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_list() {
        let mut mgr = CheckpointManager::new();
        mgr.create(
            "AI edit 1",
            PathBuf::from("a.rs"),
            "content1".into(),
            (0, 0),
        );
        mgr.create(
            "AI edit 2",
            PathBuf::from("a.rs"),
            "content2".into(),
            (5, 0),
        );
        mgr.create(
            "AI edit 3",
            PathBuf::from("b.rs"),
            "content3".into(),
            (0, 0),
        );

        assert_eq!(mgr.list().len(), 3);
        assert_eq!(mgr.for_file(std::path::Path::new("a.rs")).len(), 2);
        assert_eq!(mgr.for_file(std::path::Path::new("b.rs")).len(), 1);
    }

    #[test]
    fn test_rollback() {
        let mut mgr = CheckpointManager::new();
        let id = mgr.create(
            "before",
            PathBuf::from("x.rs"),
            "old content".into(),
            (0, 0),
        );
        let cp = mgr.get(id).unwrap();
        assert_eq!(cp.content, "old content");
    }
}
