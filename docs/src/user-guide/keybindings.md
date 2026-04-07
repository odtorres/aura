# Keybindings

This is a comprehensive reference of all keybindings in AURA. For a conceptual overview, see [Modes](modes.md).

## Customization

You can remap global shortcuts and leader key sequences in `aura.toml`:

```toml
[keybindings]
leader = "Space"  # Default. Options: "Space", "Backslash", "Comma"

# Remap global Ctrl+ shortcuts
[keybindings.global_map]
"ctrl+j" = "toggle_chat"
"ctrl+h" = "toggle_history"
"ctrl+shift+g" = "open_git_graph"
"ctrl+k" = "open_command_palette"

# Remap leader key sequences (Space + key)
[keybindings.leader_map]
e = "explain"
f = "fix"
x = "open_git_graph"
```

### Available Actions

| Action | Description |
|--------|-------------|
| `toggle_terminal` | Toggle terminal pane |
| `toggle_chat` | Toggle chat panel |
| `toggle_history` | Toggle AI History panel |
| `toggle_file_tree` | Toggle file tree sidebar |
| `toggle_git` | Toggle git/source control panel |
| `toggle_visor` | Toggle AI Visor panel |
| `toggle_blame` | Toggle inline git blame |
| `open_file_picker` | Open fuzzy file picker |
| `open_command_palette` | Open command palette |
| `open_git_graph` | Open git graph modal |
| `open_settings` | Open settings modal |
| `open_outline` | Open document outline |
| `open_branch_picker` | Open branch picker |
| `project_search` | Open project-wide search |
| `save` | Save current file |
| `intent` | Enter AI Intent mode |
| `cycle_aggressiveness` | Cycle ghost suggestion level |
| `recent_decisions` | Show recent AI decisions |
| `next_tab` | Switch to next tab |
| `prev_tab` | Switch to previous tab |
| `close_tab` | Close current tab |

Custom mappings take priority over built-in defaults. Vim core motions (`hjkl`, `w`, `b`, operators, text objects) are not configurable.

## Normal Mode

### Navigation

| Key | Action |
|-----|--------|
| `h` / `Left` | Move cursor left |
| `j` / `Down` | Move cursor down |
| `k` / `Up` | Move cursor up |
| `l` / `Right` | Move cursor right |
| `w` | Jump to next word start |
| `b` | Jump to previous word start |
| `e` | Jump to word end |
| `0` | Jump to line start |
| `$` | Jump to line end |
| `gg` | Jump to first line |
| `G` | Jump to last line |

### Mode Transitions

| Key | Target Mode |
|-----|-------------|
| `i` | Insert (at cursor) |
| `a` | Insert (after cursor) |
| `o` | Insert (new line below) |
| `v` | Visual (character) |
| `V` | Visual Line |
| `:` | Command |

### Operators (+ motion)

| Key | Action |
|-----|--------|
| `d{motion}` | Delete range (e.g. `dw`, `d$`, `dd`) |
| `c{motion}` | Change range (delete + Insert, e.g. `cw`, `ci"`) |
| `y{motion}` | Yank range (e.g. `yw`, `yy`) |
| `>{motion}` | Indent range (e.g. `>>`, `>j`) |
| `<{motion}` | Dedent range (e.g. `<<`, `<k`) |
| `{count}{op}` | Repeat with count (e.g. `3dd`, `5j`, `2dw`) |

### Text Objects (with operator)

| Key | Action |
|-----|--------|
| `i"` / `a"` | Inner / around double quotes |
| `i'` / `a'` | Inner / around single quotes |
| `i(` / `a(` | Inner / around parentheses |
| `i{` / `a{` | Inner / around braces |
| `i[` / `a[` | Inner / around brackets |
| `i<` / `a<` | Inner / around angle brackets |
| `iw` / `aw` | Inner / around word |

### Character Search

