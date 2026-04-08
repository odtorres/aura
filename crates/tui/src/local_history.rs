//! Local file history — auto-snapshot on every save, independent of git.
//!
//! Stores snapshots in ~/.aura/history/<hash>/ with timestamps.
//! Users can diff and restore any prior version.

use std::path::{Path, PathBuf};

/// A snapshot of a file at a point in time.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Original file path.
    pub file: PathBuf,
    /// Snapshot path on disk.
    pub snapshot_path: PathBuf,
    /// Unix timestamp.
    pub timestamp: u64,
    /// File size in bytes.
    pub size: usize,
}

/// Manage local file history.
pub struct LocalHistory {
    /// Base directory for history storage.
    base_dir: PathBuf,
}

impl LocalHistory {
    /// Create a new local history manager.
    pub fn new() -> Self {
        let base = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".aura/history");
        Self { base_dir: base }
    }

    /// Save a snapshot of a file.
    pub fn save_snapshot(&self, file: &Path) -> anyhow::Result<()> {
        let content = std::fs::read(file)?;
        let hash = simple_hash(file);
        let dir = self.base_dir.join(&hash);
        std::fs::create_dir_all(&dir)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let snapshot_name = format!("{}.snap", now);
        let snapshot_path = dir.join(snapshot_name);
        std::fs::write(&snapshot_path, &content)?;

        // Keep only the last 50 snapshots.
        self.prune(&dir, 50);
        Ok(())
    }

    /// List snapshots for a file, newest first.
    pub fn list(&self, file: &Path) -> Vec<HistoryEntry> {
        let hash = simple_hash(file);
        let dir = self.base_dir.join(&hash);
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut snapshots: Vec<HistoryEntry> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let name = path.file_stem()?.to_str()?;
                let ts: u64 = name.parse().ok()?;
                let size = std::fs::metadata(&path).ok()?.len() as usize;
                Some(HistoryEntry {
                    file: file.to_path_buf(),
                    snapshot_path: path,
                    timestamp: ts,
                    size,
                })
            })
            .collect();

        snapshots.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        snapshots
    }

    /// Read a snapshot's content.
    pub fn read_snapshot(&self, entry: &HistoryEntry) -> anyhow::Result<String> {
        Ok(std::fs::read_to_string(&entry.snapshot_path)?)
    }

    /// Format a timestamp as relative time.
    pub fn time_ago(ts: u64) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let diff = now.saturating_sub(ts);
        if diff < 60 {
            format!("{}s ago", diff)
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else {
            format!("{}d ago", diff / 86400)
        }
    }

    /// Remove old snapshots beyond the limit.
    fn prune(&self, dir: &Path, max: usize) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        files.sort();
        while files.len() > max {
            if let Some(oldest) = files.first() {
                let _ = std::fs::remove_file(oldest);
            }
            files.remove(0);
        }
    }
}

impl Default for LocalHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple hash of a file path for directory naming.
fn simple_hash(path: &Path) -> String {
    let s = path.display().to_string();
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_hash() {
        let h1 = simple_hash(Path::new("/tmp/a.rs"));
        let h2 = simple_hash(Path::new("/tmp/b.rs"));
        assert_ne!(h1, h2);
        assert_eq!(h1, simple_hash(Path::new("/tmp/a.rs")));
    }
}
