//! Configuration loading and theme engine for AURA.
//!
//! Reads `aura.toml` for settings: theme, keybindings, AI model,
//! aggressiveness, and editor preferences.

use ratatui::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Top-level AURA configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuraConfig {
    /// Editor settings.
    pub editor: EditorConfig,
    /// AI settings.
    pub ai: AiSettings,
    /// Theme name (built-in or path to custom theme).
    pub theme: String,
    /// Custom keybinding overrides.
    #[serde(default)]
    pub keybindings: KeybindingConfig,
    /// MCP server connections (handled by mcp_client module).
    #[serde(default)]
    pub mcp_servers: HashMap<String, toml::Value>,
    /// Update checker settings.
    #[serde(default)]
    pub update: UpdateConfig,
    /// Collaboration settings.
    #[serde(default)]
    pub collab: CollabConfig,
    /// Conversation storage settings.
    #[serde(default)]
    pub conversations: ConversationConfig,
    /// Debug adapter configurations.
    #[serde(default)]
    pub debuggers: HashMap<String, DebuggerConfig>,
    /// Project tasks (build, test, lint, etc.).
    #[serde(default)]
    pub tasks: HashMap<String, TaskConfig>,
}

impl Default for AuraConfig {
    fn default() -> Self {
        Self {
            editor: EditorConfig::default(),
            ai: AiSettings::default(),
            theme: "dark".to_string(),
            keybindings: KeybindingConfig::default(),
            mcp_servers: HashMap::new(),
            update: UpdateConfig::default(),
            collab: CollabConfig::default(),
            conversations: ConversationConfig::default(),
            debuggers: HashMap::new(),
            tasks: HashMap::new(),
        }
    }
}

/// Update checker settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Whether to check for updates on startup.
    pub check_for_updates: bool,
    /// Minimum hours between API checks (cached locally).
    pub check_interval_hours: u64,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_for_updates: true,
            check_interval_hours: 24,
        }
    }
}

/// Collaboration session settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CollabConfig {
    /// Display name shown to other peers (defaults to hostname).
    pub display_name: String,
    /// Default port to listen on when hosting (0 = random available port).
    pub default_port: u16,
    /// Enable TLS encryption for collaboration sessions.
    pub use_tls: bool,
    /// Bind address: "127.0.0.1" for local only, "0.0.0.0" for internet.
    pub bind_address: String,
    /// Require authentication token to join a session.
    pub require_auth: bool,
}

impl Default for CollabConfig {
    fn default() -> Self {
        let display_name = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "anonymous".to_string());
        Self {
            display_name,
            default_port: 0,
            use_tls: false,
            bind_address: "127.0.0.1".to_string(),
            require_auth: false,
        }
    }
}

/// Conversation storage and compaction settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConversationConfig {
    /// Maximum age in days for conversation messages (0 = no limit).
    pub max_message_age_days: u32,
    /// Maximum messages to keep per conversation (0 = no limit).
    pub max_messages_per_conversation: usize,
    /// Maximum total conversations to retain (0 = no limit).
    pub max_conversations: usize,
    /// Number of recent messages to always preserve when compacting.
    pub keep_recent_messages: usize,
    /// Whether to auto-compact on startup.
    pub auto_compact: bool,
    /// Maximum context messages sent to the AI API per chat turn.
    pub max_context_messages: usize,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            max_message_age_days: 90,
            max_messages_per_conversation: 200,
            max_conversations: 500,
            keep_recent_messages: 10,
            auto_compact: true,
            max_context_messages: 40,
        }
    }
}

