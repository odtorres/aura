//! Plugin system foundation for AURA.
//!
//! Provides a trait-based plugin API that allows extending the editor with
//! custom behaviour for key handling, file save events, and AI intent
//! augmentation. This is the Phase 8.3 foundation — full Lua/WASM runtimes
//! are deferred to a later phase.

/// An action that a plugin can request the editor perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginAction {
    /// Execute an editor command (e.g. `"w"`, `"q"`).
    RunCommand(String),
    /// Insert text at the current cursor position.
    InsertText(String),
    /// Display a message in the status bar.
    SetStatus(String),
    /// No action requested.
    None,
}

/// Core plugin interface.
///
/// Implement this trait and register an instance with [`PluginManager`] to
/// hook into editor events. All implementations must be `Send + Sync` so they
/// can be used across threads.
pub trait Plugin: Send + Sync {
    /// Human-readable name that identifies this plugin.
    fn name(&self) -> &str;

    /// Called once immediately after the plugin is registered.
    ///
    /// Use this to perform one-time initialisation (open files, set up
    /// channels, etc.). Return an error to prevent the plugin from being
    /// activated.
    fn on_load(&mut self) -> anyhow::Result<()>;

    /// Called for every keypress in any editing mode.
    ///
    /// `mode` is the current mode label (e.g. `"NORMAL"`, `"INSERT"`).
    /// `key` is a short string representation of the key (e.g. `"j"`,
    /// `"<CR>"`, `"<C-s>"`).
    ///
    /// Return `Some(action)` to request an editor action, or `None` to let
    /// normal handling proceed. Returning `Some(PluginAction::None)` is
    /// equivalent to returning `None`.
    fn on_key(&mut self, mode: &str, key: &str) -> Option<PluginAction>;

    /// Called after a buffer has been successfully written to disk.
    ///
    /// `path` is the absolute path of the saved file. Errors are logged but
    /// do not interrupt the save.
    fn on_save(&mut self, path: &str) -> anyhow::Result<()>;

    /// Called before an intent string is sent to the AI.
    ///
    /// Return `Some(modified_intent)` to replace the intent that will be sent,
    /// or `None` to leave it unchanged.
    fn on_intent(&mut self, intent: &str) -> Option<String>;
}

