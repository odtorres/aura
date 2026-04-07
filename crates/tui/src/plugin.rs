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
    /// Read the current buffer content (returned via callback).
    ReadBuffer,
    /// Get the cursor position (row, col).
    GetCursor,
    /// Replace the entire buffer content.
    SetBuffer(String),
    /// Get the current file path.
    GetFilePath,
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

    /// Update the plugin with current editor state.
    ///
    /// Called before each event dispatch so plugins have access to buffer
    /// content, cursor position, etc.
    fn update_state(&mut self, _state: &PluginEditorState) {}

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

// ---------------------------------------------------------------------------
// Lua plugin bridge
// ---------------------------------------------------------------------------

/// Shared editor state accessible to Lua plugins.
///
/// Updated by the plugin manager before each callback invocation.
#[derive(Debug, Clone, Default)]
pub struct PluginEditorState {
    /// Current buffer content.
    pub buffer_content: String,
    /// Current file path (empty for scratch buffers).
    pub file_path: String,
    /// Cursor row (0-indexed).
    pub cursor_row: usize,
    /// Cursor column (0-indexed).
    pub cursor_col: usize,
    /// Current editing mode label.
    pub mode: String,
    /// Number of lines in the buffer.
    pub line_count: usize,
    /// Current line text (under cursor).
    pub current_line: String,
}

/// A plugin loaded from a Lua script file.
///
/// Each `LuaPlugin` owns its own Lua VM instance. The Lua script must define
/// a global table called `plugin` with at least a `name` field. Optional
/// callback functions: `on_load()`, `on_key(mode, key)`, `on_save(path)`,
/// `on_intent(intent)`.
pub struct LuaPlugin {
    plugin_name: String,
    lua: std::sync::Mutex<mlua::Lua>,
}

impl LuaPlugin {
    /// Load a Lua plugin from a file path.
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let lua = mlua::Lua::new();
        let source = std::fs::read_to_string(path)?;

        lua.load(&source)
            .set_name(path.display().to_string())
            .exec()
            .map_err(|e| anyhow::anyhow!("Lua load error in {}: {e}", path.display()))?;

        let name: String = lua
            .globals()
            .get::<mlua::Table>("plugin")
            .and_then(|t| t.get::<String>("name"))
            .map_err(|e| {
                anyhow::anyhow!("Lua plugin {} must define plugin.name: {e}", path.display())
            })?;

        Ok(Self {
            plugin_name: name,
            lua: std::sync::Mutex::new(lua),
        })
    }

    /// Update the `editor` global table in Lua with current state.
    ///
    /// Plugins can access `editor.buffer`, `editor.file_path`, `editor.cursor_row`,
    /// `editor.cursor_col`, `editor.mode`, `editor.line_count`, `editor.current_line`.
    pub fn update_state(&self, state: &PluginEditorState) {
        let lua = match self.lua.lock() {
            Ok(l) => l,
            Err(_) => return,
        };
        if let Ok(table) = lua.create_table() {
            let _ = table.set("buffer", state.buffer_content.clone());
            let _ = table.set("file_path", state.file_path.clone());
            let _ = table.set("cursor_row", state.cursor_row);
            let _ = table.set("cursor_col", state.cursor_col);
            let _ = table.set("mode", state.mode.clone());
            let _ = table.set("line_count", state.line_count);
            let _ = table.set("current_line", state.current_line.clone());
            let _ = lua.globals().set("editor", table);
        }
    }

    /// Call a Lua function with a single string arg, no return.
    fn call_void_str(&self, func_name: &str, arg: &str) {
        let lua = match self.lua.lock() {
            Ok(l) => l,
            Err(_) => return,
        };
        if let Ok(table) = lua.globals().get::<mlua::Table>("plugin") {
            if let Ok(func) = table.get::<mlua::Function>(func_name) {
                if let Err(e) = func.call::<()>(arg.to_string()) {
                    tracing::warn!("Lua plugin '{}' {func_name}() error: {e}", self.plugin_name);
                }
            }
        }
    }

    /// Call a Lua function with two string args, return optional string.
    fn call_str2_ret(&self, func_name: &str, arg1: &str, arg2: &str) -> Option<String> {
        let lua = self.lua.lock().ok()?;
        let table = lua.globals().get::<mlua::Table>("plugin").ok()?;
        let func = table.get::<mlua::Function>(func_name).ok()?;
        match func.call::<Option<String>>((arg1.to_string(), arg2.to_string())) {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("Lua plugin '{}' {func_name}() error: {e}", self.plugin_name);
                None
            }
        }
    }

    /// Call a Lua function with one string arg, return optional string.
    fn call_str1_ret(&self, func_name: &str, arg: &str) -> Option<String> {
        let lua = self.lua.lock().ok()?;
        let table = lua.globals().get::<mlua::Table>("plugin").ok()?;
        let func = table.get::<mlua::Function>(func_name).ok()?;
        match func.call::<Option<String>>(arg.to_string()) {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("Lua plugin '{}' {func_name}() error: {e}", self.plugin_name);
                None
            }
        }
    }
}