/// Configuration for a debug adapter executable.
#[derive(Debug, Clone, Deserialize)]
pub struct DebuggerConfig {
    /// Command to run the debug adapter.
    pub command: String,
    /// Arguments for the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions this debugger handles (e.g. ["rs", "c", "cpp"]).
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Configuration for a project task.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskConfig {
    /// The shell command to run.
    pub command: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
}

/// Editor-specific settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    /// Show line numbers.
    pub line_numbers: bool,
    /// Show authorship markers in the gutter.
    pub show_authorship: bool,
    /// Show the minimap scrollbar on the right edge.
    pub show_minimap: bool,
    /// Tab width in spaces.
    pub tab_width: usize,
    /// Use spaces instead of tabs.
    pub spaces_for_tabs: bool,
    /// Scroll margin (lines from edge before scrolling).
    pub scroll_margin: usize,
    /// Auto-save interval in seconds (0 = disabled).
    pub auto_save_seconds: u64,
    /// Show relative line numbers (distance from cursor).
    pub relative_line_numbers: bool,
    /// Enable soft word wrap (no horizontal scrolling).
    pub word_wrap: bool,
    /// Run language formatter on save (uses LSP formatting or external command).
    pub format_on_save: bool,
    /// Automatically sync yank register with system clipboard.
    pub clipboard_sync: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            line_numbers: true,
            show_authorship: true,
            show_minimap: true,
            tab_width: 4,
            spaces_for_tabs: true,
            scroll_margin: 5,
            auto_save_seconds: 0,
            relative_line_numbers: false,
            word_wrap: false,
            format_on_save: false,
            clipboard_sync: true,
        }
    }
}

/// AI-related settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    /// Default AI model (used for chat and general requests).
    pub model: String,
    /// Maximum tokens for AI responses.
    pub max_tokens: u32,
    /// Speculative analysis aggressiveness: "minimal", "moderate", "proactive".
    pub aggressiveness: String,
    /// Idle time (ms) before triggering speculative analysis.
    pub idle_threshold_ms: u64,
    /// Maximum context messages to keep in the chat panel (0 = no limit).
    pub max_context_messages: usize,

    // --- Per-feature model overrides (empty = use default `model`) ---
    /// Model for commit message generation (e.g., "claude-haiku-4-5-20251001" for speed).
    #[serde(default)]
    pub commit_model: String,
    /// Model for speculative/ghost suggestions.
    #[serde(default)]
    pub speculative_model: String,
    /// Model for agent mode (autonomous tasks).
    #[serde(default)]
    pub agent_model: String,
    /// Model for chat panel conversations.
    #[serde(default)]
    pub chat_model: String,
    /// Model for conversation summarization/compaction.
    #[serde(default)]
    pub summarize_model: String,
}

impl AiSettings {
    /// Get the model for a specific feature, falling back to the default.
    pub fn model_for(&self, feature: &str) -> &str {
        let override_model = match feature {
            "commit" => &self.commit_model,
            "speculative" | "ghost" => &self.speculative_model,
            "agent" => &self.agent_model,
            "chat" => &self.chat_model,
            "summarize" | "compact" => &self.summarize_model,
            _ => &self.model,
        };
        if override_model.is_empty() {
            &self.model
        } else {
            override_model
        }
    }
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            aggressiveness: "moderate".to_string(),
            idle_threshold_ms: 3000,
            max_context_messages: 40,
            commit_model: String::new(),
            speculative_model: String::new(),
            agent_model: String::new(),
            chat_model: String::new(),
            summarize_model: String::new(),
        }
    }
}

/// Keybinding overrides. Maps action names to key descriptions.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct KeybindingConfig {
    /// Leader key (default: "Space"). Options: "Space", "Backslash", "Comma".
    pub leader: Option<String>,
    /// Custom leader key mappings: key → action (e.g., "e" = "explain").
    #[serde(default)]
    pub leader_map: HashMap<String, String>,
    /// Custom normal mode mappings: key → action.
    #[serde(default)]
    pub normal_map: HashMap<String, String>,
    /// Custom global shortcut mappings: key → action (e.g., "ctrl+j" = "toggle_chat").
    #[serde(default)]
    pub global_map: HashMap<String, String>,
}

