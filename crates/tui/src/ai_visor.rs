//! AI Visor — Claude Code configuration browser panel.
//!
//! Displays the contents of the `.claude/` folder (settings, skills, hooks,
//! plugins, CLAUDE.md) in a tabbed right-side panel. Provides a unique
//! visual overview of Claude Code's configuration that no other editor offers.

use std::path::{Path, PathBuf};

/// Which tab is active in the AI Visor panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisorTab {
    /// Dashboard: model, CLAUDE.md status, quick stats.
    Overview,
    /// Merged settings cascade with scope indicators.
    Settings,
    /// List of available skills.
    Skills,
    /// Hook event types and configured hooks.
    Hooks,
    /// Installed plugins.
    Plugins,
}

/// A skill entry parsed from `.claude/skills/*/SKILL.md`.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    /// Skill name (directory name or from frontmatter).
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Path to the SKILL.md file.
    pub path: PathBuf,
    /// Whether the skill is user-invocable (vs model-only).
    pub invocable: bool,
}

/// A hook entry extracted from settings JSON.
#[derive(Debug, Clone)]
pub struct HookEntry {
    /// Event name (e.g. "PreToolUse", "PostToolUse", "Stop").
    pub event: String,
    /// Hook type: "command", "http", "prompt", "agent".
    pub hook_type: String,
    /// The command/URL/prompt string.
    pub command: String,
}

/// An installed plugin.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    /// Plugin name.
    pub name: String,
    /// Source URL or path.
    pub source: String,
}

/// A settings entry with scope indicator.
#[derive(Debug, Clone)]
pub struct SettingsEntry {
    /// Scope: "G" (global), "P" (project), "L" (local).
    pub scope: String,
    /// Key path (e.g. "permissions.allow").
    pub key: String,
    /// Display value.
    pub value: String,
}

/// All loaded visor data.
#[derive(Debug, Clone, Default)]
pub struct VisorSections {
    /// CLAUDE.md content (if found).
    pub claude_md: Option<String>,
    /// CLAUDE.md file size in bytes.
    pub claude_md_size: usize,
    /// Flattened settings entries with scopes.
    pub settings: Vec<SettingsEntry>,
    /// Number of permission allow rules.
    pub permissions_allow_count: usize,
    /// Available skills.
    pub skills: Vec<SkillEntry>,
    /// Configured hooks.
    pub hooks: Vec<HookEntry>,
    /// Installed plugins.
    pub plugins: Vec<PluginEntry>,
    /// Detected model name.
    pub model: Option<String>,
    /// Detected effort level.
    pub effort: Option<String>,
}

/// State for the AI Visor right-side panel.
pub struct AiVisorPanel {
    /// Whether the panel is visible.
    pub visible: bool,
    /// Panel width in columns.
    pub width: u16,
    /// Active tab.
    pub active_tab: VisorTab,
    /// Loaded data.
    pub sections: VisorSections,
    /// Selected item index within the current tab.
    pub selected: usize,
    /// Scroll offset.
    pub scroll: usize,
}

impl AiVisorPanel {
    /// Create a new AI Visor panel.
    pub fn new(width: u16) -> Self {
        Self {
            visible: false,
            width,
            active_tab: VisorTab::Overview,
            sections: VisorSections::default(),
            selected: 0,
            scroll: 0,
        }
    }

