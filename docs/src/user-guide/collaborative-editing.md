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

## Multi-File Sessions

AURA supports multi-file collaboration in a single session. When the host starts a session, **all open files** are shared with peers:

- Joining clients automatically receive snapshots for every file the host has open and open them in new tabs
- Sync messages are routed to the correct buffer using a file identifier (a hash of the canonical file path)
- Peer cursors and selections are filtered per file — you only see cursors for the file you're currently viewing
- Scratch buffers (unsaved, no file path) are excluded from multi-file sharing

## Remote Collaboration

AURA supports collaboration over the internet with token-based authentication.

### Hosting for Remote Access

Configure in `aura.toml`:

```toml
[collab]
bind_address = "0.0.0.0"   # Listen on all interfaces (not just localhost)
require_auth = true          # Require token to join
```

When you run `:host`, the status bar shows the port and auth token:

```
Hosting on port 9000 | Token: a1b2c3d4e5f6...
```

Share the token with collaborators securely.

### Joining a Remote Session

```
:join 192.168.1.100:9000 a1b2c3d4e5f6...
```

Or from the command line:

```bash
aura --join 192.168.1.100:9000 --token a1b2c3d4e5f6...
```

If the token is wrong, the connection is rejected with a clear error message.

### Security Notes

- The auth token is generated randomly per session (32 hex chars)
- Tokens are ephemeral — they expire when the host closes the session
- Enable TLS for encrypted traffic (see below)

## TLS Encryption

Enable TLS to encrypt all collaboration traffic with a self-signed certificate:

```toml
[collab]
use_tls = true
```

When TLS is enabled:
- The host generates a self-signed certificate automatically via `rcgen`
- All peer traffic (sync messages, awareness, snapshots) is encrypted via `rustls`
- Clients accept self-signed certificates (suitable for trusted peer-to-peer use)
- TLS works alongside authentication tokens for defense in depth

Both host and client must have `use_tls = true` in their `aura.toml` (or the connection will fail due to protocol mismatch).

## Follow Mode

Follow a peer's viewport in real-time — when they scroll or switch files, your view updates automatically.

### Starting

```
:follow alice
```

The peer name is case-insensitive. If the name isn't found, AURA shows the available peer names.

### Behavior

- Your viewport syncs to the followed peer's scroll position
- If the peer switches to a different file, you switch too (if you have that file open)
- **Any local navigation breaks follow mode** — scrolling, cursor movement, mouse wheel, or search all end it
- Status bar shows `FOLLOWING <name>` while active

### Stopping

```
:unfollow
```

Follow mode also ends automatically when:
- The followed peer disconnects
- The collab session ends
- You navigate locally (scroll, cursor move, etc.)

## Shared Terminal

The host can share their terminal screen with all connected peers in real-time (read-only).

### Host Side

```
:share-term
```

Toggles terminal sharing on/off. The status bar shows `[sharing term]` when active. The host's active terminal tab is broadcast to peers as screen snapshots (~7 Hz when content changes).

### Client Side

```
:view-term
```

Toggles between the local terminal and the shared terminal view. The shared terminal is rendered with a cyan border and "Host Terminal (read-only)" title. Clients cannot interact with the shared terminal — it's view-only.

### Notes

- Only the host can share (clients get "Only the host can share their terminal")
- The host shares whichever terminal tab is active; switching tabs automatically updates the shared view
- Sharing stops when the collab session ends

## Limitations

- **Scratch buffers excluded**: Unsaved buffers without a file path are not shared in multi-file sessions.
