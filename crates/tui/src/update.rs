//! In-app update checker.
//!
//! Spawns a background thread that queries the GitHub Releases API at most once
//! per configured interval, compares the latest release tag against the running
//! version, and sends the result over a channel so the main event loop can
//! display a non-intrusive notification.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Current binary version baked in at compile time.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of the background update check.
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    /// A newer version is available.
    Available {
        /// The new version string (e.g. "0.2.0").
        version: String,
        /// URL to the GitHub release page.
        url: String,
    },
    /// The running version is up-to-date.
    UpToDate,
    /// The check failed (network error, parse error, etc.).
    Error(String),
}

/// How AURA was installed — used to tailor upgrade instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMethod {
    /// Installed via Homebrew.
    Homebrew,
    /// Installed via `cargo install`.
    CargoInstall,
    /// Installed from the AUR.
    Aur,
    /// Installed via the shell installer script.
    ShellInstaller,
    /// Unknown installation method.
    Unknown,
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// On-disk cache so we don't hit the API on every launch.
#[derive(Debug, Serialize, Deserialize)]
struct UpdateCache {
    last_check_unix: u64,
    latest_version: String,
    release_url: String,
    current_version_at_check: String,
}

/// Where the cache file lives: `~/.config/aura/update-check.json`.
fn cache_path() -> Option<PathBuf> {
    dirs_or_fallback().map(|d| d.join("update-check.json"))
}

/// Returns `~/.config/aura/` (or platform equivalent), creating it if needed.
fn dirs_or_fallback() -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        dirs_path_home()?.join(".config")
    } else if cfg!(target_os = "windows") {
        std::env::var("APPDATA").ok().map(PathBuf::from)?
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs_path_home().unwrap_or_default().join(".config"))
    };
    let dir = base.join("aura");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

