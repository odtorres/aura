//! Async worker thread for slow git CLI operations.
//!
//! AURA's git integration historically called `std::process::Command::output()`
//! synchronously on the main event loop for `git status`, `git rev-list`,
//! `git stash list`, and `git commit`. On large repos, slow filesystems, or
//! repos with `commit.gpgsign=true` / `pre-commit` hooks expecting a TTY,
//! these calls would freeze the UI for seconds — sometimes indefinitely.
//!
//! This module owns a single long-lived worker thread that drains commands
//! from an mpsc channel, runs each `git` subprocess with a hard wall-clock
//! timeout, and emits events that the main loop drains into the
//! [`crate::source_control::SourceControlPanel`] state machine.
//!
//! The pattern (mpsc command + event channels, one named worker thread)
//! mirrors `lsp.rs` and `collab.rs` — the same idiom the rest of the
//! codebase uses for I/O-bound work.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::git::GitStatusEntry;

/// Default timeout for read-only refresh operations.
///
/// Bumped from 8 s to 30 s after we observed `git status` exceeding 8 s
/// on slow filesystems (network mounts, large worktrees, cold caches),
/// causing the worker to SIGKILL git mid-write and orphan
/// `.git/index.lock`.
pub const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);
/// Hard timeout for `git commit` — picked so a normal signing setup
/// (gpg-agent, pinentry-loopback) can still complete, but a hung
/// interactive prompt is killed before the user gives up.
pub const COMMIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Grace period given to `git` after SIGTERM before we escalate to SIGKILL.
/// 500 ms is enough for git to remove its `index.lock` on a normal exit
/// without making the user wait noticeably longer on a hung process.
const SIGTERM_GRACE: Duration = Duration::from_millis(500);

/// Process-wide mutex serializing every `git` subprocess invocation that
/// originates from Aura. Both the worker thread and the synchronous
/// `GitRepo` write paths take this lock, so we never have two Aura-owned
/// `git` processes racing on `.git/index.lock` at the same time.
///
/// External `git` invocations (e.g., the user typing `git add` in their
/// shell) are obviously not covered — those are handled by the
/// `index.lock`-aware deferral in `app::tick_source_control`.
pub fn git_cli_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Run a `git` subprocess command, serialized with every other Aura git
/// invocation via [`git_cli_lock`]. Mirrors `Command::output()`.
pub fn run_git_serialized(cmd: &mut Command) -> std::io::Result<Output> {
    let _guard = git_cli_lock().lock().unwrap_or_else(|e| e.into_inner());
    cmd.output()
}

/// Which kind of git refresh produced an event — used by the panel to
/// match an in-flight request to a result, and for status-line indicators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitRefreshKind {
    /// `git status --porcelain=v1`
    Status,
    /// `git rev-list --left-right --count HEAD...@{upstream}`
    BranchInfo,
    /// `git stash list`
    Stashes,
    /// `git commit -m <msg>`
    Commit,
}

/// Commands sent from the main thread to the worker.
#[derive(Debug, Clone)]
pub enum GitCommand {
    /// Run `git status --porcelain=v1`.
    RefreshStatus,
    /// Run `git rev-list --left-right --count HEAD...@{upstream}`.
    RefreshBranchInfo,
    /// Run `git stash list --format=%gd|%s`.
    RefreshStashes,
    /// Run `git commit -m <message>`. `stdin` is wired to `/dev/null` so
    /// pre-commit hooks expecting a TTY fail fast instead of hanging.
    Commit {
        /// Commit message body.
        message: String,
    },
    /// Stop the worker thread and join.
    Shutdown,
}

