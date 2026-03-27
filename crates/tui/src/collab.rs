//! Real-time collaborative editing over TCP.
//!
//! Provides host-client collaboration using automerge's sync protocol.
//! The host starts a TCP listener; clients connect and exchange binary-framed
//! sync messages.  All network I/O happens on background threads, communicating
//! with the main event loop via `std::sync::mpsc` channels — the same pattern
//! used by `mcp_server.rs`, `lsp.rs`, and other subsystems.

use aura_core::sync::SyncState;
use std::collections::HashMap;
use std::io::{Read as IoRead, Write as IoWrite};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Wire protocol
// ---------------------------------------------------------------------------

/// Shared list of connected client streams for broadcasting.
type ClientList = Arc<Mutex<Vec<(u64, Arc<Mutex<TcpStream>>)>>>;

/// Message type bytes for the wire protocol.
const MSG_SYNC: u8 = 0x01;
const MSG_AWARENESS: u8 = 0x02;
const MSG_PEER_JOINED: u8 = 0x03;
const MSG_PEER_LEFT: u8 = 0x04;
const MSG_DOC_SNAPSHOT: u8 = 0x05;

/// Encode a wire message: 4-byte big-endian length + 1-byte type + payload.
fn encode_wire(msg_type: u8, payload: &[u8]) -> Vec<u8> {
    let total_len = 1 + payload.len(); // type byte + payload
    let mut buf = Vec::with_capacity(4 + total_len);
    buf.extend_from_slice(&(total_len as u32).to_be_bytes());
    buf.push(msg_type);
    buf.extend_from_slice(payload);
    buf
}

/// Read one wire message from a stream. Returns (type, payload).
fn read_wire(stream: &mut impl IoRead) -> std::io::Result<(u8, Vec<u8>)> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let total_len = u32::from_be_bytes(len_buf) as usize;
    if total_len == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "zero-length message",
        ));
    }
    let mut msg_buf = vec![0u8; total_len];
    stream.read_exact(&mut msg_buf)?;
    let msg_type = msg_buf[0];
    let payload = msg_buf[1..].to_vec();
    Ok((msg_type, payload))
}

/// Write one wire message to a stream.
fn write_wire(stream: &mut impl IoWrite, msg_type: u8, payload: &[u8]) -> std::io::Result<()> {
    let data = encode_wire(msg_type, payload);
    stream.write_all(&data)?;
    stream.flush()
}

// ---------------------------------------------------------------------------
// Awareness
// ---------------------------------------------------------------------------

/// Ephemeral awareness state for a peer (cursor, selection, name).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AwarenessUpdate {
    /// Unique peer identifier.
    pub peer_id: u64,
    /// Display name.
    pub name: String,
    /// Cursor position as (row, col), if any.
    pub cursor: Option<(usize, usize)>,
    /// Selection range as ((start_row, start_col), (end_row, end_col)).
    pub selection: Option<((usize, usize), (usize, usize))>,
}

// ---------------------------------------------------------------------------
// Events and commands (main loop ↔ network threads)
// ---------------------------------------------------------------------------

/// Events sent from network threads to the main event loop.
#[derive(Debug)]
pub enum CollabEvent {
    /// A sync message arrived from a peer.
    SyncMessage {
        /// Which peer sent it.
        peer_id: u64,
        /// Raw automerge sync message bytes.
        data: Vec<u8>,
    },
    /// A peer's awareness (cursor/selection) changed.
    Awareness(AwarenessUpdate),
    /// A new peer joined the session.
    PeerJoined {
        /// Unique peer identifier.
        peer_id: u64,
        /// Display name.
        name: String,
    },
    /// A peer disconnected.
    PeerLeft {
        /// Which peer left.
        peer_id: u64,
    },
    /// Full document snapshot received (client only, during initial sync).
    DocSnapshot(Vec<u8>),
    /// Client is attempting to reconnect.
    Reconnecting {
        /// Current retry attempt number.
        attempt: u32,
    },
    /// Client successfully reconnected.
    Reconnected,
    /// An error occurred on the network layer.
    Error(String),
}

