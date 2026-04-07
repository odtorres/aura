//! Plugin marketplace — discover, install, update, and uninstall AURA plugins.
//!
//! Uses a git-based registry (JSON file hosted on GitHub) and downloads
//! plugin files (`.lua` + `plugin.toml`) to `~/.aura/plugins/`.

use serde::Deserialize;
use std::path::PathBuf;

/// Default registry URL.
pub const DEFAULT_REGISTRY: &str =
    "https://raw.githubusercontent.com/odtorres/aura-plugins/main/registry.json";

/// A plugin listing from the remote registry.
#[derive(Debug, Clone, Deserialize)]
pub struct PluginListing {
    /// Plugin name (unique identifier).
    pub name: String,
    /// SemVer version string.
    pub version: String,
    /// One-line description.
    #[serde(default)]
    pub description: String,
    /// Author name.
    #[serde(default)]
    pub author: String,
    /// Source repository URL.
    #[serde(default)]
    pub repo: String,
    /// Direct download URL for the `.lua` file.
    #[serde(default)]
    pub download: String,
    /// Direct download URL for `plugin.toml`.
    #[serde(default)]
    pub toml: String,
}

/// An installed plugin discovered from `~/.aura/plugins/`.
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    /// Plugin name.
    pub name: String,
    /// Version string (empty if no plugin.toml).
    pub version: String,
    /// Description (empty if not available).
    pub description: String,
    /// Author (empty if not available).
    pub author: String,
    /// Path to the `.lua` file.
    pub path: PathBuf,
}

/// Plugin marketplace state.
pub struct MarketplaceModal {
    /// Whether the modal is visible.
    pub visible: bool,
    /// Cached registry listings.
    pub registry: Vec<PluginListing>,
    /// Installed plugins.
    pub installed: Vec<InstalledPlugin>,
    /// Search query.
    pub query: String,
    /// Filtered indices into registry.
    pub filtered: Vec<usize>,
    /// Selected index in filtered list.
    pub selected: usize,
    /// Status message.
    pub status: String,
}

impl MarketplaceModal {
    /// Create a new marketplace modal.
    pub fn new() -> Self {
        Self {
            visible: false,
            registry: Vec::new(),
            installed: Vec::new(),
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
            status: String::new(),
        }
    }

    /// Open the modal, refreshing installed plugins.
    pub fn open(&mut self) {
        self.installed = list_installed();
        self.filter();
        self.selected = 0;
        self.visible = true;
        if self.registry.is_empty() {
            self.status = "Press 'r' to refresh registry".to_string();
        } else {
            self.status = format!(
                "{} plugins in registry, {} installed",
                self.registry.len(),
                self.installed.len()
            );
        }
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
    }

    /// Type a character into the search query.
    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.filter();
        self.selected = 0;
    }

    /// Backspace in the search query.
    pub fn backspace(&mut self) {
        self.query.pop();
        self.filter();
        self.selected = 0;
    }

    /// Recompute filtered indices.
    pub fn filter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = self
            .registry
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                q.is_empty()
                    || p.name.to_lowercase().contains(&q)
                    || p.description.to_lowercase().contains(&q)
                    || p.author.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
    }

    /// Navigate down.
    pub fn select_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    /// Navigate up.
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Get the selected listing.
    pub fn selected_listing(&self) -> Option<&PluginListing> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.registry.get(i))
    }

    /// Check if a plugin is installed.
    pub fn is_installed(&self, name: &str) -> bool {
        self.installed.iter().any(|p| p.name == name)
    }

    /// Check if an installed plugin has an update available.
    pub fn has_update(&self, name: &str) -> bool {
        let installed_ver = self
            .installed
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.version.as_str())
            .unwrap_or("");
        let registry_ver = self
            .registry
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.version.as_str())
            .unwrap_or("");
        !installed_ver.is_empty() && !registry_ver.is_empty() && installed_ver != registry_ver
    }
}

impl Default for MarketplaceModal {
    fn default() -> Self {
        Self::new()
    }
}

// ── Marketplace operations ─────────────────────────────────────

/// Fetch the plugin registry from a URL.
pub fn fetch_registry(url: &str) -> anyhow::Result<Vec<PluginListing>> {
    let output = std::process::Command::new("curl")
        .args(["-sSfL", "--max-time", "10", url])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run curl: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Registry fetch failed: {}", stderr.trim());
    }

    let listings: Vec<PluginListing> = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!("Invalid registry JSON: {e}"))?;

    // Cache locally.
    let cache_path = plugins_dir().join("registry_cache.json");
    let _ = std::fs::write(&cache_path, &output.stdout);

    Ok(listings)
}