impl KeybindingConfig {
    /// Look up a global shortcut action by key code and modifiers.
    pub fn global_action(
        &self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<&str> {
        let key_str = format_key(code, modifiers);
        self.global_map.get(&key_str).map(|s| s.as_str())
    }

    /// Look up a leader key action by the character pressed after leader.
    pub fn leader_action(&self, c: char) -> Option<&str> {
        self.leader_map.get(&c.to_string()).map(|s| s.as_str())
    }

    /// Check if a key code matches the configured leader key.
    pub fn is_leader_key(&self, code: crossterm::event::KeyCode) -> bool {
        let leader = self.leader.as_deref().unwrap_or("Space");
        match leader {
            "Space" => code == crossterm::event::KeyCode::Char(' '),
            "Backslash" => code == crossterm::event::KeyCode::Char('\\'),
            "Comma" => code == crossterm::event::KeyCode::Char(','),
            _ => code == crossterm::event::KeyCode::Char(' '),
        }
    }
}

/// Format a key code + modifiers into a human-readable string for config matching.
fn format_key(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> String {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    let key = match code {
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        _ => return String::new(),
    };
    parts.push(key);
    parts.join("+")
}

/// A color theme definition.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme name.
    pub name: String,
    /// Editor background.
    pub bg: Color,
    /// Default foreground text.
    pub fg: Color,
    /// Gutter (line numbers) foreground.
    pub gutter_fg: Color,
    /// Status bar background.
    pub status_bg: Color,
    /// Status bar foreground.
    pub status_fg: Color,
    /// Normal mode indicator color.
    pub mode_normal: Color,
    /// Insert mode indicator color.
    pub mode_insert: Color,
    /// Command mode indicator color.
    pub mode_command: Color,
    /// Visual mode indicator color.
    pub mode_visual: Color,
    /// Intent mode indicator color.
    pub mode_intent: Color,
    /// Review mode indicator color.
    pub mode_review: Color,
    /// Selection background.
    pub selection_bg: Color,
    /// Selection foreground.
    pub selection_fg: Color,
    /// Git added marker.
    pub git_added: Color,
    /// Git modified marker.
    pub git_modified: Color,
    /// Git deleted marker.
    pub git_deleted: Color,
    /// Error marker.
    pub error: Color,
    /// Warning marker.
    pub warning: Color,
    /// Info marker.
    pub info: Color,
    /// Keyword color (syntax).
    pub keyword: Color,
    /// String literal color (syntax).
    pub string: Color,
    /// Comment color (syntax).
    pub comment: Color,
    /// Function name color (syntax).
    pub function: Color,
    /// Type name color (syntax).
    pub type_name: Color,
    /// Number literal color (syntax).
    pub number: Color,
    /// Ghost text / suggestion color.
    pub ghost: Color,
    /// Human authorship marker.
    pub author_human: Color,
    /// AI authorship marker.
    pub author_ai: Color,
}

/// Built-in dark theme (default).
pub fn theme_dark() -> Theme {
    Theme {
        name: "dark".to_string(),
        bg: Color::Reset,
        fg: Color::White,
        gutter_fg: Color::DarkGray,
        status_bg: Color::DarkGray,
        status_fg: Color::White,
        mode_normal: Color::Blue,
        mode_insert: Color::Green,
        mode_command: Color::Yellow,
        mode_visual: Color::Magenta,
        mode_intent: Color::Cyan,
        mode_review: Color::LightRed,
        selection_bg: Color::DarkGray,
        selection_fg: Color::White,
        git_added: Color::Green,
        git_modified: Color::Yellow,
        git_deleted: Color::Red,
        error: Color::Red,
        warning: Color::Yellow,
        info: Color::Cyan,
        keyword: Color::Magenta,
        string: Color::Green,
        comment: Color::Rgb(100, 100, 100),
        function: Color::Blue,
        type_name: Color::Yellow,
        number: Color::Cyan,
        ghost: Color::DarkGray,
        author_human: Color::Green,
        author_ai: Color::Blue,
    }
}

/// Built-in light theme.
pub fn theme_light() -> Theme {
    Theme {
        name: "light".to_string(),
        bg: Color::White,
        fg: Color::Black,
        gutter_fg: Color::Gray,
        status_bg: Color::Gray,
        status_fg: Color::Black,
        mode_normal: Color::Blue,
        mode_insert: Color::Green,
        mode_command: Color::Yellow,
        mode_visual: Color::Magenta,
        mode_intent: Color::Cyan,
        mode_review: Color::Red,
        selection_bg: Color::LightBlue,
        selection_fg: Color::Black,
        git_added: Color::Green,
        git_modified: Color::Yellow,
        git_deleted: Color::Red,
        error: Color::Red,
        warning: Color::Yellow,
        info: Color::Blue,
        keyword: Color::Magenta,
        string: Color::Green,
        comment: Color::Gray,
        function: Color::Blue,
        type_name: Color::Rgb(180, 120, 0),
        number: Color::Cyan,
        ghost: Color::Gray,
        author_human: Color::Green,
        author_ai: Color::Blue,
    }
}

/// Built-in Monokai-inspired theme.
pub fn theme_monokai() -> Theme {
    Theme {
        name: "monokai".to_string(),
        bg: Color::Rgb(39, 40, 34),
        fg: Color::Rgb(248, 248, 242),
        gutter_fg: Color::Rgb(117, 113, 94),
        status_bg: Color::Rgb(73, 72, 62),
        status_fg: Color::Rgb(248, 248, 242),
        mode_normal: Color::Rgb(102, 217, 239),
        mode_insert: Color::Rgb(166, 226, 46),
        mode_command: Color::Rgb(230, 219, 116),
        mode_visual: Color::Rgb(174, 129, 255),
        mode_intent: Color::Rgb(102, 217, 239),
        mode_review: Color::Rgb(249, 38, 114),
        selection_bg: Color::Rgb(73, 72, 62),
        selection_fg: Color::Rgb(248, 248, 242),
        git_added: Color::Rgb(166, 226, 46),
        git_modified: Color::Rgb(230, 219, 116),
        git_deleted: Color::Rgb(249, 38, 114),
        error: Color::Rgb(249, 38, 114),
        warning: Color::Rgb(230, 219, 116),
        info: Color::Rgb(102, 217, 239),
        keyword: Color::Rgb(249, 38, 114),
        string: Color::Rgb(230, 219, 116),
        comment: Color::Rgb(117, 113, 94),
        function: Color::Rgb(166, 226, 46),
        type_name: Color::Rgb(102, 217, 239),
        number: Color::Rgb(174, 129, 255),
        ghost: Color::Rgb(117, 113, 94),
        author_human: Color::Rgb(166, 226, 46),
        author_ai: Color::Rgb(102, 217, 239),
    }
}

/// Parse a color string (name or "#RRGGBB") into a ratatui Color.
fn parse_color(s: &str) -> Option<Color> {
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "reset" | "default" => Some(Color::Reset),
        hex if hex.starts_with('#') && hex.len() == 7 => {
            let r = u8::from_str_radix(&hex[1..3], 16).ok()?;
            let g = u8::from_str_radix(&hex[3..5], 16).ok()?;
            let b = u8::from_str_radix(&hex[5..7], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// Load a theme from TOML table, falling back to the base theme for unset values.
fn theme_from_toml(table: &toml::Table, base: &Theme) -> Theme {
    let mut theme = base.clone();

    macro_rules! set_color {
        ($field:ident, $key:expr) => {
            if let Some(val) = table.get($key).and_then(|v| v.as_str()) {
                if let Some(color) = parse_color(val) {
                    theme.$field = color;
                }
            }
        };
    }

    if let Some(name) = table.get("name").and_then(|v| v.as_str()) {
        theme.name = name.to_string();
    }

    set_color!(bg, "bg");
    set_color!(fg, "fg");
    set_color!(gutter_fg, "gutter_fg");
    set_color!(status_bg, "status_bg");
    set_color!(status_fg, "status_fg");
    set_color!(selection_bg, "selection_bg");
    set_color!(selection_fg, "selection_fg");
    set_color!(git_added, "git_added");
    set_color!(git_modified, "git_modified");
    set_color!(git_deleted, "git_deleted");
    set_color!(error, "error");
    set_color!(warning, "warning");
    set_color!(info, "info");
    set_color!(keyword, "keyword");
    set_color!(string, "string");
    set_color!(comment, "comment");
    set_color!(function, "function");
    set_color!(type_name, "type_name");
    set_color!(number, "number");
    set_color!(ghost, "ghost");
    set_color!(author_human, "author_human");
    set_color!(author_ai, "author_ai");

    theme
}

/// Resolve a theme by name. Checks built-in themes first, then looks
/// for a `[theme.<name>]` section in the config.
pub fn resolve_theme(name: &str, config_table: Option<&toml::Table>) -> Theme {
    match name {
        "dark" | "default" => theme_dark(),
        "light" => theme_light(),
        "monokai" => theme_monokai(),
        custom => {
            // Try to load from config's [theme.custom] section.
            if let Some(table) = config_table {
                if let Some(theme_section) = table
                    .get("theme_definitions")
                    .and_then(|t| t.as_table())
                    .and_then(|t| t.get(custom))
                    .and_then(|t| t.as_table())
                {
                    return theme_from_toml(theme_section, &theme_dark());
                }
            }
            // Fallback to dark.
            tracing::warn!("Unknown theme '{custom}', falling back to dark");
            theme_dark()
        }
    }
}

/// Load configuration from an `aura.toml` file.
pub fn load_config(path: &Path) -> AuraConfig {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return AuraConfig::default(),
    };

    match toml::from_str(&content) {
        Ok(config) => config,
        Err(e) => {
            tracing::warn!("Failed to parse {}: {}", path.display(), e);
            AuraConfig::default()
        }
    }
}

/// Load the raw TOML table (needed for custom theme resolution).
pub fn load_config_table(path: &Path) -> Option<toml::Table> {
    let content = std::fs::read_to_string(path).ok()?;
    content.parse().ok()
}

/// Per-file settings resolved from `.editorconfig` files.
#[derive(Debug, Clone)]
pub struct EditorConfigResult {
    /// Indent style: "space" or "tab".
    pub indent_style: Option<String>,
    /// Indent size (number of spaces or tab width).
    pub indent_size: Option<usize>,
    /// Whether to trim trailing whitespace on save.
    pub trim_trailing_whitespace: Option<bool>,
    /// Whether to insert a final newline on save.
    pub insert_final_newline: Option<bool>,
}

/// Look up `.editorconfig` settings for a file path.
///
/// Walks up from the file's directory, reading `.editorconfig` files and
/// accumulating matching properties. Stops when `root = true` is found.
pub fn lookup_editorconfig(file_path: &Path) -> EditorConfigResult {
    let mut result = EditorConfigResult {
        indent_style: None,
        indent_size: None,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
    };

    let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return result,
    };

    let mut dir = file_path.parent();
    while let Some(d) = dir {
        let ec_path = d.join(".editorconfig");
        if ec_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&ec_path) {
                let (is_root, props) = parse_editorconfig(&content, &file_name);
                // Apply properties (first match wins, so don't overwrite).
                if result.indent_style.is_none() {
                    result.indent_style = props.indent_style;
                }
                if result.indent_size.is_none() {
                    result.indent_size = props.indent_size;
                }
                if result.trim_trailing_whitespace.is_none() {
                    result.trim_trailing_whitespace = props.trim_trailing_whitespace;
                }
                if result.insert_final_newline.is_none() {
                    result.insert_final_newline = props.insert_final_newline;
                }
                if is_root {
                    break;
                }
            }
        }
        dir = d.parent();
    }

