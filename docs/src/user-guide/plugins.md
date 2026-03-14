# Plugins

AURA supports a plugin system that allows extending the editor with custom functionality.

## Plugin Architecture

Plugins implement the `Plugin` trait and register with the `PluginManager`. Each plugin can:

- Register new intents (AI actions)
- Add new modes
- Create custom UI panels
- Hook into editor lifecycle events

## Listing Plugins

```
:plugins
```

Shows all currently loaded plugins.

## Built-in Plugins

AURA ships with core functionality implemented as plugins:

- **File picker**: Fuzzy file finder (`<Space>p` / `:files`)
- **File tree**: Directory sidebar (`Ctrl+N` / `:tree`)
- **Terminal**: Embedded PTY terminal (`Ctrl+J` / `:term`)

## Plugin Lifecycle

Plugins are initialized when the editor starts and receive events throughout the editing session:

1. **Registration**: Plugin registers with the `PluginManager`
2. **Initialization**: Plugin sets up its state
3. **Event handling**: Plugin responds to editor events (file open, edit, save, etc.)
4. **Teardown**: Plugin cleans up on editor exit

## Developing Plugins

Plugins implement the `Plugin` trait defined in `crates/tui/src/plugin.rs`. See the [Architecture: TUI](../architecture/tui.md) section for details on the trait interface.