| Key | Action |
|-----|--------|
| `f{char}` | Jump to next char on line |
| `F{char}` | Jump to prev char on line |
| `t{char}` | Jump to before next char |
| `T{char}` | Jump to after prev char |
| `;` | Repeat last char search |
| `,` | Reverse last char search |
| `*` | Search word under cursor (forward) |
| `#` | Search word under cursor (backward) |

### Editing

| Key | Action |
|-----|--------|
| `x` | Delete character under cursor |
| `dd` | Delete line |
| `yy` / `Y` | Yank line |
| `p` | Paste from register |
| `u` | Undo last edit |
| `D` | Delete to end of line |
| `C` | Change to end of line |
| `s` | Substitute character (delete + Insert) |
| `S` | Substitute line |
| `r{char}` | Replace character under cursor |
| `J` | Join current line with next |
| `~` | Toggle case |

### Repeat & Macros

| Key | Action |
|-----|--------|
| `.` | Repeat last edit |
| `q{a-z}` | Start recording macro into register |
| `q` | Stop recording macro |
| `@{a-z}` | Play back macro from register |
| `:registers` | Open registers modal (view/edit macros) |

### Surround Editing

| Key | Action |
|-----|--------|
| `cs{old}{new}` | Change surrounding: `cs"'` changes `"hi"` to `'hi'` |
| `ds{char}` | Delete surrounding: `ds(` removes parens |
| `ys{motion}{char}` | Add surrounding: `ysiw"` wraps word in quotes |

### Marks / Bookmarks

| Key | Action |
|-----|--------|
| `m{a-z}` | Set mark at cursor position |
| `'{a-z}` | Jump to mark |

### Code Folding

| Key | Action |
|-----|--------|
| `za` | Toggle fold at cursor |
| `zc` | Close fold at cursor |
| `zo` | Open fold at cursor |
| `zM` | Close all folds |
| `zR` | Open all folds |

### LSP Navigation

| Key | Action |
|-----|--------|
| `gd` | Go to definition |
| `gp` | Peek definition (inline popup) |
| `gr` | Find all references |
| `gn` | Rename symbol |
| `K` | Show hover info |
| `F2` | Rename symbol (alternative) |

### Snippets (Insert mode)

| Key | Action |
|-----|--------|
| `Tab` | Expand snippet trigger / jump to next placeholder / insert indent |

Type a trigger word (e.g., `fn`, `if`, `for`, `def`, `class`) then press Tab to expand. Tab cycles through placeholders. See [AI Features](ai-features.md) for the full snippet list.

### Multi-Cursor

| Key | Action |
|-----|--------|
| `Ctrl+D` | Add cursor at next occurrence of word under cursor |
| `Esc` | Clear all secondary cursors |

### LSP Integration

| Key | Action |
|-----|--------|
| `gd` | Go to definition |
| `gp` | Peek definition (inline popup) |
| `gr` | Find all references |
| `gn` | Rename symbol |
| `K` | Show hover information |
| `F2` | Rename symbol (alternative) |
| `]` | Jump to next diagnostic |
| `[` | Jump to previous diagnostic |

### Ghost Suggestions

| Key | Action |
|-----|--------|
| `Tab` | Accept current ghost suggestion |
| `Esc` | Dismiss ghost suggestions |
| `Alt+]` | Next ghost suggestion |
| `Alt+[` | Previous ghost suggestion |

### UI Controls

| Key | Action |
|-----|--------|
| `Ctrl+S` | Save file |
| `Ctrl+N` | Toggle file tree sidebar (and focus) |
| `Ctrl+T` | Toggle terminal pane (and focus) |
| `Ctrl+G` | Toggle git/source control panel (and focus) |
| `Ctrl+J` | Toggle chat panel (and focus) |
| `Ctrl+H` | Toggle conversation history panel |
| `Ctrl+D` | Add cursor at next word match (multi-cursor) |
| `Ctrl+O` | Open document outline (symbol list) |
| `Ctrl+F` | Open project-wide search/replace |
| `Ctrl+I` | Toggle AI Visor panel |
| `Ctrl+B` | Open branch picker |
| `Ctrl+P` | Open command palette |
| `Ctrl+W` | Toggle split pane focus |
| `Ctrl+,` | Open settings modal |
| `F1` | Open help overlay |
| `F5` | Start/continue debug session |
| `F9` | Toggle breakpoint |
| `F10` | Step over (debugger) |
| `F11` | Step into (debugger) |