fn dirs_path_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn read_cache() -> Option<UpdateCache> {
    let path = cache_path()?;
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_cache(cache: &UpdateCache) {
    if let Some(path) = cache_path() {
        if let Ok(json) = serde_json::to_string_pretty(cache) {
            let _ = std::fs::write(path, json);
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Background check
// ---------------------------------------------------------------------------

/// Spawn a background thread that performs the update check and sends the
/// result on `sender`.
pub fn spawn_update_check(sender: mpsc::Sender<UpdateStatus>, interval_hours: u64) {
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let status = perform_check(interval_hours);
            let _ = sender.send(status);
        })
        .ok();
}

/// Spawn a background thread that performs a forced update check (bypasses cache)
/// and sends the result on `sender`.
pub fn spawn_forced_update_check(sender: mpsc::Sender<UpdateStatus>) {
    std::thread::Builder::new()
        .name("update-check-forced".into())
        .spawn(move || {
            let status = perform_forced_check();
            let _ = sender.send(status);
        })
        .ok();
}

/// Perform a forced check, bypassing the cache entirely.
fn perform_forced_check() -> UpdateStatus {
    match fetch_latest_release() {
        Ok((tag, url)) => {
            let cache = UpdateCache {
                last_check_unix: now_unix(),
                latest_version: tag.clone(),
                release_url: url.clone(),
                current_version_at_check: CURRENT_VERSION.to_string(),
            };
            write_cache(&cache);
            cache_to_status(&cache)
        }
        Err(e) => UpdateStatus::Error(e),
    }
}

fn perform_check(interval_hours: u64) -> UpdateStatus {
    let interval_secs = interval_hours * 3600;

    // Check cache first.
    if let Some(cache) = read_cache() {
        let age = now_unix().saturating_sub(cache.last_check_unix);
        let same_version = cache.current_version_at_check == CURRENT_VERSION;
        if age < interval_secs && same_version {
            return cache_to_status(&cache);
        }
    }

    // Fetch latest release from GitHub.
    match fetch_latest_release() {
        Ok((tag, url)) => {
            let cache = UpdateCache {
                last_check_unix: now_unix(),
                latest_version: tag.clone(),
                release_url: url.clone(),
                current_version_at_check: CURRENT_VERSION.to_string(),
            };
            write_cache(&cache);
            cache_to_status(&cache)
        }
        Err(e) => UpdateStatus::Error(e),
    }
}

fn cache_to_status(cache: &UpdateCache) -> UpdateStatus {
    let current = match semver::Version::parse(CURRENT_VERSION) {
        Ok(v) => v,
        Err(_) => return UpdateStatus::UpToDate,
    };
    let latest = match semver::Version::parse(&cache.latest_version) {
        Ok(v) => v,
        Err(_) => return UpdateStatus::UpToDate,
    };
    if latest > current {
        UpdateStatus::Available {
            version: cache.latest_version.clone(),
            url: cache.release_url.clone(),
        }
    } else {
        UpdateStatus::UpToDate
    }
}

/// Hit the GitHub Releases API.  Returns `(version, release_url)`.
fn fetch_latest_release() -> Result<(String, String), String> {
    let url = "https://api.github.com/repos/odtorres/aura/releases/latest";
    let mut response = ureq::get(url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", &format!("aura-editor/{}", CURRENT_VERSION))
        .call()
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let body_str = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read response: {e}"))?;

    let body: serde_json::Value =
        serde_json::from_str(&body_str).map_err(|e| format!("Failed to parse JSON: {e}"))?;

    let tag = body["tag_name"]
        .as_str()
        .ok_or("Missing tag_name")?
        .trim_start_matches('v')
        .to_string();

    let html_url = body["html_url"]
        .as_str()
        .unwrap_or("https://github.com/odtorres/aura/releases")
        .to_string();

    Ok((tag, html_url))
}

// ---------------------------------------------------------------------------
// Install method detection
// ---------------------------------------------------------------------------

/// Detect how AURA was installed by inspecting the binary path.
pub fn detect_install_method() -> InstallMethod {
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => return InstallMethod::Unknown,
    };
    let exe_lower = exe.to_lowercase();

    if exe_lower.contains("homebrew") || exe_lower.contains("linuxbrew") {
        InstallMethod::Homebrew
    } else if exe_lower.contains(".cargo/bin") || exe_lower.contains(".cargo\\bin") {
        InstallMethod::CargoInstall
    } else if exe_lower.starts_with("/usr/bin/") {
        InstallMethod::Aur
    } else {
        InstallMethod::ShellInstaller
    }
}

/// Return a user-friendly upgrade instruction string.
pub fn upgrade_instructions(_method: &InstallMethod, version: &str) -> String {
    format!(
        "Run: curl --proto '=https' --tlsv1.2 -LsSf \
         https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh  (v{version})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_method_cargo() {
        // Just verify it doesn't panic; actual result depends on the test runner.
        let _ = detect_install_method();
    }

    #[test]
    fn upgrade_instructions_formatting() {
        let instr = upgrade_instructions(&InstallMethod::Homebrew, "0.2.0");
        assert!(instr.contains("curl"));
        assert!(instr.contains("0.2.0"));

        let instr = upgrade_instructions(&InstallMethod::ShellInstaller, "0.2.0");
        assert!(instr.contains("aura-installer.sh"));
    }

    #[test]
    fn cache_to_status_newer() {
        let cache = UpdateCache {
            last_check_unix: now_unix(),
            latest_version: "99.0.0".into(),
            release_url: "https://example.com".into(),
            current_version_at_check: CURRENT_VERSION.into(),
        };
        match cache_to_status(&cache) {
            UpdateStatus::Available { version, .. } => assert_eq!(version, "99.0.0"),
            other => panic!("Expected Available, got {:?}", other),
        }
    }

    #[test]
    fn cache_to_status_same() {
        let cache = UpdateCache {
            last_check_unix: now_unix(),
            latest_version: CURRENT_VERSION.into(),
            release_url: "https://example.com".into(),
            current_version_at_check: CURRENT_VERSION.into(),
        };
        assert!(matches!(cache_to_status(&cache), UpdateStatus::UpToDate));
    }
}
