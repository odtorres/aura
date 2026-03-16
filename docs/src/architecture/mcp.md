# MCP Protocol

AURA implements both an MCP server (exposing editor capabilities) and an MCP client (connecting to external tool servers). This makes AURA a platform — external AI agents can read and edit buffers, and AURA can leverage external tools.

## MCP Server

AURA exposes an MCP server over WebSocket (localhost, auto-assigned port).

### Tools Exposed

| Tool | Description |
|------|-------------|
| `read_buffer` | Read the current buffer content |
| `edit_buffer` | Apply edits to the buffer |
| `get_diagnostics` | Get LSP diagnostics |
| `get_selection` | Get the current selection |
| `get_cursor_context` | Get cursor position and surrounding context |
| `get_conversation_history` | Retrieve conversation history |

### Resources Exposed

| Resource | Description |
|----------|-------------|
| `buffer/current` | Current buffer text |
| `buffer/info` | File path, language, line count, modified state |
| `diagnostics` | Current diagnostics list |

### Auto-Discovery

When AURA starts, it:

1. Binds the MCP server to a random available port on localhost
2. Sets `AURA_MCP_PORT` in the embedded terminal's environment
3. Writes `~/.aura/mcp.json` with connection details:

```json
{
  "host": "127.0.0.1",
  "port": 8432,
  "pid": 12345,
  "file": "/path/to/current/file.rs"
}
```

4. Cleans up the discovery file on exit

This allows Claude Code (or any MCP client) running inside AURA's terminal to automatically connect.

## Claude Code Bridge (`aura-mcp-bridge`)

Claude Code communicates with MCP servers over **stdio**, while AURA's MCP server uses **TCP** with Content-Length framing. The `aura-mcp-bridge` binary bridges the two transports:

```
Claude Code <--stdio--> aura-mcp-bridge <--TCP--> AURA MCP Server
```

The bridge:
1. Discovers the running AURA instance via `AURA_MCP_PORT` env var or `~/.aura/mcp.json`
2. Connects to AURA's TCP MCP server
3. Forwards Content-Length framed JSON-RPC messages bidirectionally between stdio and TCP
4. Logs to `~/.aura/bridge.log` (never to stdout, which is the MCP transport)

### Setup

Add to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "aura": {
      "command": "aura-mcp-bridge"
    }
  }
}
```

No arguments needed — the bridge auto-discovers the running AURA instance.

See [Claude Code Integration](../user-guide/claude-code.md) for detailed setup instructions.

## MCP Client

AURA can connect to external MCP servers defined in `aura.toml`:

```toml
[mcp_servers.filesystem]
url = "ws://localhost:9000"

[mcp_servers.custom-tools]
url = "ws://localhost:9001"
```

Connected servers' tools become available within the editor.

## Multi-Agent Support

Multiple AI agents can connect to AURA's MCP server simultaneously:

- Each agent gets a unique `AuthorId` in the CRDT
- Edits from different agents are tracked separately
- The status bar shows the agent count and MCP port
- If two agents edit the same region, the human decides via the Review interface
- Agent orchestration: different agents can be assigned different tasks (e.g., "Agent A handles tests, Agent B handles implementation")

## Agent Registry

The `AgentRegistry` in `mcp_server.rs` tracks connected agents:

- Assigns unique author IDs
- Routes MCP actions to the appropriate handler
- Generates `McpAppResponse` for each action

## API Reference

See the [rustdoc for `aura-tui` MCP modules](/api/aura_tui/mcp_server/) and [`mcp_client`](/api/aura_tui/mcp_client/).
