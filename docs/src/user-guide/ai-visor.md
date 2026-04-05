# AI Visor

The AI Visor is a right-side panel that provides a unified view of all Claude Code and AI agent configuration in your project. Open it with `Ctrl+I` or `:visor`.

## Tabs

The visor has six tabs. Press the number key (`1`-`6`) or `Tab` to cycle between them.

### 1: Overview

A dashboard showing quick stats:

- **Model** — active Claude model (from settings cascade)
- **Effort** — configured effort level
- **CLAUDE.md** — whether a project instructions file exists (with size)
- **Settings / Skills / Hooks / Plugins / Agents** — counts for each category
- **Permissions** — number of allowed permission rules
- **Rules / Docs** — project rules and indexed docs counts

### 2: Settings

Flattened view of all Claude Code settings with scope indicators:

- **[G]** Global — from `~/.claude/settings.json`
- **[P]** Project — from `.claude/settings.json`
- **[L]** Local — from `.claude/settings.local.json`

Settings are shown in cascade order so you can see which scope overrides which.

### 3: Skills

Lists all available skills discovered from:

- `.claude/skills/*/SKILL.md` — skill definitions with YAML frontmatter
- `.claude/commands/*.md` — legacy command format

Each entry shows the skill name and description. Press `Enter` to open the skill file in the editor.

- **`▶`** — user-invocable skill (can be called with `/skill-name`)
- **`○`** — model-only skill (used by Claude automatically)

### 4: Hooks

Shows all configured hooks from settings files, grouped by event type:

- Event name and hook type (command, http, prompt, agent)
- The command or URL that runs when the hook fires

### 5: Plugins

Lists installed Claude Code plugins from `~/.claude/plugins/installed_plugins.json`, showing name and source URL.

### 6: Agents

Discovers agent definitions from markdown files:

- **Project agents** — `.claude/agents/*.md` in the project directory
- **Global agents** — `~/.claude/agents/*.md` in your home directory

Each agent file is a markdown document with optional YAML frontmatter:

```markdown
---
name: my-agent
description: A brief description of what this agent does
---

Agent prompt and instructions here...
```

Agents are shown with scope tags:

- **[P]** — project-level agent (green)
- **[G]** — global agent (blue)

Press `Enter` on an agent to open its definition file in the editor.

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+I` | Toggle visor panel |
| `1`-`6` | Jump to tab |
| `Tab` | Cycle to next tab |
| `j` / `k` | Navigate entries |
| `Enter` | Open selected skill/agent file |
| `Esc` | Close visor / return focus to editor |
