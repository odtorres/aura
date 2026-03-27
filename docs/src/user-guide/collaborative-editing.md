# Collaborative Editing

AURA supports real-time collaborative editing, allowing multiple instances to work on the same file simultaneously. Edits are synchronized using the automerge CRDT, so concurrent changes are merged automatically without conflicts.

## Starting a Session

### Hosting

One AURA instance acts as the host. The host starts a TCP listener and waits for peers to connect.

**From the command line:**

```bash
aura myfile.rs --host
```

**From within the editor:**

```
:host
```

The port number is shown in the status bar (e.g., `COLLAB:54321`). Share this port with peers who want to join.

### Joining

Other instances connect to the host by address and port.

**From the command line:**

```bash
aura --join 127.0.0.1:54321
```

**From within the editor:**

```
:join 127.0.0.1:54321
```

On join, the host sends a full document snapshot. The joining instance's buffer is replaced with the host's content, and sync begins.

### Setting a Display Name

Your display name is shown to peers above your cursor. By default, it uses your system username. Override it with:

```bash
aura myfile.rs --host --name alice
```

Or configure it permanently in `aura.toml`:

```toml
[collab]
display_name = "alice"
default_port = 0    # 0 = random available port
```

### Ending a Session

```
:collab-stop
```

This disconnects all peers (if hosting) or disconnects from the host (if joined).

## Peer Awareness

When collaborating, you can see other peers' cursors and selections in real-time:

- **Cursor blocks**: Each peer's cursor is shown as a colored block at their position in the file
- **Name labels**: The peer's display name appears above their cursor
- **Selection highlights**: When a peer selects text, the selection range is highlighted in their color
- **Unique colors**: Each peer is assigned a unique color from a rotating palette (cyan, magenta, orange, teal, purple, yellow)

Awareness updates are throttled to every 50ms for efficiency.

## Status Bar

The status bar shows the current collaboration state:

| Indicator | Meaning |
|-----------|---------|
| `COLLAB:54321` | Hosting on port 54321 |
| `COLLAB:54321 (2 peers)` | Hosting with 2 connected peers |
| `COLLAB (1 peers)` | Connected as a client, 1 other peer |
| `COLLAB reconnecting #3...` | Connection lost, retry attempt 3 |

## Reconnection

If the connection drops (network issue, host restart), the client automatically reconnects with exponential backoff:

- Retry intervals: 1s, 2s, 4s, 8s, 16s, 30s (max)
- During reconnection, local edits continue normally
- On reconnect, the automerge sync protocol catches up with all missed changes from both sides
- The host retains disconnected peer sync state for 5 minutes, so reconnections within that window are efficient

## How It Works

Under the hood, AURA's collaborative editing uses:

1. **Automerge CRDT**: Every edit is recorded in an automerge document. The sync protocol exchanges only the changes each side is missing.
2. **Incremental rope reconciliation**: When remote changes arrive, only the changed character range is patched in the rope buffer (not the entire document).
3. **Binary wire protocol**: Messages are framed with a 4-byte length prefix + 1-byte type tag. Types include sync messages, awareness updates, peer join/leave notifications, and document snapshots.
4. **Background threads**: All network I/O runs on background threads, communicating with the main event loop via channels. The editor never blocks on network operations.

## Limitations

- **Single file per session**: Each collaboration session edits one file. Multiple files require multiple sessions.
- **Localhost only**: Currently limited to TCP on localhost. Remote collaboration over the internet requires manual port forwarding or a VPN.
- **No authentication**: Any client that can reach the host's port can join. Use network-level access control for security.