/// Load cached registry from disk (for offline use).
pub fn load_cached_registry() -> Vec<PluginListing> {
    let cache_path = plugins_dir().join("registry_cache.json");
    std::fs::read_to_string(&cache_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Install a plugin from the registry.
pub fn install_plugin(listing: &PluginListing) -> anyhow::Result<()> {
    let dir = plugins_dir();
    let _ = std::fs::create_dir_all(&dir);

    // Download .lua file.
    if !listing.download.is_empty() {
        let lua_path = dir.join(format!("{}.lua", listing.name));
        download_file(&listing.download, &lua_path)?;
    }

    // Download plugin.toml.
    if !listing.toml.is_empty() {
        let toml_path = dir.join(format!("{}.plugin.toml", listing.name));
        download_file(&listing.toml, &toml_path)?;
    } else {
        // Generate a minimal plugin.toml from listing metadata.
        let toml_content = format!(
            "name = \"{}\"\nversion = \"{}\"\ndescription = \"{}\"\nauthor = \"{}\"\n",
            listing.name, listing.version, listing.description, listing.author
        );
        let toml_path = dir.join(format!("{}.plugin.toml", listing.name));
        std::fs::write(&toml_path, toml_content)?;
    }

    Ok(())
}

/// Uninstall a plugin by name.
pub fn uninstall_plugin(name: &str) -> anyhow::Result<()> {
    let dir = plugins_dir();
    let lua_path = dir.join(format!("{}.lua", name));
    let toml_path = dir.join(format!("{}.plugin.toml", name));

    if lua_path.exists() {
        std::fs::remove_file(&lua_path)?;
    }
    if toml_path.exists() {
        std::fs::remove_file(&toml_path)?;
    }

    Ok(())
}

/// List installed plugins from `~/.aura/plugins/`.
pub fn list_installed() -> Vec<InstalledPlugin> {
    let dir = plugins_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut plugins = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "lua") {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Try to load metadata from plugin.toml.
        let toml_path = dir.join(format!("{}.plugin.toml", stem));
        let (name, version, description, author) =
            if let Ok(content) = std::fs::read_to_string(&toml_path) {
                parse_plugin_toml(&content, &stem)
            } else {
                (stem, String::new(), String::new(), String::new())
            };

        plugins.push(InstalledPlugin {
            name,
            version,
            description,
            author,
            path,
        });
    }

    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

// ── Helpers ────────────────────────────────────────────────────

/// Get the plugins directory path.
fn plugins_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".aura/plugins")
}

/// Download a file from a URL to a local path.
fn download_file(url: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let output = std::process::Command::new("curl")
        .args(["-sSfL", "--max-time", "30", "-o"])
        .arg(dest.as_os_str())
        .arg(url)
        .output()
        .map_err(|e| anyhow::anyhow!("Download failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Download failed: {}", stderr.trim());
    }

    Ok(())
}

/// Parse a plugin.toml file for metadata.
pub(crate) fn parse_plugin_toml(
    content: &str,
    default_name: &str,
) -> (String, String, String, String) {
    let mut name = default_name.to_string();
    let mut version = String::new();
    let mut description = String::new();
    let mut author = String::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name") {
            if let Some(val) = val.trim_start().strip_prefix('=') {
                name = val.trim().trim_matches('"').to_string();
            }
        } else if let Some(val) = line.strip_prefix("version") {
            if let Some(val) = val.trim_start().strip_prefix('=') {
                version = val.trim().trim_matches('"').to_string();
            }
        } else if let Some(val) = line.strip_prefix("description") {
            if let Some(val) = val.trim_start().strip_prefix('=') {
                description = val.trim().trim_matches('"').to_string();
            }
        } else if let Some(val) = line.strip_prefix("author") {
            if let Some(val) = val.trim_start().strip_prefix('=') {
                author = val.trim().trim_matches('"').to_string();
            }
        }
    }

    (name, version, description, author)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plugin_toml() {
        let toml_content = r#"
name = "my-plugin"
version = "1.2.3"
description = "A test plugin"
author = "Test Author"
"#;
        let (name, version, description, author) = parse_plugin_toml(toml_content, "fallback");
        assert_eq!(name, "my-plugin");
        assert_eq!(version, "1.2.3");
        assert_eq!(description, "A test plugin");
        assert_eq!(author, "Test Author");
    }

    #[test]
    fn test_marketplace_modal_filter() {
        let mut modal = MarketplaceModal::new();
        modal.registry = vec![
            PluginListing {
                name: "git-blame".to_string(),
                version: "0.1.0".to_string(),
                description: "Show git blame annotations".to_string(),
                author: "Alice".to_string(),
                repo: String::new(),
                download: String::new(),
                toml: String::new(),
            },
            PluginListing {
                name: "formatter".to_string(),
                version: "0.2.0".to_string(),
                description: "Auto-format on save".to_string(),
                author: "Bob".to_string(),
                repo: String::new(),
                download: String::new(),
                toml: String::new(),
            },
            PluginListing {
                name: "snippets-extra".to_string(),
                version: "1.0.0".to_string(),
                description: "Extra code snippets".to_string(),
                author: "Alice".to_string(),
                repo: String::new(),
                download: String::new(),
                toml: String::new(),
            },
        ];

        // No query — all entries match.
        modal.filter();
        assert_eq!(modal.filtered.len(), 3);

        // Filter by "git" — only git-blame matches.
        modal.query = "git".to_string();
        modal.filter();
        assert_eq!(modal.filtered.len(), 1);
        assert_eq!(modal.filtered[0], 0);

        // Filter by author "alice" (case-insensitive).
        modal.query = "alice".to_string();
        modal.filter();
        assert_eq!(modal.filtered.len(), 2);
        assert_eq!(modal.filtered[0], 0);
        assert_eq!(modal.filtered[1], 2);
    }
}
