# Claude Code Integration

AURA can be used alongside [Claude Code](https://claude.com/claude-code) so that Claude Code conversations flow into AURA's conversation store in real-time.

## How It Works

Claude Code speaks MCP over stdio, while AURA's MCP server uses TCP. The `aura-mcp-bridge` binary bridges these two transports, forwarding JSON-RPC messages bidirectionally.

```
Claude Code <--stdio--> aura-mcp-bridge <--TCP--> AURA MCP Server
```

## Setup

### 1. Install `aura-mcp-bridge`

The bridge ships alongside the `aura` binary. If you installed AURA via Homebrew or the shell installer, `aura-mcp-bridge` is already available.

To build from source:

```bash
cargo install --path crates/bridge
```

### 2. Start AURA

Launch AURA with any file:

```bash
aura myfile.rs
```

AURA automatically starts its MCP server and writes a discovery file to `~/.aura/mcp.json`.

### 3. Configure Claude Code

Add a `.mcp.json` file to your project root:

```json
{
  "mcpServers": {
    "aura": {
      "command": "aura-mcp-bridge"
    }
  }
}
```

No arguments are needed — the bridge auto-discovers the running AURA instance.

### 4. Verify

In Claude Code, run `/mcp` to confirm AURA's tools are available. You should see tools like `read_buffer`, `edit_buffer`, `log_conversation`, and others.

## Available Tools

Once connected, Claude Code can use all of AURA's MCP tools:

| Tool | Description |
|------|-------------|
| `read_buffer` | Read the current buffer content |
| `edit_buffer` | Apply edits to the buffer |
| `log_conversation` | Store a message in AURA's conversation history |
| `get_cursor_context` | Get cursor position and surrounding context |
| `get_diagnostics` | Get LSP diagnostics |
| `get_selection` | Get the current selection |
| `get_conversation_history` | Retrieve conversation history |

## Configuration

### Explicit Port Override

If auto-discovery doesn't work, set the port explicitly:

```json
{
  "mcpServers": {
    "aura": {
      "command": "aura-mcp-bridge",
      "env": {
        "AURA_MCP_PORT": "8432"
      }
    }
  }
}
```

### Troubleshooting

- **Bridge can't find AURA**: Make sure AURA is running before starting Claude Code. Check that `~/.aura/mcp.json` exists.
- **Connection refused**: The MCP port may have changed. Restart AURA and try again.
- **Logs**: Check `~/.aura/bridge.log` for detailed bridge diagnostics.