/// Events emitted by the worker for the main loop to drain.
#[derive(Debug, Clone)]
pub enum GitEvent {
    /// `RefreshStatus` returned successfully.
    StatusReady(Vec<GitStatusEntry>),
    /// `RefreshBranchInfo` returned successfully.
    BranchInfoReady {
        /// Commits on local that aren't on upstream.
        ahead: usize,
        /// Commits on upstream that aren't on local.
        behind: usize,
    },
    /// `RefreshStashes` returned successfully — list of `(name, message)`.
    StashesReady(Vec<(String, String)>),
    /// `Commit` finished successfully — payload is the new commit's short
    /// hash if we could read it, or `"OK"` otherwise.
    CommitDone(Result<String, String>),
    /// A command timed out and the child process was killed.
    Timeout(GitRefreshKind),
    /// A command failed with a non-timeout error.
    Failed {
        /// Which command produced the failure.
        kind: GitRefreshKind,
        /// Human-readable error message.
        message: String,
    },
    /// Yellow banner: the repo has `commit.gpgsign=true`. Surfaced once
    /// at worker startup so the user understands why a commit might fail
    /// fast in non-interactive mode.
    GpgSignWarning,
}

/// Handle owned by the main thread for talking to the worker.
pub struct GitWorker {
    cmd_tx: Sender<GitCommand>,
    event_rx: Receiver<GitEvent>,
    join: Option<thread::JoinHandle<()>>,
}

impl GitWorker {
    /// Spawn a worker rooted at `workdir`. Reads `git config commit.gpgsign`
    /// once at startup; if true, sends a [`GitEvent::GpgSignWarning`].
    pub fn spawn(workdir: PathBuf) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<GitCommand>();
        let (event_tx, event_rx) = mpsc::channel::<GitEvent>();

        let join = thread::Builder::new()
            .name("aura-git-worker".to_string())
            .spawn(move || worker_loop(workdir, cmd_rx, event_tx))
            .expect("failed to spawn git worker");

        Self {
            cmd_tx,
            event_rx,
            join: Some(join),
        }
    }

    /// Send a command to the worker. Drops silently if the worker has
    /// shut down — losing a refresh request is harmless.
    pub fn send(&self, cmd: GitCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Drain all currently-buffered events. Call once per frame from the
    /// main event loop.
    pub fn drain_events(&self) -> Vec<GitEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.event_rx.try_recv() {
            out.push(ev);
        }
        out
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(GitCommand::Shutdown);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

/// Worker thread main loop.
fn worker_loop(workdir: PathBuf, cmd_rx: Receiver<GitCommand>, event_tx: Sender<GitEvent>) {
    // One-shot startup probe for commit.gpgsign so the user sees the
    // banner without paying for it on every commit attempt.
    if probe_gpgsign(&workdir) {
        let _ = event_tx.send(GitEvent::GpgSignWarning);
    }

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            GitCommand::Shutdown => break,
            GitCommand::RefreshStatus => run_status(&workdir, &event_tx),
            GitCommand::RefreshBranchInfo => run_branch_info(&workdir, &event_tx),
            GitCommand::RefreshStashes => run_stashes(&workdir, &event_tx),
            GitCommand::Commit { message } => run_commit(&workdir, &message, &event_tx),
        }
    }
}

/// Returns true if `git config --get commit.gpgsign` resolves to a truthy value.
fn probe_gpgsign(workdir: &PathBuf) -> bool {
    let out = run_git_serialized(
        Command::new("git")
            .args(["config", "--get", "commit.gpgsign"])
            .current_dir(workdir)
            .stdin(Stdio::null()),
    );
    matches!(
        out,
        Ok(o) if o.status.success()
            && matches!(
                String::from_utf8_lossy(&o.stdout).trim(),
                "true" | "yes" | "1" | "on"
            )
    )
}

fn run_status(workdir: &PathBuf, event_tx: &Sender<GitEvent>) {
    match run_with_timeout(
        Command::new("git")
            .args(["status", "--porcelain=v1"])
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        REFRESH_TIMEOUT,
        Some(workdir),
    ) {
        TimedOutput::Ok { stdout, .. } => {
            let entries = parse_porcelain_v1(&stdout);
            let _ = event_tx.send(GitEvent::StatusReady(entries));
        }
        TimedOutput::Timeout => {
            let _ = event_tx.send(GitEvent::Timeout(GitRefreshKind::Status));
        }
        TimedOutput::Failed(message) => {
            let _ = event_tx.send(GitEvent::Failed {
                kind: GitRefreshKind::Status,
                message,
            });
        }
    }
}