> **Note:** Panel-switching shortcuts (`Ctrl+T/G/N/J/H/,`) work from **any focused panel** — you never need to press `Esc` first to switch between panels.

## Leader Key Sequences (`Space` + key)

| Sequence | Action |
|----------|--------|
| `<Space>i` | Enter Intent mode |
| `<Space>e` | AI: Explain selected code |
| `<Space>f` | AI: Fix errors at cursor |
| `<Space>t` | AI: Generate test |
| `<Space>u` | Undo AI edits only |
| `<Space>a` | Toggle authorship markers |
| `<Space>b` | Toggle inline git blame |
| `<Space>c` | Show conversation history for current line |
| `<Space>d` | Show recent AI decisions |
| `<Space>g` | Cycle AI aggressiveness (minimal/moderate/proactive) |
| `<Space>s` | Show semantic info for symbol at cursor |
| `<Space>p` | Open fuzzy file picker |

## Insert Mode

| Key | Action |
|-----|--------|
| `Esc` | Return to Normal mode |
| Characters | Insert at cursor |
| `Enter` | Insert newline |
| `Backspace` | Delete character before cursor |
| Arrow keys | Navigate (without leaving Insert) |
| `Ctrl+S` | Save file |

## Visual / Visual Line Mode

| Key | Action |
|-----|--------|
| `Esc` | Cancel selection, return to Normal |
| `h`/`j`/`k`/`l` | Extend selection |
| `w`/`b` | Extend by word |
| `0`/`$` | Extend to line start/end |
| `g`/`G` | Extend to file start/end |
| `d` / `x` | Delete selection |
| `y` | Yank (copy) selection |

## Command Mode

| Command | Alias | Action |
|---------|-------|--------|
| `:w` | | Save file |
| `:q` | | Quit (warns if unsaved) |
| `:q!` | | Force quit |
| `:wq` | | Save and quit |
| `:intent` | | Enter Intent mode |
| `:search <query>` | | Search conversation history |
| `:decisions` | `:dec` | Show recent AI decisions |
| `:undo-tree` | `:ut` | Show undo tree |
| `:commit` | `:gc` | Generate AI commit message |
| `:commit <msg>` | | Commit with message |
| `:branches` | `:br` | List git branches |
| `:checkout <name>` | | Switch git branch |
| `:branch <name>` | | Create new git branch |
| `:blame` | | Toggle inline blame |
| `:log` | `:gl` | Show aura git log |
| `:log <N>` | | Show last N log entries |
| `:experiment <name>` | | Enter experimental mode on branch |
| `:code-action` | `:ca` | Request LSP code actions |
| `:plugins` | | List loaded plugins |
| `:files` | `:fp` | Open fuzzy file picker |
| `:term` | `:terminal` | Toggle terminal pane |
| `:term new` | | Open new terminal tab |
| `:term close` | | Close active terminal tab |
| `:term next` | | Switch to next terminal tab |
| `:term prev` | | Switch to previous terminal tab |
| `:tree` | | Toggle file tree sidebar |
| `:term-height <N>` | `:th <N>` | Set terminal pane height |
| `:compact` | | Compact conversation database |
| `:vsplit` | `:vs` | Vertical split pane |
| `:hsplit` | `:sp` | Horizontal split pane |
| `:only` | | Close split pane |
| `:graph` | | Open git graph modal |
| `:branches` | `:br` | Open branch picker |
| `:settings` | `:prefs` | Open settings modal |
| `:update` | `:check-update` | Force check for updates |
| `:host` | | Start hosting a collab session |
| `:join <addr:port>` | | Join a collab session |
| `:collab-stop` | | End the collab session |
| `:follow <name>` | | Follow a peer's viewport in real-time |
| `:unfollow` | | Stop following a peer |
| `:share-term` | | Toggle terminal sharing (host only) |
| `:view-term` | | Toggle shared terminal view (client only) |
| `Ctrl+Shift+G` | | Open git graph modal |
| `Ctrl+]` | | Jump to next edit prediction |
| `Ctrl+[` | | Jump to previous edit prediction |
| `:agent <task>` | | Start autonomous AI agent |
| `:agent -n <N> <task>` | | Agent with custom iteration limit |
| `:agent stop` | | Stop the running agent |
| `:search <query>` | `:grep` | Project-wide search |
| `:visor` | | Open AI Visor panel |
| `:merge` | | Open merge conflict editor |
| `:references` | `:ref` | Find all references |
| `:rename <name>` | | Rename symbol |
| `:accept-current` | `:ac` | Resolve conflict: keep current |
| `:accept-incoming` | `:ai` | Resolve conflict: keep incoming |
| `:accept-both` | `:ab` | Resolve conflict: keep both |
| `:debug` | `:db` | Start debug session |
| `:breakpoint` | `:bp` | Toggle breakpoint |
| `:marks` | | List all marks |
| `:registers` | `:reg` | Show registers modal (yank + macros) |
| `:fix` | | Send last failed terminal command to AI chat |
| `:set rnu` | | Enable relative line numbers |
| `:set nornu` | | Disable relative line numbers |
| `:set wrap` | | Enable word wrap |
| `:set nowrap` | | Disable word wrap |

