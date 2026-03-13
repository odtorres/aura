# AURA

**AI-native Universal Reactive Authoring** — a terminal text editor built from the ground up for human + AI co-authoring.

> The human steers. The AI proposes. The editor mediates.

## What makes AURA different?

Current editors treat AI as a plugin — a guest in a house built for a single human cursor. AURA treats human and AI as **co-authors**, with the editor as the mediator between them.

- **Authorship-aware editing**: Every change is tagged with who made it (human or AI). Undo just the AI's changes without losing yours.
- **Intent-first workflow**: Express what you want to achieve in natural language. The AI proposes, you review in a structured diff, then accept or reject per-hunk.
- **Conversation as history**: The conversation that led to every piece of code is recorded and queryable. Six months later, ask "why was this written this way?"
- **Multi-agent collaboration**: Multiple AI agents can work simultaneously via CRDT, with conflict-free concurrent editing.
- **Speculative execution**: The AI thinks ahead in the background, offering improvement suggestions as ghost text overlays.

## Status

🚧 **Early development — Phase 0 (Foundation)**

Currently building the minimal viable editor: rope-based buffer, modal editing, file I/O, and TUI rendering.

See [TODO.md](TODO.md) for the full roadmap.

## Quick Start

```bash
# Build
cargo build --release

# Open a file
cargo run -p aura -- path/to/file.rs

# Open scratch buffer
cargo run -p aura
```

## Keybindings (Phase 0)

| Key       | Mode    | Action                |
|-----------|---------|-----------------------|
| `i`       | Normal  | Enter Insert mode     |
| `a`       | Normal  | Append after cursor   |
| `o`       | Normal  | Open line below       |
| `Esc`     | Insert  | Return to Normal mode |
| `h/j/k/l` | Normal | Navigate              |
| `x`       | Normal  | Delete character      |
| `u`       | Normal  | Undo                  |
| `:`       | Normal  | Enter Command mode    |
| `:w`      | Command | Save                  |
| `:q`      | Command | Quit                  |
| `:wq`     | Command | Save and quit         |
| `Ctrl+S`  | Any     | Save                  |

## Tech Stack

Rust · Tokio · Ropey · Ratatui · Crossterm · Automerge (planned) · Tree-sitter (planned) · MCP (planned)

## License

MIT