fn run_branch_info(workdir: &PathBuf, event_tx: &Sender<GitEvent>) {
    match run_with_timeout(
        Command::new("git")
            .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        REFRESH_TIMEOUT,
        Some(workdir),
    ) {
        TimedOutput::Ok { stdout, .. } => {
            let text = String::from_utf8_lossy(&stdout);
            let parts: Vec<&str> = text.trim().split('\t').collect();
            let (ahead, behind) = if parts.len() == 2 {
                (parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0))
            } else {
                (0, 0)
            };
            let _ = event_tx.send(GitEvent::BranchInfoReady { ahead, behind });
        }
        TimedOutput::Timeout => {
            let _ = event_tx.send(GitEvent::Timeout(GitRefreshKind::BranchInfo));
        }
        TimedOutput::Failed(_) => {
            // No upstream is the common case; surface as zero/zero rather
            // than as a banner so the panel stays clean.
            let _ = event_tx.send(GitEvent::BranchInfoReady {
                ahead: 0,
                behind: 0,
            });
        }
    }
}

fn run_stashes(workdir: &PathBuf, event_tx: &Sender<GitEvent>) {
    match run_with_timeout(
        Command::new("git")
            .args(["stash", "list", "--format=%gd|%s"])
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        REFRESH_TIMEOUT,
        Some(workdir),
    ) {
        TimedOutput::Ok { stdout, .. } => {
            let text = String::from_utf8_lossy(&stdout);
            let stashes: Vec<(String, String)> = text
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let mut parts = line.splitn(2, '|');
                    let name = parts.next().unwrap_or("").to_string();
                    let message = parts.next().unwrap_or("").to_string();
                    (name, message)
                })
                .collect();
            let _ = event_tx.send(GitEvent::StashesReady(stashes));
        }
        TimedOutput::Timeout => {
            let _ = event_tx.send(GitEvent::Timeout(GitRefreshKind::Stashes));
        }
        TimedOutput::Failed(_) => {
            // Treat failure as "no stashes" — stash list errors when
            // there's no stash entry namespace yet are noisy and benign.
            let _ = event_tx.send(GitEvent::StashesReady(Vec::new()));
        }
    }
}

fn run_commit(workdir: &PathBuf, message: &str, event_tx: &Sender<GitEvent>) {
    match run_with_timeout(
        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        COMMIT_TIMEOUT,
        Some(workdir),
    ) {
        TimedOutput::Ok {
            status_success,
            stdout,
            stderr,
        } if status_success => {
            // Try to read the new HEAD short hash.
            let short = read_head_short(workdir).unwrap_or_else(|| {
                String::from_utf8_lossy(&stdout)
                    .lines()
                    .next()
                    .unwrap_or("OK")
                    .to_string()
            });
            let _ = stderr; // unused on success path
            let _ = event_tx.send(GitEvent::CommitDone(Ok(short)));
        }
        TimedOutput::Ok { stderr, .. } => {
            let msg = String::from_utf8_lossy(&stderr).trim().to_string();
            let msg = if msg.is_empty() {
                "git commit failed".to_string()
            } else {
                msg
            };
            let _ = event_tx.send(GitEvent::CommitDone(Err(msg)));
        }
        TimedOutput::Timeout => {
            let _ = event_tx.send(GitEvent::Timeout(GitRefreshKind::Commit));
        }
        TimedOutput::Failed(message) => {
            let _ = event_tx.send(GitEvent::CommitDone(Err(message)));
        }
    }
}

