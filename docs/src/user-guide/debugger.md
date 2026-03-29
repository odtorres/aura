# Integrated Debugger (DAP)

AURA includes an integrated debugger using the Debug Adapter Protocol (DAP) — the same protocol VS Code uses. This means any DAP-compatible debug adapter works with AURA.

## Supported Debug Adapters

| Language | Adapter | Install |
|----------|---------|---------|
| Rust, C, C++ | CodeLLDB | `codelldb` on PATH |
| Python | debugpy | `pip install debugpy` |
| Go | Delve | `go install github.com/go-delve/delve/cmd/dlv@latest` |
| JavaScript/TypeScript | Node.js | Built-in (`node --inspect`) |

AURA auto-detects the appropriate adapter based on file extension.

## Breakpoints

### Setting Breakpoints

- **F9** — Toggle breakpoint on the current line
- `:breakpoint` or `:bp` — Toggle breakpoint on the current line

Breakpoints are shown as red dots (`●`) in the gutter. They persist for the editor session and work even without an active debug session — set them before you start debugging.

### Breakpoint Indicators

| Gutter | Meaning |
|--------|---------|
| `●` (red) | Breakpoint set |
| `→` (yellow) | Current execution line |
| `⏸` (yellow) | Breakpoint + current execution line |

## Starting a Debug Session

### Quick Start

```
:debug
```

or press **F5** — auto-detects the debug adapter for the current file type and starts the session.

### Launch a Specific Program

```
:debug ./target/debug/my_program
```

Launches the specified binary under the debugger.

## Debug Controls

| Key | Action |
|-----|--------|
| **F5** | Continue (or start if no session) |
| **Shift+F5** | Stop debugging |
| **F9** | Toggle breakpoint |
| **F10** | Step over |
| **F11** | Step into |
| **Shift+F11** | Step out |

### Command-Mode Equivalents

| Command | Action |
|---------|--------|
| `:debug` / `:db` | Start debug session |
| `:debug <program>` | Launch specific program |
| `:breakpoint` / `:bp` | Toggle breakpoint |
| `:continue` / `:dc` | Continue execution |
| `:step` / `:ds` | Step over |
| `:stepin` / `:dsi` | Step into |
| `:stepout` / `:dso` | Step out |
| `:debug stop` / `:dstop` | Stop session |
| `:debug panel` / `:dp` | Toggle debug panel |

## Debug Panel

The debug panel appears at the bottom of the screen (like the terminal) when a debug session is active. It has three tabs:

### 1. Call Stack (press `1`)

Shows the current call stack when execution is paused:

```
#0  main()           main.rs:42
#1  run_program()    lib.rs:128
#2  std::rt::lang_start  rt.rs:165
```

Press **Enter** on a frame to navigate to that location in the editor.

### 2. Variables (press `2`)

Shows local variables for the selected stack frame:

```
  x: i32 = 42
  name: &str = "hello"
▶ items: Vec<i32> = Vec(3)
```

Compound types (structs, vectors) show a `▶` indicator and can be expanded with **Enter**.

### 3. Output (press `3`)

Shows program stdout/stderr output, scrollable.

### Panel Navigation

| Key | Action |
|-----|--------|
| `1` / `2` / `3` | Switch tabs |
| `j` / `k` | Navigate items |
| `Enter` | Expand variable or jump to frame |
| `Esc` | Return focus to editor |

## Configuration

Custom debug adapters can be configured in `aura.toml`:

```toml
[debuggers.my-adapter]
command = "codelldb"
args = ["--port", "0"]
extensions = ["rs", "c", "cpp"]

[debuggers.debugpy]
command = "python3"
args = ["-m", "debugpy.adapter"]
extensions = ["py"]
```

If no user configuration is found, AURA falls back to auto-detection based on file extension.