    /// Toggle visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Navigate up.
    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Navigate down.
    pub fn select_down(&mut self) {
        let max = self.current_list_len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Cycle to the next tab.
    pub fn next_tab(&mut self) {
        self.active_tab = match self.active_tab {
            VisorTab::Overview => VisorTab::Settings,
            VisorTab::Settings => VisorTab::Skills,
            VisorTab::Skills => VisorTab::Hooks,
            VisorTab::Hooks => VisorTab::Plugins,
            VisorTab::Plugins => VisorTab::Overview,
        };
        self.selected = 0;
        self.scroll = 0;
    }

    /// Get the number of items in the current tab.
    fn current_list_len(&self) -> usize {
        match self.active_tab {
            VisorTab::Overview => 0,
            VisorTab::Settings => self.sections.settings.len(),
            VisorTab::Skills => self.sections.skills.len(),
            VisorTab::Hooks => self.sections.hooks.len(),
            VisorTab::Plugins => self.sections.plugins.len(),
        }
    }

    /// Get the path of the selected skill (for opening in editor).
    pub fn selected_skill_path(&self) -> Option<&Path> {
        if self.active_tab == VisorTab::Skills {
            self.sections
                .skills
                .get(self.selected)
                .map(|s| s.path.as_path())
        } else {
            None
        }
    }
}

/// Load all Claude Code configuration data from the project.
pub fn load_visor_data(project_root: &Path) -> VisorSections {
    let mut sections = VisorSections::default();

    // Load CLAUDE.md from project root.
    let claude_md_path = project_root.join("CLAUDE.md");
    if let Ok(content) = std::fs::read_to_string(&claude_md_path) {
        sections.claude_md_size = content.len();
        sections.claude_md = Some(content);
    }

    // Load settings files.
    let project_settings = load_json(&project_root.join(".claude/settings.json"));
    let local_settings = load_json(&project_root.join(".claude/settings.local.json"));
    let global_settings = dirs_home()
        .map(|h| load_json(&h.join(".claude/settings.json")))
        .unwrap_or_default();

    // Extract model and effort from settings.
    sections.model = extract_string(&project_settings, "model")
        .or_else(|| extract_string(&local_settings, "model"))
        .or_else(|| extract_string(&global_settings, "model"));
    sections.effort = extract_string(&project_settings, "effortLevel")
        .or_else(|| extract_string(&local_settings, "effortLevel"))
        .or_else(|| extract_string(&global_settings, "effortLevel"));

    // Build flattened settings entries.
    flatten_settings(&global_settings, "G", &mut sections.settings);
    flatten_settings(&project_settings, "P", &mut sections.settings);
    flatten_settings(&local_settings, "L", &mut sections.settings);

    // Count permissions.
    sections.permissions_allow_count = count_permissions_allow(&project_settings)
        + count_permissions_allow(&local_settings)
        + count_permissions_allow(&global_settings);

    // Load skills from .claude/skills/*/.
    let skills_dir = project_root.join(".claude/skills");
    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let skill_md = entry.path().join("SKILL.md");
                if skill_md.exists() {
                    if let Ok(content) = std::fs::read_to_string(&skill_md) {
                        let skill = parse_skill_md(&content, &skill_md);
                        sections.skills.push(skill);
                    }
                }
            }
        }
    }
    // Also check .claude/commands/ (legacy).
    let commands_dir = project_root.join(".claude/commands");
    if let Ok(entries) = std::fs::read_dir(&commands_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let desc = content.lines().next().unwrap_or("").to_string();
                    sections.skills.push(SkillEntry {
                        name,
                        description: desc,
                        path,
                        invocable: true,
                    });
                }
            }
        }
    }

    // Load hooks from settings.
    extract_hooks(&project_settings, &mut sections.hooks);
    extract_hooks(&local_settings, &mut sections.hooks);
    extract_hooks(&global_settings, &mut sections.hooks);

    // Load plugins from global config.
    if let Some(home) = dirs_home() {
        let plugins_path = home.join(".claude/plugins/installed_plugins.json");
        if let Ok(content) = std::fs::read_to_string(&plugins_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(arr) = val.as_array() {
                    for item in arr {
                        let name = item
                            .get("name")
                            .or_else(|| item.get("package"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let source = item
                            .get("source")
                            .or_else(|| item.get("url"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        sections.plugins.push(PluginEntry { name, source });
                    }
                }
            }
        }
    }

    sections
}

// ── Helpers ─────────────────────────────────────────────────────

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn load_json(path: &Path) -> serde_json::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn extract_string(val: &serde_json::Value, key: &str) -> Option<String> {
    val.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn count_permissions_allow(val: &serde_json::Value) -> usize {
    val.get("permissions")
        .and_then(|p| p.get("allow"))
        .and_then(|a| a.as_array())
        .map_or(0, |arr| arr.len())
}

fn flatten_settings(val: &serde_json::Value, scope: &str, out: &mut Vec<SettingsEntry>) {
    if let Some(obj) = val.as_object() {
        for (key, value) in obj {
            if key == "permissions" {
                // Flatten permissions specially.
                if let Some(allow) = value.get("allow").and_then(|a| a.as_array()) {
                    for item in allow {
                        let display = item
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| serde_json::to_string(item).unwrap_or_default());
                        out.push(SettingsEntry {
                            scope: scope.to_string(),
                            key: "permissions.allow".to_string(),
                            value: display,
                        });
                    }
                }
                if let Some(deny) = value.get("deny").and_then(|a| a.as_array()) {
                    for item in deny {
                        let display = item
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| serde_json::to_string(item).unwrap_or_default());
                        out.push(SettingsEntry {
                            scope: scope.to_string(),
                            key: "permissions.deny".to_string(),
                            value: display,
                        });
                    }
                }
            } else if key == "hooks" {
                // Skip hooks — shown in their own tab.
            } else {
                let display = match value {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => serde_json::to_string(value).unwrap_or_default(),
                };
                if !display.is_empty() {
                    out.push(SettingsEntry {
                        scope: scope.to_string(),
                        key: key.clone(),
                        value: display,
                    });
                }
            }
        }
    }
}

fn extract_hooks(val: &serde_json::Value, out: &mut Vec<HookEntry>) {
    if let Some(hooks) = val.get("hooks").and_then(|h| h.as_object()) {
        for (event, hook_list) in hooks {
            if let Some(arr) = hook_list.as_array() {
                for hook in arr {
                    let hook_type = hook
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("command")
                        .to_string();
                    let command = hook
                        .get("command")
                        .or_else(|| hook.get("url"))
                        .or_else(|| hook.get("prompt"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    out.push(HookEntry {
                        event: event.clone(),
                        hook_type,
                        command,
                    });
                }
            }
        }
    }
}

fn parse_skill_md(content: &str, path: &Path) -> SkillEntry {
    let mut name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut description = String::new();
    let mut invocable = true;

    // Parse YAML frontmatter if present.
    if let Some(after_start) = content.strip_prefix("---") {
        if let Some(end) = after_start.find("---") {
            let frontmatter = &after_start[..end];
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:") {
                    name = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = line.strip_prefix("description:") {
                    description = val.trim().trim_matches('"').to_string();
                } else if line.starts_with("user-invocable:") {
                    invocable = line.contains("true");
                }
            }
        }
    }

    if description.is_empty() {
        // Use first non-empty, non-header line.
        description = content
            .lines()
            .skip_while(|l| l.starts_with("---") || l.trim().is_empty() || l.starts_with('#'))
            .find(|l| !l.trim().is_empty() && !l.starts_with("---"))
            .unwrap_or("")
            .to_string();
    }

    SkillEntry {
        name,
        description,
        path: path.to_path_buf(),
        invocable,
    }
}