/// Read the new HEAD short hash after a successful commit.
fn read_head_short(workdir: &PathBuf) -> Option<String> {
    let out = run_git_serialized(
        Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(workdir)
            .stdin(Stdio::null()),
    )
    .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Result of a timed-out subprocess run.
enum TimedOutput {
    Ok {
        status_success: bool,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Timeout,
    Failed(String),
}

/// Spawn the configured `Command`, poll `try_wait` every 50 ms, and kill
/// the child if `timeout` elapses without the process exiting.
///
/// On timeout we send SIGTERM first and give git a brief grace window
/// ([`SIGTERM_GRACE`]) to release `.git/index.lock` cleanly before we
/// escalate to SIGKILL. After SIGKILL, we sweep any `index.lock` that
/// appeared during this run — a SIGKILL between lock acquisition and
/// the rename-into-place leaves a stale lock that blocks every later
/// `git add` until removed.
///
/// The whole run holds [`git_cli_lock`] so other Aura-owned git
/// subprocesses can't collide with it.
///
/// `workdir` is needed for the post-timeout lock sweep. If it is `None`,
/// the sweep is skipped (used by the timeout self-test).
///
/// Caller must already have configured `stdin`/`stdout`/`stderr` on the
/// `Command` (we don't re-pipe here because the caller may want to vary
/// it per command).
fn run_with_timeout(cmd: &mut Command, timeout: Duration, workdir: Option<&Path>) -> TimedOutput {
    let _guard = git_cli_lock().lock().unwrap_or_else(|e| e.into_inner());

    // Snapshot the lock state before spawning. `None` = no pre-existing
    // lock; if we time out and a lock is present afterward, it's one we
    // (or git on our behalf) created and orphaned.
    let lock_path = workdir.map(|w| w.join(".git").join("index.lock"));
    let pre_existed = lock_path.as_deref().map(|p| p.exists()).unwrap_or(false);

    let mut child: Child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return TimedOutput::Failed(format!("spawn failed: {e}")),
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_end(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = s.read_to_end(&mut stderr);
                }
                return TimedOutput::Ok {
                    status_success: status.success(),
                    stdout,
                    stderr,
                };
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    terminate_child(&mut child);
                    if !pre_existed {
                        if let Some(ref p) = lock_path {
                            sweep_orphan_lock(p, started);
                        }
                    }
                    return TimedOutput::Timeout;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return TimedOutput::Failed(format!("wait failed: {e}")),
        }
    }
}

/// Send SIGTERM, give git a [`SIGTERM_GRACE`] window to clean up its
/// `index.lock`, then escalate to SIGKILL. On non-Unix platforms we have
/// no SIGTERM equivalent that `Command` exposes, so we fall through to
/// the immediate kill.
fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        // SAFETY: `child.id()` returns a valid process id while the
        // child handle has not been waited on. `libc::kill` is async-
        // signal-safe and just delivers a signal.
        let pid = child.id() as libc::pid_t;
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + SIGTERM_GRACE;
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Remove `.git/index.lock` if it appeared during this run and is
/// therefore an orphan we created. Mtime check is a cheap heuristic:
/// we only delete locks whose mtime is at-or-after `run_started`, which
/// rules out a fresh lock created by another process the instant we
/// returned.
fn sweep_orphan_lock(lock_path: &Path, run_started: Instant) {
    let Ok(meta) = std::fs::metadata(lock_path) else {
        return; // gone already — nothing to clean up
    };
    let Ok(mtime) = meta.modified() else {
        // No mtime support — be conservative and don't touch it.
        return;
    };
    // Convert `run_started` (Instant) to a SystemTime threshold. We can't
    // compare Instant↔SystemTime directly, so derive the threshold by
    // walking back from "now": threshold = now - elapsed_since_started.
    let elapsed = run_started.elapsed();
    let threshold = SystemTime::now()
        .checked_sub(elapsed)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    if mtime >= threshold {
        match std::fs::remove_file(lock_path) {
            Ok(()) => {
                tracing::warn!("swept orphan {} after git timeout", lock_path.display());
            }
            Err(e) => {
                tracing::debug!("failed to sweep orphan {}: {}", lock_path.display(), e);
            }
        }
    }
}