    result
}

/// Parse an `.editorconfig` file and return properties matching a filename.
///
/// Returns `(is_root, properties)`.
fn parse_editorconfig(content: &str, filename: &str) -> (bool, EditorConfigResult) {
    let mut is_root = false;
    let mut result = EditorConfigResult {
        indent_style: None,
        indent_size: None,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
    };

    let mut section_matches = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // Root directive.
        if line.to_lowercase().starts_with("root") {
            if let Some(val) = line.split('=').nth(1) {
                if val.trim().eq_ignore_ascii_case("true") {
                    is_root = true;
                }
            }
            continue;
        }

        // Section header: [pattern]
        if line.starts_with('[') && line.ends_with(']') {
            let pattern = &line[1..line.len() - 1];
            section_matches = editorconfig_glob_matches(pattern, filename);
            continue;
        }

        // Key = value (only if current section matches).
        if !section_matches {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let val = val.trim().to_lowercase();
            match key.as_str() {
                "indent_style" => result.indent_style = Some(val),
                "indent_size" => {
                    if let Ok(n) = val.parse::<usize>() {
                        result.indent_size = Some(n);
                    }
                }
                "trim_trailing_whitespace" => {
                    result.trim_trailing_whitespace = Some(val == "true");
                }
                "insert_final_newline" => {
                    result.insert_final_newline = Some(val == "true");
                }
                _ => {}
            }
        }
    }

    (is_root, result)
}

