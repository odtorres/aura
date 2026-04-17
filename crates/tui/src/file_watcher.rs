//! Filesystem watcher that replaces the editor's 2 s mtime polling loop.
//!
//! The watcher runs in its own OS thread (via `notify`) and delivers change
//! notifications for registered paths onto an `mpsc` channel. The main event
//! loop drains the channel once per tick. Events are debounced inside
//! [`FileWatcher::drain`] so bursts from atomic-save patterns (write-then-rename)
//! collapse into a single notification per path per debounce window.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

/// Minimum time between two delivered events for the same path. Coalesces the
/// write-then-rename bursts that atomic-save editors produce on macOS + Linux.
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(200);

/// A filesystem watcher that buffers change events for registered paths.
///
/// Failure to construct the watcher or register a path is not fatal — the
/// editor falls back to its previous mtime-poll behavior in that case. Callers
/// should treat `None` from [`FileWatcher::new`] as "watching disabled".
pub struct FileWatcher {
    // Kept alive so the notify thread keeps running.
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<Event>>,
    /// Paths currently registered for individual watching. notify requires we
    /// ask for each file explicitly; we keep the set so we can unwatch cleanly.
    watched: HashSet<PathBuf>,
    /// Last delivery time per path, used for debounce coalescing.
    last_delivered: HashMap<PathBuf, Instant>,
}

impl FileWatcher {
    /// Create a new watcher. Returns `None` if the OS backend fails to start,
    /// in which case callers should skip file watching entirely.
    pub fn new() -> Option<Self> {
        let (tx, rx) = channel();
        let watcher = notify::recommended_watcher(move |res| {
            // If the receiver is dropped the app is shutting down; ignore.
            let _ = tx.send(res);
        })
        .ok()?;

        Some(Self {
            _watcher: watcher,
            rx,
            watched: HashSet::new(),
            last_delivered: HashMap::new(),
        })
    }

    /// Begin watching `path`. Safe to call repeatedly for the same path —
    /// subsequent calls are no-ops so callers can eagerly register on every
    /// tab open without tracking membership themselves.
    pub fn watch(&mut self, path: &Path) {
        let canon = path.to_path_buf();
        if self.watched.contains(&canon) {
            return;
        }
        if self
            ._watcher
            .watch(path, RecursiveMode::NonRecursive)
            .is_ok()
        {
            self.watched.insert(canon);
        }
    }

    /// Stop watching `path`. No-op if not currently watched.
    pub fn unwatch(&mut self, path: &Path) {
        if self.watched.remove(path) {
            let _ = self._watcher.unwatch(path);
        }
    }

    /// Drain all pending filesystem events and return the set of paths that
    /// changed since the last drain. Events within the debounce window of a
    /// previous delivery for the same path are dropped.
    pub fn drain(&mut self) -> Vec<PathBuf> {
        let mut changed: HashSet<PathBuf> = HashSet::new();
        while let Ok(res) = self.rx.try_recv() {
            let Ok(event) = res else { continue };
            if !Self::is_content_event(&event.kind) {
                continue;
            }
            for p in event.paths {
                changed.insert(p);
            }
        }

        let now = Instant::now();
        changed
            .into_iter()
            .filter(|path| match self.last_delivered.get(path) {
                Some(last) if now.duration_since(*last) < DEBOUNCE_WINDOW => false,
                _ => {
                    self.last_delivered.insert(path.clone(), now);
                    true
                }
            })
            .collect()
    }

    fn is_content_event(kind: &EventKind) -> bool {
        matches!(
            kind,
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
        )
    }
}
