//! Interactive settings modal overlay.
//!
//! Provides a centered popup where users can toggle editor settings
//! in real-time. Changes take effect immediately and are reflected
//! in the running editor.

/// A single setting entry in the modal.
#[derive(Debug, Clone)]
pub struct SettingEntry {
    /// Display label.
    pub label: String,
    /// Setting key (used to identify which config field to modify).
    pub key: String,
    /// Current value type and state.
    pub value: SettingValue,
}

/// Value types for settings.
#[derive(Debug, Clone)]
pub enum SettingValue {
    /// Boolean toggle (on/off).
    Bool(bool),
    /// Numeric value with optional min/max.
    Number {
        /// Current value.
        current: u64,
        /// Minimum allowed value.
        min: u64,
        /// Maximum allowed value.
        max: u64,
    },
}

/// The settings modal state.
pub struct SettingsModal {
    /// Whether the modal is visible.
    pub visible: bool,
    /// All settings entries.
    pub entries: Vec<SettingEntry>,
    /// Currently selected entry index.
    pub selected: usize,
}

impl SettingsModal {
    /// Create a new settings modal (initially hidden).
    pub fn new() -> Self {
        Self {
            visible: false,
            entries: Vec::new(),
            selected: 0,
        }
    }

    /// Open the modal, refreshing settings from the current config.
    pub fn open(&mut self, config: &crate::config::AuraConfig) {
        self.entries = vec![
            SettingEntry {
                label: "Show Minimap".to_string(),
                key: "editor.show_minimap".to_string(),
                value: SettingValue::Bool(config.editor.show_minimap),
            },
            SettingEntry {
                label: "Show Line Numbers".to_string(),
                key: "editor.line_numbers".to_string(),
                value: SettingValue::Bool(config.editor.line_numbers),
            },
            SettingEntry {
                label: "Show Authorship".to_string(),
                key: "editor.show_authorship".to_string(),
                value: SettingValue::Bool(config.editor.show_authorship),
            },
            SettingEntry {
                label: "Spaces for Tabs".to_string(),
                key: "editor.spaces_for_tabs".to_string(),
                value: SettingValue::Bool(config.editor.spaces_for_tabs),
            },
            SettingEntry {
                label: "Tab Width".to_string(),
                key: "editor.tab_width".to_string(),
                value: SettingValue::Number {
                    current: config.editor.tab_width as u64,
                    min: 1,
                    max: 8,
                },
            },
            SettingEntry {
                label: "Scroll Margin".to_string(),
                key: "editor.scroll_margin".to_string(),
                value: SettingValue::Number {
                    current: config.editor.scroll_margin as u64,
                    min: 0,
                    max: 20,
                },
            },
            SettingEntry {
                label: "Auto-Compact Conversations".to_string(),
                key: "conversations.auto_compact".to_string(),
                value: SettingValue::Bool(config.conversations.auto_compact),
            },
            SettingEntry {
                label: "Max Context Messages".to_string(),
                key: "conversations.max_context_messages".to_string(),
                value: SettingValue::Number {
                    current: config.conversations.max_context_messages as u64,
                    min: 10,
                    max: 200,
                },
            },
            SettingEntry {
                label: "Check for Updates".to_string(),
                key: "update.check_for_updates".to_string(),
                value: SettingValue::Bool(config.update.check_for_updates),
            },
        ];
        self.selected = 0;
        self.visible = true;
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Move selection down.
    pub fn select_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    /// Move selection up.
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Toggle or increment the selected setting.
    pub fn toggle_selected(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.selected) {
            match &mut entry.value {
                SettingValue::Bool(b) => *b = !*b,
                SettingValue::Number { current, max, .. } => {
                    *current = (*current + 1).min(*max);
                }
            }
        }
    }

    /// Decrement the selected numeric setting.
    pub fn decrement_selected(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.selected) {
            if let SettingValue::Number { current, min, .. } = &mut entry.value {
                *current = current.saturating_sub(1).max(*min);
            }
        }
    }

    /// Apply the current modal state back to the config.
    pub fn apply_to_config(&self, config: &mut crate::config::AuraConfig) {
        for entry in &self.entries {
            match (entry.key.as_str(), &entry.value) {
                ("editor.show_minimap", SettingValue::Bool(v)) => {
                    config.editor.show_minimap = *v;
                }
                ("editor.line_numbers", SettingValue::Bool(v)) => {
                    config.editor.line_numbers = *v;
                }
                ("editor.show_authorship", SettingValue::Bool(v)) => {
                    config.editor.show_authorship = *v;
                }
                ("editor.spaces_for_tabs", SettingValue::Bool(v)) => {
                    config.editor.spaces_for_tabs = *v;
                }
                ("editor.tab_width", SettingValue::Number { current, .. }) => {
                    config.editor.tab_width = *current as usize;
                }
                ("editor.scroll_margin", SettingValue::Number { current, .. }) => {
                    config.editor.scroll_margin = *current as usize;
                }
                ("conversations.auto_compact", SettingValue::Bool(v)) => {
                    config.conversations.auto_compact = *v;
                }
                ("conversations.max_context_messages", SettingValue::Number { current, .. }) => {
                    config.conversations.max_context_messages = *current as usize;
                }
                ("update.check_for_updates", SettingValue::Bool(v)) => {
                    config.update.check_for_updates = *v;
                }
                _ => {}
            }
        }
    }
}

impl Default for SettingsModal {
    fn default() -> Self {
        Self::new()
    }
}
