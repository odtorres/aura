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
        }
    }
}

/// AI-related settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    /// AI model to use.
    pub model: String,
    /// Maximum tokens for AI responses.
    pub max_tokens: u32,
    /// Speculative analysis aggressiveness: "minimal", "moderate", "proactive".
    pub aggressiveness: String,
    /// Idle time (ms) before triggering speculative analysis.
    pub idle_threshold_ms: u64,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            aggressiveness: "moderate".to_string(),
            idle_threshold_ms: 3000,
        }
    }
}

/// Keybinding overrides. Maps action names to key descriptions.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct KeybindingConfig {
    /// Leader key (default: Space).
    pub leader: Option<String>,
    /// Custom leader key mappings: action_name → key.
    #[serde(default)]
    pub leader_map: HashMap<String, String>,
    /// Custom normal mode mappings: key → action.
    #[serde(default)]
    pub normal_map: HashMap<String, String>,
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
}