/// Commands sent from the main event loop to the network threads.
#[derive(Debug, Clone)]
pub enum CollabCommand {
    /// Broadcast a sync message to all peers.
    BroadcastSync(Vec<u8>),
    /// Broadcast an awareness update.
    BroadcastAwareness(AwarenessUpdate),
    /// Shut down the collaboration session.
    Shutdown,
}

// ---------------------------------------------------------------------------
// Peer info
// ---------------------------------------------------------------------------

/// Tracks state for a connected peer.
pub struct PeerInfo {
    /// Unique peer identifier.
    pub peer_id: u64,
    /// Display name.
    pub name: String,
    /// Automerge sync state for this peer.
    pub sync_state: SyncState,
    /// Latest awareness (cursor, selection).
    pub awareness: Option<AwarenessUpdate>,
    /// When we last heard from this peer.
    pub last_seen: std::time::Instant,
}

// ---------------------------------------------------------------------------
// Collab status (for UI display)
// ---------------------------------------------------------------------------

/// Current status of the collaboration session.
#[derive(Debug, Clone)]
pub enum CollabStatus {
    /// Hosting a session.
    Hosting {
        /// Port we are listening on.
        port: u16,
        /// Number of connected peers.
        peer_count: usize,
    },
    /// Connected as a client.
    Connected {
        /// Number of peers in the session.
        peer_count: usize,
    },
    /// Client is attempting to reconnect.
    Reconnecting {
        /// Current retry attempt number.
        attempt: u32,
    },
    /// Not in a collaborative session.
    Inactive,
}

// ---------------------------------------------------------------------------
// CollabSession — the main handle
// ---------------------------------------------------------------------------

/// Manages a collaboration session (either host or client).
///
/// The session owns background threads for network I/O and communicates
/// with the main event loop via channels.
pub struct CollabSession {
    /// Receive events from network threads.
    event_rx: mpsc::Receiver<CollabEvent>,
    /// Send commands to network threads.
    command_tx: mpsc::Sender<CollabCommand>,
    /// Connected peers and their sync states.
    pub peers: HashMap<u64, PeerInfo>,
    /// Whether this instance is the host.
    pub is_host: bool,
    /// Our peer ID.
    pub local_peer_id: u64,
    /// Our display name.
    pub local_name: String,
    /// Port we are listening on (host only).
    pub port: Option<u16>,
    /// Whether the client is currently reconnecting.
    pub reconnecting: bool,
    /// Current reconnect attempt number (client only).
    pub reconnect_attempt: u32,
    /// Disconnected peers with retained sync state (host only).
    /// Maps peer_id → (SyncState, disconnect_time).
    disconnected_peers: HashMap<u64, (SyncState, std::time::Instant)>,
    /// Shutdown flag shared with threads.
    shutdown: Arc<Mutex<bool>>,
}