/// Manages the collection of active plugins and routes editor events to them.
pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginManager {
    /// Create a new, empty plugin manager.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Register a plugin.
    ///
    /// `on_load` is called immediately. If it returns an error the plugin is
    /// not added and the error is logged.
    pub fn register(&mut self, mut plugin: Box<dyn Plugin>) {
        match plugin.on_load() {
            Ok(()) => {
                tracing::info!("Plugin loaded: {}", plugin.name());
                self.plugins.push(plugin);
            }
            Err(e) => {
                tracing::error!("Plugin '{}' failed to load: {}", plugin.name(), e);
            }
        }
    }

    /// Notify all plugins of a keypress.
    ///
    /// Returns the collected [`PluginAction`]s from every plugin that returned
    /// `Some`. `PluginAction::None` values are filtered out.
    pub fn notify_key(&mut self, mode: &str, key: &str) -> Vec<PluginAction> {
        self.plugins
            .iter_mut()
            .filter_map(|p| p.on_key(mode, key))
            .filter(|a| a != &PluginAction::None)
            .collect()
    }

    /// Notify all plugins that a file was saved.
    ///
    /// Errors from individual plugins are logged and do not stop other plugins
    /// from being notified.
    pub fn notify_save(&mut self, path: &str) {
        for plugin in &mut self.plugins {
            if let Err(e) = plugin.on_save(path) {
                tracing::warn!("Plugin '{}' error in on_save: {}", plugin.name(), e);
            }
        }
    }

    /// Pass an intent through all plugins, allowing them to augment it.
    ///
    /// Plugins are visited in registration order. Each plugin may modify the
    /// intent string; the (potentially modified) string is passed to the next
    /// plugin. Returns the final intent, or `None` if no plugin modified it.
    pub fn notify_intent(&mut self, intent: &str) -> Option<String> {
        let mut current: Option<String> = None;
        for plugin in &mut self.plugins {
            let input = current.as_deref().unwrap_or(intent);
            if let Some(modified) = plugin.on_intent(input) {
                current = Some(modified);
            }
        }
        current
    }

    /// Return the names of all currently loaded plugins.
    pub fn plugin_names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoPlugin {
        loaded: bool,
        last_key: Option<String>,
        saved_paths: Vec<String>,
    }

    impl EchoPlugin {
        fn new() -> Self {
            Self {
                loaded: false,
                last_key: None,
                saved_paths: Vec::new(),
            }
        }
    }

    impl Plugin for EchoPlugin {
        fn name(&self) -> &str {
            "echo"
        }

        fn on_load(&mut self) -> anyhow::Result<()> {
            self.loaded = true;
            Ok(())
        }

        fn on_key(&mut self, _mode: &str, key: &str) -> Option<PluginAction> {
            self.last_key = Some(key.to_string());
            None
        }

        fn on_save(&mut self, path: &str) -> anyhow::Result<()> {
            self.saved_paths.push(path.to_string());
            Ok(())
        }

        fn on_intent(&mut self, _intent: &str) -> Option<String> {
            None
        }
    }

    struct StatusPlugin;

    impl Plugin for StatusPlugin {
        fn name(&self) -> &str {
            "status-setter"
        }

        fn on_load(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn on_key(&mut self, _mode: &str, key: &str) -> Option<PluginAction> {
            if key == "!" {
                Some(PluginAction::SetStatus("bang!".to_string()))
            } else {
                None
            }
        }

        fn on_save(&mut self, _path: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn on_intent(&mut self, _intent: &str) -> Option<String> {
            None
        }
    }

    struct IntentPrefixPlugin;

    impl Plugin for IntentPrefixPlugin {
        fn name(&self) -> &str {
            "intent-prefix"
        }

        fn on_load(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn on_key(&mut self, _mode: &str, _key: &str) -> Option<PluginAction> {
            None
        }

        fn on_save(&mut self, _path: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn on_intent(&mut self, intent: &str) -> Option<String> {
            Some(format!("[prefixed] {}", intent))
        }
    }

    struct FailingLoadPlugin;

    impl Plugin for FailingLoadPlugin {
        fn name(&self) -> &str {
            "failing-load"
        }

        fn on_load(&mut self) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("intentional load failure"))
        }

        fn on_key(&mut self, _mode: &str, _key: &str) -> Option<PluginAction> {
            None
        }

        fn on_save(&mut self, _path: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn on_intent(&mut self, _intent: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn new_manager_has_no_plugins() {
        let mgr = PluginManager::new();
        assert!(mgr.plugin_names().is_empty());
    }

    #[test]
    fn register_calls_on_load() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(EchoPlugin::new()));
        assert_eq!(mgr.plugin_names(), vec!["echo"]);
    }

    #[test]
    fn failed_load_does_not_add_plugin() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(FailingLoadPlugin));
        assert!(mgr.plugin_names().is_empty());
    }

    #[test]
    fn notify_key_returns_actions() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(StatusPlugin));

        let actions = mgr.notify_key("NORMAL", "j");
        assert!(actions.is_empty());

        let actions = mgr.notify_key("NORMAL", "!");
        assert_eq!(actions, vec![PluginAction::SetStatus("bang!".to_string())]);
    }

    #[test]
    fn notify_key_filters_plugin_action_none() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(EchoPlugin::new()));
        // EchoPlugin always returns None from on_key.
        let actions = mgr.notify_key("NORMAL", "j");
        assert!(actions.is_empty());
    }

    #[test]
    fn notify_save_calls_all_plugins() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(EchoPlugin::new()));
        mgr.notify_save("/tmp/test.rs");
        // No panic — success.
    }

    #[test]
    fn notify_intent_chains_plugins() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(IntentPrefixPlugin));
        let result = mgr.notify_intent("refactor this");
        assert_eq!(result, Some("[prefixed] refactor this".to_string()));
    }

    #[test]
    fn notify_intent_no_modification_returns_none() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(EchoPlugin::new()));
        // EchoPlugin returns None from on_intent.
        let result = mgr.notify_intent("refactor this");
        assert!(result.is_none());
    }

    #[test]
    fn plugin_names_reflects_registered_plugins() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(EchoPlugin::new()));
        mgr.register(Box::new(StatusPlugin));
        let names = mgr.plugin_names();
        assert_eq!(names, vec!["echo", "status-setter"]);
    }

    #[test]
    fn default_equals_new() {
        let mgr = PluginManager::default();
        assert!(mgr.plugin_names().is_empty());
    }
}
