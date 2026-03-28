# Plugins

AURA supports dynamic plugins written in **Lua 5.4**. Place `.lua` files in `~/.aura/plugins/` and they're automatically loaded on startup.

## Quick Start

Create `~/.aura/plugins/hello.lua`:

```lua
plugin = {
    name = "hello",

    on_load = function()
        -- Called once when the plugin loads.
    end,

    on_save = function(path)
        -- Called after a file is saved.
    end,

    on_key = function(mode, key)
        -- Called on every keypress.
        -- Return "status:message" to show in status bar.
        -- Return "cmd:w" to execute a command.
        -- Return "insert:text" to insert text at cursor.
        -- Return nil to do nothing.
        return nil
    end,

    on_intent = function(intent)
        -- Called before an intent is sent to AI.
        -- Return a modified string to change the intent.
        -- Return nil to leave it unchanged.
        return nil
    end,
}
```

## Plugin API

Each Lua plugin must define a global `plugin` table with at least a `name` field. All callback functions are optional.

### Callbacks

| Callback | Arguments | Return | When |
|----------|-----------|--------|------|
| `on_load()` | none | none | Once, at startup |
| `on_key(mode, key)` | mode string, key string | action string or nil | Every keypress |
| `on_save(path)` | file path string | none | After file save |
| `on_intent(intent)` | intent string | modified string or nil | Before AI intent |

### Return Actions (from `on_key`)

| Prefix | Effect | Example |
|--------|--------|---------|
| `cmd:` | Execute editor command | `"cmd:w"` (save) |
| `insert:` | Insert text at cursor | `"insert:hello"` |
| `status:` | Show status bar message | `"status:Plugin active"` |

## Listing Plugins

```
:plugins
```

Shows all currently loaded plugins (both Lua and built-in).

## Example: Auto-format on Save

```lua
plugin = {
    name = "auto-format",

    on_save = function(path)
        if path:match("%.rs$") then
            os.execute("rustfmt " .. path)
        end
    end,
}
```

## Example: Custom Keybinding

```lua
plugin = {
    name = "quick-save",

    on_key = function(mode, key)
        if mode == "NORMAL" and key == "Q" then
            return "cmd:wq"
        end
        return nil
    end,
}
```

## Plugin Directory

Plugins are loaded from `~/.aura/plugins/`. Create the directory if it doesn't exist:

```bash
mkdir -p ~/.aura/plugins
```

Each `.lua` file is an independent plugin with its own Lua VM instance. Plugins are sandboxed — they can't access other plugins' state.

## Architecture

Plugins implement the `Plugin` trait (Rust) via a Lua bridge. The bridge:

- Creates a Lua 5.4 VM per plugin (via `mlua` crate)
- Wraps each `.lua` file as a `LuaPlugin` struct implementing `Plugin`
- Routes editor events through the `PluginManager` to all loaded plugins
- Collects return actions and applies them to the editor

See the [Architecture: TUI](../architecture/tui.md) section for details on the internal trait interface.
