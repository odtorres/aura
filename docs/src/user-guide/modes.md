# Modes

AURA uses vim-inspired modal editing with additional modes for AI interaction. Each mode defines how keystrokes are interpreted.

## Mode Overview

```
                    ┌─────────┐
            i/a/o   │         │  Esc
          ┌────────►│ Insert  ├────────┐
          │         │         │        │
          │         └─────────┘        │
          │                            ▼
     ┌────┴────┐    ┌─────────┐   ┌────────┐
     │         │ :  │         │Esc│        │
     │ Normal  ├───►│ Command ├──►│ Normal │
     │         │    │         │   │        │
     └┬──┬──┬──┘    └─────────┘   └────────┘
      │  │  │
      │  │  │ v/V    ┌─────────┐
      │  │  └───────►│ Visual  ├──── Esc ──► Normal
      │  │           └─────────┘
      │  │
      │  │ <Space>i  ┌─────────┐  Enter  ┌─────────┐
      │  └──────────►│ Intent  ├────────►│ Review  ├──── a/r ──► Normal
      │              └─────────┘         └────┬────┘
      │                   ▲                   │
      │                   │    e/R (edit/     │
      │                   └────revise)────────┘
      │
      │ Focus panels
      ├──── Ctrl+N ──► File Tree ──── Esc ──► Normal
      └──── Ctrl+J ──► Terminal  ──── Esc ──► Normal
```

## Normal Mode

The default mode. All navigation and commands start here.

- **Navigation**: `h`/`j`/`k`/`l`, word motions (`w`/`b`/`e`), `gg`/`G`, `0`/`$`
- **Editing**: `x` (delete char), `d` (delete line), `y` (yank), `p` (paste), `u` (undo)
- **Mode transitions**: `i`/`a`/`o` (Insert), `v`/`V` (Visual), `:` (Command), `<Space>` (leader)
- **LSP**: `gd` (go to definition), `K` (hover), `]`/`[` (next/prev diagnostic)
- **Ghost suggestions**: `Tab` (accept), `Alt+]`/`Alt+[` (cycle), `Esc` (dismiss)

## Insert Mode

Entered via `i`, `a`, or `o` from Normal mode. Characters are inserted directly into the buffer.

- All typed characters are inserted at the cursor position
- `Esc` returns to Normal mode
- `Ctrl+S` saves without leaving Insert mode
- Arrow keys navigate without leaving Insert mode

Every insertion is tagged with the human `AuthorId` for authorship tracking.

## Visual Mode

Entered via `v` (character selection) or `V` (line selection) from Normal mode.

- Navigation keys extend the selection
- `d`/`x` deletes the selection and copies to the register
- `y` yanks the selection
- `Esc` cancels and returns to Normal

Visual selections also define the context range sent to AI when entering Intent mode.

## Command Mode

Entered via `:` from Normal mode. Type an ex-style command and press `Enter` to execute.

Commands include file operations (`:w`, `:q`, `:wq`), git operations (`:commit`, `:branches`, `:blame`), UI toggles (`:tree`, `:term`, `:files`), and AI features (`:intent`).

See [Keybindings](keybindings.md) for the full command list.

## Intent Mode

Entered via `<Space>i` or `:intent` (requires `ANTHROPIC_API_KEY`).

In Intent mode, you type a natural language description of what you want the AI to do. Press `Enter` to submit — the intent is sent to Claude with the current buffer context, cursor position, and relevant diagnostics. The AI response streams in as a structured edit proposal.

Quick AI actions bypass the intent input:
- `<Space>e` — Explain selected code
- `<Space>f` — Fix errors at cursor
- `<Space>t` — Generate test for function

## Review Mode

Entered automatically when the AI returns a proposal.

The screen splits into current code (top) and proposed code (bottom) with diff highlighting.

| Key | Action |
|-----|--------|
| `a` / `Enter` | Accept the proposal — applies the AI edit to the buffer |
| `r` / `Esc` | Reject the proposal — discards it |
| `e` | Edit the proposal text in-place before accepting |
| `R` | Request a revision — enter Intent mode to describe changes to the proposal |

Accepted edits are tagged with the AI's `AuthorId` and can be independently undone with `<Space>u`.