impl Plugin for LuaPlugin {
    fn name(&self) -> &str {
        &self.plugin_name
    }

    fn update_state(&mut self, state: &PluginEditorState) {
        // Delegate to the inherent method.
        LuaPlugin::update_state(self, state);
    }

    fn on_load(&mut self) -> anyhow::Result<()> {
        // Call on_load with no args.
        let lua = self
            .lua
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        if let Ok(table) = lua.globals().get::<mlua::Table>("plugin") {
            if let Ok(func) = table.get::<mlua::Function>("on_load") {
                func.call::<()>(())
                    .map_err(|e| anyhow::anyhow!("on_load error: {e}"))?;
            }
        }
        Ok(())
    }

    fn on_key(&mut self, mode: &str, key: &str) -> Option<PluginAction> {
        let result = self.call_str2_ret("on_key", mode, key)?;
        // Parse the return string as an action.
        if let Some(cmd) = result.strip_prefix("cmd:") {
            Some(PluginAction::RunCommand(cmd.to_string()))
        } else if let Some(text) = result.strip_prefix("insert:") {
            Some(PluginAction::InsertText(text.to_string()))
        } else {
            result
                .strip_prefix("status:")
                .map(|msg| PluginAction::SetStatus(msg.to_string()))
        }
    }

    fn on_save(&mut self, path: &str) -> anyhow::Result<()> {
        self.call_void_str("on_save", path);
        Ok(())
    }

    fn on_intent(&mut self, intent: &str) -> Option<String> {
        self.call_str1_ret("on_intent", intent)
    }
}

/// Discover and load all `.lua` plugins from `~/.aura/plugins/`.
pub fn discover_lua_plugins() -> Vec<Box<dyn Plugin>> {
    let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();

    let plugins_dir = dirs_path_home().map(|h| h.join(".aura").join("plugins"));

    let dir = match plugins_dir {
        Some(d) if d.is_dir() => d,
        _ => return plugins,
    };

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return plugins,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("lua") {
            match LuaPlugin::from_file(&path) {
                Ok(plugin) => {
                    tracing::info!(
                        "Discovered Lua plugin: {} ({})",
                        plugin.plugin_name,
                        path.display()
                    );
                    plugins.push(Box::new(plugin));
                }
                Err(e) => {
                    tracing::warn!("Failed to load Lua plugin {}: {e}", path.display());
                }
            }
        }
    }

    plugins
}

/// A WASM plugin that runs as an external WASI process.
///
/// Communication is via stdin/stdout JSON messages. The WASM runtime
/// (`wasmtime` or `wasmer`) must be installed on the system.
pub struct WasmPlugin {
    /// Plugin name extracted from filename.
    plugin_name: String,
    /// Path to the `.wasm` file.
    wasm_path: std::path::PathBuf,
}

