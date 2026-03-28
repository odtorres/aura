# Keybindings

This is a comprehensive reference of all keybindings in AURA. For a conceptual overview, see [Modes](modes.md).

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

### LSP Integration

| Key | Action |
|-----|--------|
| `gd` | Go to definition |
| `K` | Show hover information |
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
| `Ctrl+J` | Toggle chat panel (and focus) |
| `Ctrl+H` | Toggle conversation history panel |
| `Ctrl+W` | Toggle split pane focus |
| `Ctrl+,` | Open settings modal |
| `F1` | Open help overlay |

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
| `:tree` | | Toggle file tree sidebar |
| `:term-height <N>` | `:th <N>` | Set terminal pane height |
| `:compact` | | Compact conversation database |
| `:vsplit` | `:vs` | Vertical split pane |
| `:hsplit` | `:sp` | Horizontal split pane |
| `:only` | | Close split pane |
| `:settings` | `:prefs` | Open settings modal |
| `:update` | `:check-update` | Force check for updates |
| `:host` | | Start hosting a collab session |
| `:join <addr:port>` | | Join a collab session |
| `:collab-stop` | | End the collab session |

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
| `i` / `Enter` | Edit commit message (on Commit Message section) |
| `Enter` | Open diff view (on file entry) |
| `Esc` | Return focus to editor |

## Conversation Panel

| Key | Action |
|-----|--------|
| `Esc` / `q` | Close panel |
| `j` / `Down` | Scroll down |
| `k` / `Up` | Scroll up |

## Customization

Keybindings can be customized in `aura.toml`. See [Configuration](../getting-started/configuration.md#keybinding-customization).