/// Simple glob matching for editorconfig patterns.
///
/// Supports `*` (any chars except `/`), `*.ext`, `{a,b}` alternatives.
fn editorconfig_glob_matches(pattern: &str, filename: &str) -> bool {
    // Handle brace alternatives: {*.rs,*.toml}
    if pattern.contains('{') && pattern.contains('}') {
        if let Some(start) = pattern.find('{') {
            if let Some(end) = pattern.find('}') {
                let prefix = &pattern[..start];
                let suffix = &pattern[end + 1..];
                let alternatives = &pattern[start + 1..end];
                return alternatives.split(',').any(|alt| {
                    let full = format!("{prefix}{alt}{suffix}");
                    editorconfig_glob_matches(&full, filename)
                });
            }
        }
    }

    // Simple star matching.
    if pattern == "*" {
        return true;
    }
    if let Some(ext) = pattern.strip_prefix("*.") {
        return filename.ends_with(&format!(".{ext}"));
    }
    if pattern.contains('*') {
        // Split on * and check prefix/suffix.
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            return filename.starts_with(parts[0]) && filename.ends_with(parts[1]);
        }
    }

    // Exact match.
    pattern == filename
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AuraConfig::default();
        assert_eq!(config.theme, "dark");
        assert_eq!(config.editor.tab_width, 4);
        assert!(config.editor.line_numbers);
    }

    #[test]
    fn test_parse_color_names() {
        assert_eq!(parse_color("red"), Some(Color::Red));
        assert_eq!(parse_color("Blue"), Some(Color::Blue));
        assert_eq!(parse_color("DarkGray"), Some(Color::DarkGray));
        assert_eq!(parse_color("reset"), Some(Color::Reset));
    }

    #[test]
    fn test_parse_color_hex() {
        assert_eq!(parse_color("#FF0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("#00ff00"), Some(Color::Rgb(0, 255, 0)));
        assert_eq!(parse_color("#2728ff"), Some(Color::Rgb(39, 40, 255)));
    }

    #[test]
    fn test_parse_color_invalid() {
        assert_eq!(parse_color("nope"), None);
        assert_eq!(parse_color("#ZZ0000"), None);
        assert_eq!(parse_color("#FF"), None);
    }

    #[test]
    fn test_resolve_builtin_themes() {
        let dark = resolve_theme("dark", None);
        assert_eq!(dark.name, "dark");

        let light = resolve_theme("light", None);
        assert_eq!(light.name, "light");

        let monokai = resolve_theme("monokai", None);
        assert_eq!(monokai.name, "monokai");
    }

    #[test]
    fn test_resolve_unknown_theme_fallback() {
        let theme = resolve_theme("nonexistent", None);
        assert_eq!(theme.name, "dark"); // fallback
    }

    #[test]
    fn test_theme_from_toml() {
        let toml_str = r##"
name = "custom"
bg = "#1e1e2e"
fg = "#cdd6f4"
keyword = "magenta"
"##;
        let table: toml::Table = toml_str.parse().unwrap();
        let theme = theme_from_toml(&table, &theme_dark());
        assert_eq!(theme.name, "custom");
        assert_eq!(theme.bg, Color::Rgb(30, 30, 46));
        assert_eq!(theme.keyword, Color::Magenta);
        // Unset values fall back to dark theme.
        assert_eq!(theme.string, theme_dark().string);
    }

    #[test]
    fn test_parse_config_toml() {
        let toml_str = r#"
theme = "monokai"

[editor]
tab_width = 2
spaces_for_tabs = true

[ai]
model = "claude-opus-4-20250514"
aggressiveness = "proactive"

[keybindings]
leader = "Space"

[keybindings.leader_map]
e = "explain"
f = "fix"
"#;
        let config: AuraConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.theme, "monokai");
        assert_eq!(config.editor.tab_width, 2);
        assert_eq!(config.ai.model, "claude-opus-4-20250514");
        assert_eq!(config.ai.aggressiveness, "proactive");
        assert_eq!(
            config.keybindings.leader_map.get("e"),
            Some(&"explain".to_string())
        );
    }

    #[test]
    fn test_load_nonexistent_config() {
        let config = load_config(Path::new("/nonexistent/aura.toml"));
        assert_eq!(config.theme, "dark"); // default
    }

    #[test]
    fn test_format_key_simple() {
        use crossterm::event::{KeyCode, KeyModifiers};
        assert_eq!(
            format_key(KeyCode::Char('j'), KeyModifiers::CONTROL),
            "ctrl+j"
        );
        assert_eq!(
            format_key(
                KeyCode::Char('g'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            "ctrl+shift+g"
        );
        assert_eq!(format_key(KeyCode::F(5), KeyModifiers::empty()), "f5");
        assert_eq!(format_key(KeyCode::Esc, KeyModifiers::empty()), "esc");
        assert_eq!(format_key(KeyCode::Char('a'), KeyModifiers::ALT), "alt+a");
    }

    #[test]
    fn test_keybinding_is_leader_key() {
        use crossterm::event::KeyCode;
        let default_config = KeybindingConfig::default();
        assert!(default_config.is_leader_key(KeyCode::Char(' ')));
        assert!(!default_config.is_leader_key(KeyCode::Char('a')));

        let backslash_config = KeybindingConfig {
            leader: Some("Backslash".into()),
            ..Default::default()
        };
        assert!(backslash_config.is_leader_key(KeyCode::Char('\\')));
        assert!(!backslash_config.is_leader_key(KeyCode::Char(' ')));
    }

    #[test]
    fn test_keybinding_global_action() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let mut config = KeybindingConfig::default();
        config
            .global_map
            .insert("ctrl+k".into(), "open_command_palette".into());

        assert_eq!(
            config.global_action(KeyCode::Char('k'), KeyModifiers::CONTROL),
            Some("open_command_palette")
        );
        assert_eq!(
            config.global_action(KeyCode::Char('z'), KeyModifiers::CONTROL),
            None
        );
    }

    #[test]
    fn test_keybinding_leader_action() {
        let mut config = KeybindingConfig::default();
        config
            .leader_map
            .insert("x".into(), "open_git_graph".into());

        assert_eq!(config.leader_action('x'), Some("open_git_graph"));
        assert_eq!(config.leader_action('y'), None);
    }

    #[test]
    fn test_editorconfig_glob_star() {
        assert!(editorconfig_glob_matches("*", "anything.rs"));
        assert!(editorconfig_glob_matches("*.rs", "main.rs"));
        assert!(!editorconfig_glob_matches("*.rs", "main.py"));
    }

    #[test]
    fn test_editorconfig_glob_braces() {
        assert!(editorconfig_glob_matches("{*.rs,*.toml}", "Cargo.toml"));
        assert!(editorconfig_glob_matches("{*.rs,*.toml}", "main.rs"));
        assert!(!editorconfig_glob_matches("{*.rs,*.toml}", "main.py"));
    }

    #[test]
    fn test_parse_editorconfig() {
        let content = "\
root = true

[*]
indent_style = space
indent_size = 4
trim_trailing_whitespace = true

[*.rs]
indent_size = 2
";
        let (is_root, result) = parse_editorconfig(content, "main.rs");
        assert!(is_root);
        // *.rs overrides indent_size; [*] provides indent_style.
        assert_eq!(result.indent_size, Some(2));
        assert_eq!(result.indent_style.as_deref(), Some("space"));
    }

    #[test]
    fn test_parse_editorconfig_general_section() {
        let content = "[*]\nindent_style = tab\ninsert_final_newline = true\n";
        let (_, result) = parse_editorconfig(content, "anything.txt");
        assert_eq!(result.indent_style.as_deref(), Some("tab"));
        assert_eq!(result.insert_final_newline, Some(true));
    }
}