/// Parse `git status --porcelain=v1` output into structured entries.
///
/// Extracted as a free function so worker tests don't need a real repo.
pub fn parse_porcelain_v1(stdout: &[u8]) -> Vec<GitStatusEntry> {
    let text = String::from_utf8_lossy(stdout);
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.len() < 4 {
            continue;
        }
        let bytes = line.as_bytes();
        let index_status = bytes[0] as char;
        let worktree_status = bytes[1] as char;
        let rel_path = line[3..].to_string();
        entries.push(GitStatusEntry {
            rel_path,
            index_status,
            worktree_status,
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v1_basic() {
        let input = b" M src/main.rs\n?? README.md\nA  src/new.rs\n";
        let entries = parse_porcelain_v1(input);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].rel_path, "src/main.rs");
        assert_eq!(entries[0].index_status, ' ');
        assert_eq!(entries[0].worktree_status, 'M');
        assert_eq!(entries[1].rel_path, "README.md");
        assert_eq!(entries[1].index_status, '?');
        assert_eq!(entries[1].worktree_status, '?');
        assert_eq!(entries[2].rel_path, "src/new.rs");
        assert_eq!(entries[2].index_status, 'A');
        assert_eq!(entries[2].worktree_status, ' ');
    }

    #[test]
    fn parses_porcelain_v1_skips_short_lines() {
        let input = b"M\n";
        let entries = parse_porcelain_v1(input);
        assert!(entries.is_empty());
    }

    #[test]
    fn timeout_kills_long_running_child() {
        // `sleep 30` lasts well beyond the 200 ms timeout.
        let mut cmd = Command::new("sleep");
        cmd.arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let started = Instant::now();
        let res = run_with_timeout(&mut cmd, Duration::from_millis(200), None);
        let elapsed = started.elapsed();
        assert!(matches!(res, TimedOutput::Timeout));
        // SIGTERM grace + poll period; comfortably under a second normally.
        assert!(
            elapsed < Duration::from_secs(2),
            "kill took too long: {elapsed:?}"
        );
    }

    #[test]
    fn timeout_returns_ok_for_fast_child() {
        let mut cmd = Command::new("true");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let res = run_with_timeout(&mut cmd, Duration::from_secs(5), None);
        match res {
            TimedOutput::Ok { status_success, .. } => assert!(status_success),
            _ => panic!("expected Ok"),
        }
    }

    /// Simulates a hung `git` mid-write: spawn a `sh` process that
    /// creates `.git/index.lock` and then sleeps. After timeout, the
    /// orphaned lock should be swept.
    #[cfg(unix)]
    #[test]
    fn timeout_sweeps_orphan_index_lock() {
        let dir = std::env::temp_dir().join(format!("aura-git-sweep-{}", std::process::id()));
        let git_dir = dir.join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        let lock = git_dir.join("index.lock");
        // Start with no lock present so the run flags it as orphan-eligible.
        let _ = std::fs::remove_file(&lock);

        let script = format!("touch {}; sleep 30", lock.display());
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let res = run_with_timeout(&mut cmd, Duration::from_millis(300), Some(&dir));
        assert!(matches!(res, TimedOutput::Timeout));
        assert!(!lock.exists(), "orphan index.lock should have been swept");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// If a lock already exists when the run starts, the sweep must
    /// leave it alone — it isn't ours to remove.
    #[cfg(unix)]
    #[test]
    fn timeout_preserves_pre_existing_index_lock() {
        let dir = std::env::temp_dir().join(format!("aura-git-preserve-{}", std::process::id()));
        let git_dir = dir.join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        let lock = git_dir.join("index.lock");
        std::fs::write(&lock, b"").unwrap();

        let mut cmd = Command::new("sleep");
        cmd.arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let res = run_with_timeout(&mut cmd, Duration::from_millis(200), Some(&dir));
        assert!(matches!(res, TimedOutput::Timeout));
        assert!(lock.exists(), "pre-existing index.lock must not be swept");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