impl WasmPlugin {
    /// Create a new WASM plugin from a `.wasm` file path.
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        // Verify the file exists and has WASM magic bytes.
        let bytes = std::fs::read(path)?;
        if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
            anyhow::bail!("Not a valid WASM file: {}", path.display());
        }
        Ok(Self {
            plugin_name: name,
            wasm_path: path.to_path_buf(),
        })
    }

    /// Run the WASM plugin with a JSON command and return the JSON response.
    fn run_command(&self, command: &str) -> Option<String> {
        // Try wasmtime first, then wasmer.
        let runtimes = ["wasmtime", "wasmer"];
        for runtime in &runtimes {
            if let Ok(output) = std::process::Command::new(runtime)
                .arg("run")
                .arg(&self.wasm_path)
                .arg("--")
                .arg(command)
                .output()
            {
                if output.status.success() {
                    return String::from_utf8(output.stdout).ok();
                }
            }
        }
        None
    }
}

impl Plugin for WasmPlugin {
    fn name(&self) -> &str {
        &self.plugin_name
    }

    fn on_load(&mut self) -> anyhow::Result<()> {
        // Verify a WASI runtime is available.
        let has_runtime = std::process::Command::new("wasmtime")
            .arg("--version")
            .output()
            .is_ok()
            || std::process::Command::new("wasmer")
                .arg("--version")
                .output()
                .is_ok();
        if !has_runtime {
            anyhow::bail!("No WASI runtime found (install wasmtime or wasmer)");
        }
        Ok(())
    }

    fn on_key(&mut self, mode: &str, key: &str) -> Option<PluginAction> {
        let cmd = format!("{{\"event\":\"key\",\"mode\":\"{mode}\",\"key\":\"{key}\"}}");
        let response = self.run_command(&cmd)?;
        let trimmed = response.trim();
        if let Some(rest) = trimmed.strip_prefix("cmd:") {
            Some(PluginAction::RunCommand(rest.to_string()))
        } else if let Some(rest) = trimmed.strip_prefix("insert:") {
            Some(PluginAction::InsertText(rest.to_string()))
        } else {
            trimmed
                .strip_prefix("status:")
                .map(|rest| PluginAction::SetStatus(rest.to_string()))
        }
    }

    fn on_save(&mut self, path: &str) -> anyhow::Result<()> {
        let cmd = format!("{{\"event\":\"save\",\"path\":\"{path}\"}}");
        self.run_command(&cmd);
        Ok(())
    }

    fn on_intent(&mut self, intent: &str) -> Option<String> {
        let cmd = format!("{{\"event\":\"intent\",\"text\":\"{intent}\"}}");
        self.run_command(&cmd)
    }
}

/// Discover and load all `.wasm` plugins from `~/.aura/plugins/`.
pub fn discover_wasm_plugins() -> Vec<Box<dyn Plugin>> {
    let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();

    let plugins_dir = dirs_path_home().map(|h| h.join(".aura").join("plugins"));

    let dir = match plugins_dir {
        Some(d) if d.is_dir() => d,
        _ => return plugins,
    };

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return plugins,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
            match WasmPlugin::from_file(&path) {
                Ok(plugin) => {
                    tracing::info!(
                        "Discovered WASM plugin: {} ({})",
                        plugin.plugin_name,
                        path.display()
                    );
                    plugins.push(Box::new(plugin));
                }
                Err(e) => {
                    tracing::warn!("Failed to load WASM plugin {}: {e}", path.display());
                }
            }
        }
    }

    plugins
}

/// Get the user's home directory.
fn dirs_path_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
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

    /// Update editor state for all plugins.
    ///
    /// Sets the `editor` global table in each Lua plugin so scripts can
    /// access `editor.buffer`, `editor.cursor_row`, `editor.file_path`, etc.
    pub fn update_editor_state(&mut self, state: &PluginEditorState) {
        for plugin in &mut self.plugins {
            plugin.update_state(state);
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