impl CollabSession {
    /// Start hosting a collaboration session on the given port (0 = random).
    pub fn host(display_name: &str, port: u16, doc_snapshot: Vec<u8>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(format!("127.0.0.1:{port}"))?;
        let actual_port = listener.local_addr()?.port();
        listener.set_nonblocking(false)?;

        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel::<CollabCommand>();
        let shutdown = Arc::new(Mutex::new(false));
        let shutdown_clone = shutdown.clone();

        let local_peer_id = generate_peer_id();
        let name = display_name.to_string();

        // Shared list of connected client streams for broadcasting.
        let clients: ClientList = Arc::new(Mutex::new(Vec::new()));
        let clients_for_accept = clients.clone();
        let clients_for_cmd = clients.clone();
        let snapshot = Arc::new(Mutex::new(doc_snapshot));

        // Accept thread: listen for incoming connections.
        let event_tx_accept = event_tx.clone();
        let shutdown_accept = shutdown_clone.clone();
        thread::Builder::new()
            .name("collab-accept".to_string())
            .spawn(move || {
                for stream_result in listener.incoming() {
                    if *shutdown_accept.lock().unwrap() {
                        break;
                    }
                    let stream = match stream_result {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

                    let event_tx = event_tx_accept.clone();
                    let clients = clients_for_accept.clone();
                    let snap = snapshot.lock().unwrap().clone();
                    let shutdown_peer = shutdown_accept.clone();

                    thread::Builder::new()
                        .name("collab-peer".to_string())
                        .spawn(move || {
                            host_handle_peer(stream, event_tx, clients, snap, shutdown_peer);
                        })
                        .ok();
                }
            })?;

        // Command dispatch thread: forward commands to all connected clients.
        let shutdown_cmd = shutdown_clone.clone();
        thread::Builder::new()
            .name("collab-cmd".to_string())
            .spawn(move || {
                while let Ok(cmd) = command_rx.recv() {
                    if *shutdown_cmd.lock().unwrap() {
                        break;
                    }
                    match cmd {
                        CollabCommand::BroadcastSync(data) => {
                            broadcast(&clients_for_cmd, MSG_SYNC, &data);
                        }
                        CollabCommand::BroadcastAwareness(update) => {
                            if let Ok(json) = serde_json::to_vec(&update) {
                                broadcast(&clients_for_cmd, MSG_AWARENESS, &json);
                            }
                        }
                        CollabCommand::Shutdown => break,
                    }
                }
            })?;

        Ok(Self {
            event_rx,
            command_tx,
            peers: HashMap::new(),
            is_host: true,
            local_peer_id,
            local_name: name,
            port: Some(actual_port),
            reconnecting: false,
            reconnect_attempt: 0,
            disconnected_peers: HashMap::new(),
            shutdown,
        })
    }

    /// Join an existing collaboration session.
    pub fn join(display_name: &str, addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel::<CollabCommand>();
        let shutdown = Arc::new(Mutex::new(false));

        let local_peer_id = generate_peer_id();
        let name = display_name.to_string();
        let reconnect_addr = addr.to_string();

        // Send our PeerJoined message.
        let join_payload = serde_json::to_vec(&serde_json::json!({
            "peer_id": local_peer_id,
            "name": name,
        }))
        .unwrap();

        let mut writer = stream.try_clone()?;
        write_wire(&mut writer, MSG_PEER_JOINED, &join_payload)?;

        let writer = Arc::new(Mutex::new(writer));
        let writer_for_cmd = writer.clone();

        // Reader thread with automatic reconnection.
        let event_tx_read = event_tx;
        let shutdown_read = shutdown.clone();
        let reconnect_name = name.clone();
        let reconnect_peer_id = local_peer_id;
        let reconnect_writer = writer.clone();
        let reader_stream = stream;
        thread::Builder::new()
            .name("collab-reader".to_string())
            .spawn(move || {
                client_reader_loop(
                    reader_stream,
                    event_tx_read,
                    shutdown_read,
                    reconnect_addr,
                    reconnect_name,
                    reconnect_peer_id,
                    reconnect_writer,
                );
            })?;

        // Command dispatch thread: forward commands to the host.
        let shutdown_cmd = shutdown.clone();
        thread::Builder::new()
            .name("collab-writer".to_string())
            .spawn(move || {
                while let Ok(cmd) = command_rx.recv() {
                    if *shutdown_cmd.lock().unwrap() {
                        break;
                    }
                    let mut w = writer_for_cmd.lock().unwrap();
                    match cmd {
                        CollabCommand::BroadcastSync(data) => {
                            let _ = write_wire(&mut *w, MSG_SYNC, &data);
                        }
                        CollabCommand::BroadcastAwareness(update) => {
                            if let Ok(json) = serde_json::to_vec(&update) {
                                let _ = write_wire(&mut *w, MSG_AWARENESS, &json);
                            }
                        }
                        CollabCommand::Shutdown => break,
                    }
                }
            })?;

        Ok(Self {
            event_rx,
            command_tx,
            peers: HashMap::new(),
            is_host: false,
            local_peer_id,
            local_name: name,
            port: None,
            reconnecting: false,
            reconnect_attempt: 0,
            disconnected_peers: HashMap::new(),
            shutdown,
        })
    }

    /// Poll for incoming events (non-blocking).
    pub fn poll_events(&self) -> Vec<CollabEvent> {
        let mut events = Vec::new();
        loop {
            match self.event_rx.try_recv() {
                Ok(event) => events.push(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        events
    }

    /// Send a command to the network threads.
    pub fn send_command(&self, cmd: CollabCommand) {
        let _ = self.command_tx.send(cmd);
    }

    /// Broadcast a sync message to all peers.
    pub fn broadcast_sync(&self, data: Vec<u8>) {
        self.send_command(CollabCommand::BroadcastSync(data));
    }

    /// Broadcast an awareness update.
    pub fn broadcast_awareness(&self, update: AwarenessUpdate) {
        self.send_command(CollabCommand::BroadcastAwareness(update));
    }

    /// Get the current collaboration status for UI display.
    pub fn status(&self) -> CollabStatus {
        if self.reconnecting {
            return CollabStatus::Reconnecting {
                attempt: self.reconnect_attempt,
            };
        }
        if self.is_host {
            CollabStatus::Hosting {
                port: self.port.unwrap_or(0),
                peer_count: self.peers.len(),
            }
        } else {
            CollabStatus::Connected {
                peer_count: self.peers.len(),
            }
        }
    }

    /// Register a new peer. On the host, restores retained sync state if available.
    pub fn add_peer(&mut self, peer_id: u64, name: String) {
        // Try to restore sync state from a previous connection.
        let sync_state = self.restore_peer_state(peer_id).unwrap_or_default();

        self.peers.insert(
            peer_id,
            PeerInfo {
                peer_id,
                name,
                sync_state,
                awareness: None,
                last_seen: std::time::Instant::now(),
            },
        );
    }

    /// Remove a peer. On the host, retains sync state for potential reconnection.
    pub fn remove_peer(&mut self, peer_id: u64) {
        if let Some(peer) = self.peers.remove(&peer_id) {
            if self.is_host {
                // Retain sync state for 5 minutes in case they reconnect.
                self.disconnected_peers
                    .insert(peer_id, (peer.sync_state, std::time::Instant::now()));
            }
        }
    }

    /// Try to restore a disconnected peer's sync state (host only).
    /// Returns the retained SyncState if the peer reconnected within the TTL.
    pub fn restore_peer_state(&mut self, peer_id: u64) -> Option<SyncState> {
        if let Some((state, disconnected_at)) = self.disconnected_peers.remove(&peer_id) {
            if disconnected_at.elapsed() < Duration::from_secs(300) {
                return Some(state);
            }
        }
        None
    }

    /// Clean up expired disconnected peer states (older than 5 minutes).
    pub fn cleanup_disconnected_peers(&mut self) {
        self.disconnected_peers
            .retain(|_, (_, time)| time.elapsed() < Duration::from_secs(300));
    }

    /// Update a peer's awareness.
    pub fn update_peer_awareness(&mut self, update: AwarenessUpdate) {
        if let Some(peer) = self.peers.get_mut(&update.peer_id) {
            peer.last_seen = std::time::Instant::now();
            peer.awareness = Some(update);
        }
    }

    /// Shut down the session.
    pub fn shutdown(&self) {
        *self.shutdown.lock().unwrap() = true;
        let _ = self.command_tx.send(CollabCommand::Shutdown);
        // Poke the listener to unblock the accept loop (host only).
        if let Some(port) = self.port {
            let _ = TcpStream::connect(format!("127.0.0.1:{port}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Host: handle a single peer connection
// ---------------------------------------------------------------------------

/// Handle a single peer connection on the host side.
fn host_handle_peer(
    stream: TcpStream,
    event_tx: mpsc::Sender<CollabEvent>,
    clients: ClientList,
    doc_snapshot: Vec<u8>,
    shutdown: Arc<Mutex<bool>>,
) {
    let mut reader = stream.try_clone().expect("clone stream");
    let writer = Arc::new(Mutex::new(stream));

    // Wait for the PeerJoined message.
    let peer_id = match read_wire(&mut reader) {
        Ok((MSG_PEER_JOINED, payload)) => {
            if let Ok(info) = serde_json::from_slice::<serde_json::Value>(&payload) {
                let peer_id = info["peer_id"].as_u64().unwrap_or(0);
                let name = info["name"].as_str().unwrap_or("anonymous").to_string();
                let _ = event_tx.send(CollabEvent::PeerJoined { peer_id, name });
                peer_id
            } else {
                return;
            }
        }
        _ => return,
    };

    // Send the document snapshot.
    {
        let mut w = writer.lock().unwrap();
        if write_wire(&mut *w, MSG_DOC_SNAPSHOT, &doc_snapshot).is_err() {
            return;
        }
    }

    // Register this client for broadcasting.
    clients.lock().unwrap().push((peer_id, writer));

    // Read loop: receive messages from the peer.
    loop {
        if *shutdown.lock().unwrap() {
            break;
        }
        match read_wire(&mut reader) {
            Ok((msg_type, payload)) => {
                let event = match msg_type {
                    MSG_SYNC => CollabEvent::SyncMessage {
                        peer_id,
                        data: payload,
                    },
                    MSG_AWARENESS => {
                        if let Ok(update) = serde_json::from_slice::<AwarenessUpdate>(&payload) {
                            CollabEvent::Awareness(update)
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };
                if event_tx.send(event).is_err() {
                    break;
                }
            }
            Err(_) => {
                break;
            }
        }
    }

    // Peer disconnected — clean up.
    let _ = event_tx.send(CollabEvent::PeerLeft { peer_id });
    let mut cl = clients.lock().unwrap();
    cl.retain(|(id, _)| *id != peer_id);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Client reader loop with automatic reconnection on disconnect.
///
/// Reads messages from the host. On disconnect, retries with exponential
/// backoff (1s, 2s, 4s, ... up to 30s). On successful reconnect, re-sends
/// the PeerJoined message and resumes reading.
fn client_reader_loop(
    initial_stream: TcpStream,
    event_tx: mpsc::Sender<CollabEvent>,
    shutdown: Arc<Mutex<bool>>,
    addr: String,
    name: String,
    peer_id: u64,
    writer: Arc<Mutex<TcpStream>>,
) {
    let mut reader = initial_stream;

    loop {
        // Read messages until disconnected.
        loop {
            if *shutdown.lock().unwrap() {
                return;
            }
            match read_wire(&mut reader) {
                Ok((msg_type, payload)) => {
                    let event = decode_event(msg_type, payload);
                    if event_tx.send(event).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    if *shutdown.lock().unwrap() {
                        return;
                    }
                    break; // Disconnected — enter reconnect loop.
                }
            }
        }

        // Reconnection loop with exponential backoff.
        let mut attempt = 0u32;
        loop {
            if *shutdown.lock().unwrap() {
                return;
            }

            attempt += 1;
            let _ = event_tx.send(CollabEvent::Reconnecting { attempt });

            // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 30s max.
            let delay = Duration::from_secs((1u64 << attempt.min(5)).min(30));
            thread::sleep(delay);

            if *shutdown.lock().unwrap() {
                return;
            }

            // Attempt to reconnect.
            match TcpStream::connect(&addr) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

                    // Re-send PeerJoined.
                    let join_payload = serde_json::to_vec(&serde_json::json!({
                        "peer_id": peer_id,
                        "name": name,
                    }))
                    .unwrap();

                    let mut new_writer = match stream.try_clone() {
                        Ok(w) => w,
                        Err(_) => continue,
                    };

                    if write_wire(&mut new_writer, MSG_PEER_JOINED, &join_payload).is_err() {
                        continue;
                    }

                    // Swap the writer so command dispatch uses the new connection.
                    *writer.lock().unwrap() = new_writer;
                    reader = stream;

                    let _ = event_tx.send(CollabEvent::Reconnected);
                    break; // Resume reading.
                }
                Err(_) => continue, // Retry.
            }
        }
    }
}

/// Decode a wire message into a CollabEvent.
fn decode_event(msg_type: u8, payload: Vec<u8>) -> CollabEvent {
    match msg_type {
        MSG_SYNC => CollabEvent::SyncMessage {
            peer_id: 0, // client doesn't know host's peer_id; uses 0
            data: payload,
        },
        MSG_AWARENESS => {
            if let Ok(update) = serde_json::from_slice::<AwarenessUpdate>(&payload) {
                CollabEvent::Awareness(update)
            } else {
                CollabEvent::Error("malformed awareness message".to_string())
            }
        }
        MSG_PEER_JOINED => {
            if let Ok(info) = serde_json::from_slice::<serde_json::Value>(&payload) {
                let peer_id = info["peer_id"].as_u64().unwrap_or(0);
                let name = info["name"].as_str().unwrap_or("anonymous").to_string();
                CollabEvent::PeerJoined { peer_id, name }
            } else {
                CollabEvent::Error("malformed peer-joined message".to_string())
            }
        }
        MSG_PEER_LEFT => {
            if let Ok(info) = serde_json::from_slice::<serde_json::Value>(&payload) {
                let peer_id = info["peer_id"].as_u64().unwrap_or(0);
                CollabEvent::PeerLeft { peer_id }
            } else {
                CollabEvent::Error("malformed peer-left message".to_string())
            }
        }
        MSG_DOC_SNAPSHOT => CollabEvent::DocSnapshot(payload),
        _ => CollabEvent::Error(format!("unknown message type: 0x{msg_type:02x}")),
    }
}

/// Broadcast a wire message to all connected clients.
fn broadcast(clients: &ClientList, msg_type: u8, payload: &[u8]) {
    let cl = clients.lock().unwrap();
    for (_, stream) in cl.iter() {
        let mut s = stream.lock().unwrap();
        let _ = write_wire(&mut *s, msg_type, payload);
    }
}

/// Generate a random peer ID using timestamp + thread ID for uniqueness.
fn generate_peer_id() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wire_roundtrip() {
        let payload = b"hello world";
        let encoded = encode_wire(MSG_SYNC, payload);
        let mut cursor = std::io::Cursor::new(encoded);
        let (msg_type, decoded) = read_wire(&mut cursor).unwrap();
        assert_eq!(msg_type, MSG_SYNC);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_wire_empty_payload() {
        let encoded = encode_wire(MSG_PEER_LEFT, &[]);
        let mut cursor = std::io::Cursor::new(encoded);
        let (msg_type, decoded) = read_wire(&mut cursor).unwrap();
        assert_eq!(msg_type, MSG_PEER_LEFT);
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_awareness_serde_roundtrip() {
        let update = AwarenessUpdate {
            peer_id: 42,
            name: "alice".to_string(),
            cursor: Some((10, 5)),
            selection: None,
        };
        let json = serde_json::to_vec(&update).unwrap();
        let decoded: AwarenessUpdate = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.peer_id, 42);
        assert_eq!(decoded.name, "alice");
        assert_eq!(decoded.cursor, Some((10, 5)));
        assert!(decoded.selection.is_none());
    }

    #[test]
    fn test_host_client_sync() {
        // Start a host with a simple document.
        let mut doc = aura_core::CrdtDoc::with_text("hello");
        let snapshot = doc.save_bytes();
        let session = CollabSession::host("host", 0, snapshot).unwrap();
        let port = session.port.unwrap();

        // Give the listener a moment.
        std::thread::sleep(Duration::from_millis(50));

        // Connect a client.
        let addr = format!("127.0.0.1:{port}");
        let client = CollabSession::join("client", &addr).unwrap();

        // Wait for the snapshot to arrive.
        std::thread::sleep(Duration::from_millis(200));

        let events = client.poll_events();
        let has_snapshot = events
            .iter()
            .any(|e| matches!(e, CollabEvent::DocSnapshot(_)));
        assert!(has_snapshot, "client should have received a snapshot");

        // Host should see the PeerJoined event.
        let host_events = session.poll_events();
        let has_join = host_events
            .iter()
            .any(|e| matches!(e, CollabEvent::PeerJoined { .. }));
        assert!(has_join, "host should have received PeerJoined");

        // Clean up.
        client.shutdown();
        session.shutdown();
    }
}