## Review Mode

| Key | Action |
|-----|--------|
| `a` / `Enter` | Accept AI proposal |
| `r` / `Esc` | Reject AI proposal |
| `e` | Edit proposal text in-place |
| `R` | Request revision with follow-up |

## Intent Mode

| Key | Action |
|-----|--------|
| `Esc` | Cancel and return to Normal (or Review if editing) |
| `Enter` | Submit intent / confirm edit / submit revision |
| `Backspace` | Delete character (returns to Normal if empty) |
| Characters | Type intent text |

## Terminal Pane (focused)

| Key | Action |
|-----|--------|
| `Esc` | Return focus to editor |
| `Ctrl+J` / `` Ctrl+` `` | Return focus to editor |
| `Ctrl+Shift+T` | Open new terminal tab |
| `Ctrl+Shift+]` | Switch to next terminal tab |
| `Ctrl+Shift+[` | Switch to previous terminal tab |
| `Ctrl+Shift+Up` | Increase terminal height |
| `Ctrl+Shift+Down` | Decrease terminal height |
| `Ctrl+C` | Send interrupt signal |
| `Ctrl+D` | Send EOF |
| `Ctrl+L` | Clear terminal screen |
| All other keys | Forwarded to PTY shell |

## File Tree (focused)

| Key | Action |
|-----|--------|
| `j` / `Down` | Select next entry |
| `k` / `Up` | Select previous entry |
| `Enter` / `l` | Open file or expand directory |
| `h` | Collapse directory or go to parent |
| `Esc` | Return focus to editor |
| `Ctrl+N` | Close file tree and return focus |

## File Picker (overlay)

| Key | Action |
|-----|--------|
| `Esc` | Close picker |
| `Enter` | Open selected file |
| `Backspace` | Delete character from query |
| `Ctrl+K` / `Up` | Select previous match |
| `Ctrl+J` / `Down` | Select next match |
| Characters | Type to filter files |

## Chat Panel (focused)

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Esc` | Unfocus (return to editor) |
| `Ctrl+J` | Close chat panel |
| `Ctrl+H` | Switch to conversation history |
| `Ctrl+Up` | Scroll messages up |
| `Ctrl+Down` | Scroll messages down |
| `PageUp` / `PageDown` | Scroll by page |
| `Up` / `Down` | Move cursor in input |
| `Left` / `Right` | Move cursor in input |
| `Home` / `End` | Jump to input start/end |
| `Y` | Approve pending tool call |
| `N` | Deny pending tool call |

## AI History Panel (focused)

| Key | Action |
|-----|--------|
| `j` / `Down` | Select next conversation |
| `k` / `Up` | Select previous conversation |
| `Enter` | Expand conversation / open detail modal |
| `/` | Start search (filter by title, file, branch) |
| `u` | Scroll expanded messages up |
| `d` | Scroll expanded messages down |
| `Esc` | Close search / unfocus panel / close modal |

### Detail Modal

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll line by line |
| `d` / `u` | Page down / up |
| `Esc` / `q` | Close modal |

## Git Panel (focused)

| Key | Action |
|-----|--------|
| `j` / `Down` | Select next entry |
| `k` / `Up` | Select previous entry |
| `Tab` | Cycle to next section |
| `BackTab` | Cycle to previous section |
| `s` | Stage selected file |
| `S` | Stage all changed files |
| `u` | Unstage selected file |
| `d` | Discard changes (with `y` confirmation) |
| `c` | Commit staged changes |
| `g` | Generate AI commit message |
| `i` / `Enter` | Edit commit message (on Commit Message section) |
| `Enter` | Open diff view (on file entry) |
| `Esc` | Return focus to editor |

## Conversation Panel

| Key | Action |
|-----|--------|
| `Esc` / `q` | Close panel |
| `j` / `Down` | Scroll down |
| `k` / `Up` | Scroll up |

## New Commands (v0.3+)

### Editing Commands

| Command | Description |
|---------|-------------|
| `:sort` | Sort selected or all lines alphabetically |
| `:sort!` | Sort lines in reverse |
| `:comment` | Toggle line comment (also `gc` in normal/visual) |
| `:duplicate` / `:dup` | Duplicate current line |
| `:upper` | Convert selection to UPPERCASE |
| `:lower` | Convert selection to lowercase |
| `:trim` | Trim trailing whitespace from all lines |
| `:encoding lf` | Convert line endings to LF |
| `:encoding crlf` | Convert line endings to CRLF |
| `:%s/old/new/g` | Search and replace (global) |
| `:s/old/new` | Search and replace (current line) |
| `:N` (number) | Jump to line N (e.g., `:42`) |

### Navigation & Panels

| Command / Key | Description |
|---------------|-------------|
| `:cd <path>` | Change working directory |
| `:pwd` | Show working directory |
| `:calls` | Show incoming callers (LSP call hierarchy) |
| `:session save <name>` | Save named session |
| `:session load <name>` | Load named session |
| `:session list` | List saved sessions |
| `:session delete <name>` | Delete named session |
| `:pin` / `:unpin` | Pin/unpin current tab |
| `:tabmove left/right` | Reorder tabs |
| `:scrollsync` | Toggle split pane scroll sync |
| `:watch <expr>` | Add debug watch expression |
| `:unwatch <expr>` | Remove watch expression |

### Agent Commands

| Command | Description |
|---------|-------------|
| `:agent <task>` | Start autonomous agent |
| `:agent plan <task>` | Start agent with planning phase |
| `:agent pause` | Pause running agent |
| `:agent resume` | Resume paused agent |
| `:agent trust read\|write\|full` | Set agent trust level |
| `:agent diff` | Review agent changes |
| `:agent timeline` | Toggle agent activity timeline |

### New Keybindings

| Key | Mode | Description |
|-----|------|-------------|
| `Alt+j` | Normal | Move line down |
| `Alt+k` | Normal | Move line up |
| `gc` | Normal/Visual | Toggle line comment |
| `ge` | Normal | Backward to end of previous word |
| `gE` | Normal | Backward to end of previous WORD |
| `Ctrl+P` | Chat (agent) | Pause/resume agent |
| `Ctrl+F` | Terminal | Search terminal scrollback |
| `( [ { " ' <` | Visual | Wrap selection in brackets/quotes |

### Breakpoints

| Command | Description |
|---------|-------------|
| `:breakpoint` / `:bp` | Toggle breakpoint at cursor |
| `:breakpoint if <cond>` | Set conditional breakpoint |
| `Enter` | Expand/collapse debug variable |

## Configuration (aura.toml)

### Per-Feature AI Models

```toml
[ai]
model = "claude-sonnet-4-20250514"              # default
commit_model = "claude-haiku-4-5-20251001"      # fast commit messages
speculative_model = "claude-haiku-4-5-20251001" # ghost suggestions
agent_model = "claude-sonnet-4-20250514"        # agent tasks
chat_model = ""                                  # empty = use default
summarize_model = "claude-haiku-4-5-20251001"   # conversation compaction
```

### Editor Settings

```toml
[editor]
format_on_save = false     # Run formatter on save
auto_save_seconds = 0      # Auto-save interval (0 = disabled)
clipboard_sync = true      # Sync yank to system clipboard
show_minimap = true        # Show code minimap
```

## File Tree Actions (focused)

| Key | Action |
|-----|--------|
| `r` | Rename file/directory (inline input) |
| `d` | Delete file/directory (with `y` confirmation) |
| `a` | New file in selected directory |
| `A` | New directory in selected directory |
| `y` | Copy (yank) file |
| `x` | Cut file |
| `p` | Paste copied/cut file |
| `.` | Reveal in Finder / file manager |

## Interactive Rebase Modal

| Key | Action |
|-----|--------|
| `p` | Pick (keep commit as-is) |
| `r` | Reword (edit message) |
| `e` | Edit (pause for amending) |
| `s` | Squash (meld with previous) |
| `f` | Fixup (meld, discard message) |
| `d` | Drop (remove commit) |
| `Alt+j` / `Alt+k` | Reorder commit up/down |
| `w` / `Enter` | Execute rebase |
| `q` / `Esc` | Abort |

## Plugin Marketplace Modal

| Key | Action |
|-----|--------|
| Type | Filter by name/description/author |
| `Enter` | Install selected plugin |
| `d` | Uninstall selected plugin |
| `r` | Refresh registry from remote |
| `j` / `k` | Navigate |
| `Esc` | Close |

## New Commands (v0.5+)

| Command | Description |
|---------|-------------|
| `:zen` | Toggle zen mode (hide all chrome) |
| `:preview` / `:md` | Toggle markdown live preview |
| `:rebase [N]` | Interactive rebase last N commits (default 10) |
| `:ssh user@host:/path` | Open remote file via SSH |
| `:plugin search [query]` | Open plugin marketplace |
| `:plugin install <name>` | Install a plugin |
| `:plugin uninstall <name>` | Uninstall a plugin |
| `:plugin update` | Update all plugins |
| `:plugin list` | List installed plugins |
| `:refactor <instruction>` | Multi-file AI refactoring |
| `:review` | AI code review of staged diff |
| `:export [path]` | Export chat history as markdown |
| `:http send` | Execute HTTP request at cursor |
| `:cell run` | Run code cell at cursor |
| `:cell run-all` | Run all code cells |
| `:pair on/off` | Toggle AI pair programming |
| `:keymap vim/emacs/vscode` | Switch keybinding profile |

## Inline AI Completions (Insert Mode)

| Key | Action |
|-----|--------|
| `Tab` | Accept inline completion |
| Any other key | Dismiss completion |

Ghost text appears after the cursor when the AI has a suggestion. Powered by the speculative engine.

## Mouse Actions

| Action | Description |
|--------|-------------|
| Drag tab | Reorder tabs by dragging in the tab bar |
| Click status bar | Open command palette |
| Click gutter | Toggle breakpoint |

## Customization

Keybindings can be customized in `aura.toml`. See [Configuration](../getting-started/configuration.md#keybinding-customization).
