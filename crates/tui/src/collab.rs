//! Real-time collaborative editing over TCP (with optional TLS).
//!
//! Provides host-client collaboration using automerge's sync protocol.
//! The host starts a TCP listener; clients connect and exchange binary-framed
//! sync messages.  All network I/O happens on background threads, communicating
//! with the main event loop via `std::sync::mpsc` channels — the same pattern
//! used by `mcp_server.rs`, `lsp.rs`, and other subsystems.
//!
//! Supports multi-file sessions: each message carries a `file_id` (u64 hash of
//! the canonical file path) so sync and awareness are routed to the correct buffer.
//!
//! When TLS is enabled (`use_tls = true` in `aura.toml`), a self-signed certificate
//! is generated and streams are encrypted via rustls.  Because `rustls::StreamOwned`
//! cannot be split like `TcpStream::try_clone()`, a single-threaded relay bridges
//! the TLS stream to message-level channels (`WireReader` / `WireWriter`).

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

/// Shared list of connected client writers for broadcasting.
type ClientList = Arc<Mutex<Vec<(u64, Arc<Mutex<WireWriter>>)>>>;

/// Message type bytes for the wire protocol.
const MSG_SYNC: u8 = 0x01;
const MSG_AWARENESS: u8 = 0x02;
const MSG_PEER_JOINED: u8 = 0x03;
const MSG_PEER_LEFT: u8 = 0x04;
const MSG_DOC_SNAPSHOT: u8 = 0x05;
const MSG_FILE_OPENED: u8 = 0x06;
const MSG_FILE_CLOSED: u8 = 0x07;
const MSG_AUTHENTICATE: u8 = 0x08;
const MSG_TERMINAL_SNAPSHOT: u8 = 0x09;

// ---------------------------------------------------------------------------
// TLS + Auth configuration
// ---------------------------------------------------------------------------

/// Configuration for TLS and authentication in collab sessions.
#[derive(Debug, Clone)]
pub struct TlsAuthConfig {
    /// Whether TLS is enabled.
    pub use_tls: bool,
    /// Bind address for the host (e.g., "0.0.0.0" for internet).
    pub bind_address: String,
    /// Authentication token (None = no auth required).
    pub auth_token: Option<String>,
}

impl Default for TlsAuthConfig {
    fn default() -> Self {
        Self {
            use_tls: false,
            bind_address: "127.0.0.1".to_string(),
            auth_token: None,
        }
    }
}

/// Generate a random authentication token (16-byte hex string).
pub fn generate_auth_token() -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    let h1 = hasher.finish();
    // Second hash for more entropy.
    h1.hash(&mut hasher);
    let h2 = hasher.finish();
    format!("{h1:016x}{h2:016x}")
}

// ---------------------------------------------------------------------------
// Transport abstraction (WireReader / WireWriter)
// ---------------------------------------------------------------------------

/// Writer half of a collab connection (either plaintext TCP or TLS channel).
enum WireWriter {
    /// Plaintext: direct TcpStream (from `try_clone`).
    Tcp(TcpStream),
    /// TLS: sends message tuples to a relay thread.
    Channel(mpsc::Sender<(u8, Vec<u8>)>),
}

impl WireWriter {
    /// Write a framed wire message.
    fn write_message(&mut self, msg_type: u8, payload: &[u8]) -> std::io::Result<()> {
        match self {
            WireWriter::Tcp(stream) => write_wire(stream, msg_type, payload),
            WireWriter::Channel(tx) => tx
                .send((msg_type, payload.to_vec()))
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "relay closed")),
        }
    }
}

/// Reader half of a collab connection (either plaintext TCP or TLS channel).
enum WireReader {
    /// Plaintext: direct TcpStream.
    Tcp(TcpStream),
    /// TLS: receives message tuples from a relay thread.
    Channel(mpsc::Receiver<(u8, Vec<u8>)>),
}

impl WireReader {
    /// Read one framed wire message.
    fn read_message(&mut self) -> std::io::Result<(u8, Vec<u8>)> {
        match self {
            WireReader::Tcp(stream) => read_wire(stream),
            WireReader::Channel(rx) => rx.recv().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::ConnectionReset, "relay closed")
            }),
        }
    }
}

/// Split a plaintext TCP stream into independent reader and writer halves.
fn split_tcp(stream: TcpStream) -> std::io::Result<(WireReader, WireWriter)> {
    let clone = stream.try_clone()?;
    Ok((WireReader::Tcp(stream), WireWriter::Tcp(clone)))
}

/// Spawn relay threads for a TLS stream, returning channel-based reader/writer.
///
/// Uses two threads sharing the TLS stream via `Arc<Mutex<>>`: a reader thread
/// that blocks on `read_wire`, and a writer thread that blocks on channel recv.
/// This avoids the partial-read timeout issue with `read_exact`.
fn spawn_tls_relay<S: IoRead + IoWrite + Send + 'static>(
    stream: S,
    shutdown: Arc<Mutex<bool>>,
) -> std::io::Result<(WireReader, WireWriter)> {
    let (read_tx, read_rx) = mpsc::channel::<(u8, Vec<u8>)>();
    let (write_tx, write_rx) = mpsc::channel::<(u8, Vec<u8>)>();
    let stream = Arc::new(Mutex::new(stream));

    // Single relay thread: uses try-read with short timeout to alternate
    // between reading and writing without blocking either operation.
    thread::Builder::new()
        .name("tls-relay".into())
        .spawn(move || {
            // We need to handle partial reads carefully. Use a small buffer
            // to read whatever is available, then parse complete messages.
            let mut pending_read: Vec<u8> = Vec::new();

            loop {
                if *shutdown.lock().expect("lock poisoned") {
                    break;
                }

                // Try to read some bytes (non-blocking-ish via small buffer reads).
                let mut buf = [0u8; 8192];
                let mut guard = stream.lock().expect("lock poisoned");

                // Try reading available data.
                match guard.read(&mut buf) {
                    Ok(0) => {
                        drop(guard);
                        break; // EOF
                    }
                    Ok(n) => {
                        pending_read.extend_from_slice(&buf[..n]);
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        // No data available — that's fine.
                    }
                    Err(_) => {
                        drop(guard);
                        break; // Connection error.
                    }
                }

                // Parse complete messages from the buffer.
                while pending_read.len() >= 5 {
                    // Need at least 4 (length) + 1 (type) bytes.
                    let total_len =
                        u32::from_be_bytes(pending_read[..4].try_into().unwrap()) as usize;
                    if total_len == 0 || pending_read.len() < 4 + total_len {
                        break; // Incomplete message.
                    }
                    let msg_type = pending_read[4];
                    let payload = pending_read[5..4 + total_len].to_vec();
                    pending_read.drain(..4 + total_len);
                    drop(guard);
                    if read_tx.send((msg_type, payload)).is_err() {
                        return;
                    }
                    guard = stream.lock().expect("lock poisoned");
                }

                // Drain pending writes.
                loop {
                    match write_rx.try_recv() {
                        Ok((msg_type, payload)) => {
                            if write_wire(&mut *guard, msg_type, &payload).is_err() {
                                return;
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }

                drop(guard);
                // Small sleep to avoid busy-looping when idle.
                thread::sleep(Duration::from_millis(5));
            }
        })?;

    Ok((WireReader::Channel(read_rx), WireWriter::Channel(write_tx)))
}

// ---------------------------------------------------------------------------
// TLS infrastructure
// ---------------------------------------------------------------------------

/// Generate a self-signed TLS certificate and private key for collab hosting.
///
/// Returns (cert_der, key_der) suitable for building a rustls ServerConfig.
fn generate_tls_cert() -> Result<
    (
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    ),
    String,
> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .map_err(|e| format!("cert generation failed: {e}"))?;
    let cert_der = rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());
    let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(
        rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der().to_vec()),
    );
    Ok((vec![cert_der], key_der))
}

/// Build a rustls ServerConfig from a self-signed cert.
fn build_tls_server_config() -> Result<Arc<rustls::ServerConfig>, String> {
    // Ensure a crypto provider is installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (certs, key) = generate_tls_cert()?;
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("TLS server config failed: {e}"))?;
    Ok(Arc::new(config))
}

/// Build a rustls ClientConfig that accepts any certificate (for self-signed).
fn build_tls_client_config() -> Arc<rustls::ClientConfig> {
    // Ensure a crypto provider is installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertVerifier))
        .with_no_client_auth();
    Arc::new(config)
}

/// Certificate verifier that accepts any certificate (for self-signed collab).
#[derive(Debug)]
struct NoCertVerifier;

impl rustls::client::danger::ServerCertVerifier for NoCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Compute a deterministic file identifier from a canonical path.
///
/// Uses `DefaultHasher` to produce a `u64` that is consistent across peers
/// as long as the canonical path is the same.
pub fn file_id_from_path(path: &std::path::Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.as_os_str().hash(&mut hasher);
    hasher.finish()
}

/// Prepend an 8-byte big-endian file_id to a payload.
fn prepend_file_id(file_id: u64, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + payload.len());
    buf.extend_from_slice(&file_id.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// Extract an 8-byte big-endian file_id from the start of a payload.
/// Returns (file_id, remaining_payload). Falls back to file_id=0 for short payloads.
fn extract_file_id(payload: &[u8]) -> (u64, Vec<u8>) {
    if payload.len() >= 8 {
        let file_id = u64::from_be_bytes(payload[..8].try_into().unwrap());
        (file_id, payload[8..].to_vec())
    } else {
        (0, payload.to_vec())
    }
}

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
    /// Which file this awareness applies to (0 = legacy/default).
    #[serde(default)]
    pub file_id: u64,
    /// Cursor position as (row, col), if any.
    pub cursor: Option<(usize, usize)>,
    /// Selection range as ((start_row, start_col), (end_row, end_col)).
    pub selection: Option<((usize, usize), (usize, usize))>,
    /// Viewport scroll row (top visible line).
    #[serde(default)]
    pub scroll_row: Option<usize>,
    /// Viewport scroll column (leftmost visible column).
    #[serde(default)]
    pub scroll_col: Option<usize>,
}

// ---------------------------------------------------------------------------
// Events and commands (main loop ↔ network threads)
// ---------------------------------------------------------------------------

/// Events sent from network threads to the main event loop.
#[derive(Debug)]
pub enum CollabEvent {
    /// A sync message arrived from a peer for a specific file.
    SyncMessage {
        /// Which peer sent it.
        peer_id: u64,
        /// Which file this sync applies to (0 = legacy/default).
        file_id: u64,
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
    /// Document snapshot received for a specific file.
    DocSnapshot {
        /// Which file this snapshot is for.
        file_id: u64,
        /// File path (so the client can open the correct tab).
        path: String,
        /// Serialized automerge document.
        data: Vec<u8>,
    },
    /// Host opened a new file.
    FileOpened {
        /// File identifier.
        file_id: u64,
        /// File path.
        path: String,
    },
    /// Host closed a file.
    FileClosed {
        /// File identifier.
        file_id: u64,
    },
    /// Client is attempting to reconnect.
    Reconnecting {
        /// Current retry attempt number.
        attempt: u32,
    },
    /// Client successfully reconnected.
    Reconnected,
    /// Host shared a terminal screen snapshot.
    TerminalSnapshot {
        /// JSON-serialized snapshot data.
        data: Vec<u8>,
    },
    /// An error occurred on the network layer.
    Error(String),
}

/// Commands sent from the main event loop to the network threads.
#[derive(Debug, Clone)]
pub enum CollabCommand {
    /// Broadcast a sync message for a specific file to all peers.
    BroadcastSync {
        /// Which file this sync applies to.
        file_id: u64,
        /// Raw automerge sync message bytes.
        data: Vec<u8>,
    },
    /// Broadcast an awareness update.
    BroadcastAwareness(AwarenessUpdate),
    /// Notify peers that a file was opened (includes snapshot for new joiners).
    NotifyFileOpened {
        /// File identifier.
        file_id: u64,
        /// File path.
        path: String,
        /// Serialized automerge document snapshot.
        snapshot: Vec<u8>,
    },
    /// Notify peers that a file was closed.
    NotifyFileClosed {
        /// File identifier.
        file_id: u64,
    },
    /// Broadcast a terminal screen snapshot to all peers (host only).
    BroadcastTerminalSnapshot {
        /// JSON-serialized TerminalSnapshot.
        data: Vec<u8>,
    },
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
    /// Automerge sync states per file (file_id → SyncState).
    pub sync_states: HashMap<u64, SyncState>,
    /// Latest awareness per file (file_id → AwarenessUpdate).
    pub awareness: HashMap<u64, AwarenessUpdate>,
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
    /// Disconnected peers with retained sync states (host only).
    /// Maps peer_id → (per-file sync states, disconnect_time).
    disconnected_peers: HashMap<u64, (HashMap<u64, SyncState>, std::time::Instant)>,
    /// Authentication token for this session (host only, for display).
    pub auth_token: Option<String>,
    /// Shutdown flag shared with threads.
    shutdown: Arc<Mutex<bool>>,
}

impl CollabSession {
    /// Start hosting a collaboration session on the given port (0 = random).
    ///
    /// `files` is a list of `(file_id, path, snapshot_bytes)` for each open file.
    pub fn host(
        display_name: &str,
        port: u16,
        files: Vec<(u64, String, Vec<u8>)>,
        tls_auth: &TlsAuthConfig,
    ) -> std::io::Result<Self> {
        let bind_addr = format!("{}:{port}", tls_auth.bind_address);
        let listener = TcpListener::bind(&bind_addr)?;
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
        let file_snapshots = Arc::new(Mutex::new(files));

        // Build TLS server config if enabled.
        let tls_server_config = if tls_auth.use_tls {
            match build_tls_server_config() {
                Ok(config) => {
                    tracing::info!("TLS enabled for collab session");
                    Some(config)
                }
                Err(e) => {
                    tracing::warn!("TLS setup failed, falling back to plaintext: {e}");
                    None
                }
            }
        } else {
            None
        };

        // Accept thread: listen for incoming connections.
        let event_tx_accept = event_tx.clone();
        let shutdown_accept = shutdown_clone.clone();
        let file_snaps_for_accept = file_snapshots.clone();
        let file_snaps_for_cmd = file_snapshots;
        let auth_token_for_accept = tls_auth.auth_token.clone().map(Arc::new);
        let tls_config_for_accept = tls_server_config;
        thread::Builder::new()
            .name("collab-accept".to_string())
            .spawn(move || {
                for stream_result in listener.incoming() {
                    if *shutdown_accept.lock().expect("lock poisoned") {
                        break;
                    }
                    let stream = match stream_result {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

                    let event_tx = event_tx_accept.clone();
                    let clients = clients_for_accept.clone();
                    let snaps = file_snaps_for_accept.lock().expect("lock poisoned").clone();
                    let shutdown_peer = shutdown_accept.clone();
                    let auth_token = auth_token_for_accept.clone();
                    let tls_config = tls_config_for_accept.clone();

                    thread::Builder::new()
                        .name("collab-peer".to_string())
                        .spawn(move || {
                            // Build transport pair: TLS or plaintext.
                            let transport = if let Some(ref config) = tls_config {
                                match rustls::ServerConnection::new(config.clone()) {
                                    Ok(tls_conn) => {
                                        let _ = stream
                                            .set_read_timeout(Some(Duration::from_millis(50)));
                                        let tls_stream = rustls::StreamOwned::new(tls_conn, stream);
                                        spawn_tls_relay(tls_stream, shutdown_peer.clone())
                                    }
                                    Err(e) => {
                                        tracing::warn!("TLS handshake failed: {e}");
                                        return;
                                    }
                                }
                            } else {
                                split_tcp(stream)
                            };
                            let (reader, writer) = match transport {
                                Ok(pair) => pair,
                                Err(e) => {
                                    tracing::warn!("Transport setup failed: {e}");
                                    return;
                                }
                            };
                            host_handle_peer(
                                reader,
                                writer,
                                event_tx,
                                clients,
                                snaps,
                                shutdown_peer,
                                auth_token,
                            );
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
                    if *shutdown_cmd.lock().expect("lock poisoned") {
                        break;
                    }
                    match cmd {
                        CollabCommand::BroadcastSync { file_id, data } => {
                            let payload = prepend_file_id(file_id, &data);
                            broadcast(&clients_for_cmd, MSG_SYNC, &payload);
                        }
                        CollabCommand::BroadcastAwareness(update) => {
                            if let Ok(json) = serde_json::to_vec(&update) {
                                broadcast(&clients_for_cmd, MSG_AWARENESS, &json);
                            }
                        }
                        CollabCommand::NotifyFileOpened {
                            file_id,
                            path,
                            snapshot,
                        } => {
                            // Add to the snapshots list for future joiners.
                            file_snaps_for_cmd.lock().expect("lock poisoned").push((
                                file_id,
                                path.clone(),
                                snapshot.clone(),
                            ));
                            // Broadcast snapshot to current clients.
                            let payload = encode_snapshot_payload(file_id, &path, &snapshot);
                            broadcast(&clients_for_cmd, MSG_DOC_SNAPSHOT, &payload);
                        }
                        CollabCommand::NotifyFileClosed { file_id } => {
                            // Remove from snapshots.
                            file_snaps_for_cmd
                                .lock()
                                .unwrap()
                                .retain(|(id, _, _)| *id != file_id);
                            let payload = file_id.to_be_bytes().to_vec();
                            broadcast(&clients_for_cmd, MSG_FILE_CLOSED, &payload);
                        }
                        CollabCommand::BroadcastTerminalSnapshot { data } => {
                            broadcast(&clients_for_cmd, MSG_TERMINAL_SNAPSHOT, &data);
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
            auth_token: tls_auth.auth_token.clone(),
            shutdown,
        })
    }

    /// Join an existing collaboration session.
    pub fn join(
        display_name: &str,
        addr: &str,
        auth_token: Option<&str>,
        use_tls: bool,
    ) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel::<CollabCommand>();
        let shutdown = Arc::new(Mutex::new(false));

        let local_peer_id = generate_peer_id();
        let name = display_name.to_string();
        let reconnect_addr = addr.to_string();

        // Build transport pair: TLS or plaintext.
        let (mut reader, mut writer) = if use_tls {
            let tls_config = build_tls_client_config();
            let server_name = rustls::pki_types::ServerName::try_from("localhost")
                .map_err(|e| std::io::Error::other(format!("{e}")))?;
            let tls_conn = rustls::ClientConnection::new(tls_config, server_name)
                .map_err(|e| std::io::Error::other(format!("{e}")))?;
            let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
            let tls_stream = rustls::StreamOwned::new(tls_conn, stream);
            spawn_tls_relay(tls_stream, shutdown.clone())?
        } else {
            split_tcp(stream)?
        };

        // Send auth token if provided.
        if let Some(token) = auth_token {
            writer.write_message(MSG_AUTHENTICATE, token.as_bytes())?;
            // Wait for acceptance.
            match reader.read_message() {
                Ok((MSG_AUTHENTICATE, payload)) => {
                    let response = String::from_utf8_lossy(&payload).to_string();
                    if response != "accepted" {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "Authentication rejected by host",
                        ));
                    }
                }
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "Invalid auth response from host",
                    ));
                }
            }
        }

        // Send our PeerJoined message.
        let join_payload = serde_json::to_vec(&serde_json::json!({
            "peer_id": local_peer_id,
            "name": name,
        }))
        .unwrap();
        writer.write_message(MSG_PEER_JOINED, &join_payload)?;

        let writer = Arc::new(Mutex::new(writer));
        let writer_for_cmd = writer.clone();

        // Reader thread with automatic reconnection.
        let event_tx_read = event_tx;
        let shutdown_read = shutdown.clone();
        let reconnect_name = name.clone();
        let reconnect_peer_id = local_peer_id;
        let reconnect_writer = writer.clone();
        thread::Builder::new()
            .name("collab-reader".to_string())
            .spawn(move || {
                client_reader_loop(
                    reader,
                    event_tx_read,
                    shutdown_read,
                    reconnect_addr,
                    reconnect_name,
                    reconnect_peer_id,
                    reconnect_writer,
                    use_tls,
                );
            })?;

        // Command dispatch thread: forward commands to the host.
        let shutdown_cmd = shutdown.clone();
        thread::Builder::new()
            .name("collab-writer".to_string())
            .spawn(move || {
                while let Ok(cmd) = command_rx.recv() {
                    if *shutdown_cmd.lock().expect("lock poisoned") {
                        break;
                    }
                    let mut w = writer_for_cmd.lock().expect("lock poisoned");
                    match cmd {
                        CollabCommand::BroadcastSync { file_id, data } => {
                            let payload = prepend_file_id(file_id, &data);
                            let _ = w.write_message(MSG_SYNC, &payload);
                        }
                        CollabCommand::BroadcastAwareness(update) => {
                            if let Ok(json) = serde_json::to_vec(&update) {
                                let _ = w.write_message(MSG_AWARENESS, &json);
                            }
                        }
                        CollabCommand::NotifyFileOpened { .. }
                        | CollabCommand::NotifyFileClosed { .. }
                        | CollabCommand::BroadcastTerminalSnapshot { .. } => {
                            // Only host sends these notifications.
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
            auth_token: None,
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

    /// Broadcast a sync message for a specific file to all peers.
    pub fn broadcast_sync(&self, file_id: u64, data: Vec<u8>) {
        self.send_command(CollabCommand::BroadcastSync { file_id, data });
    }

    /// Broadcast an awareness update.
    pub fn broadcast_awareness(&self, update: AwarenessUpdate) {
        self.send_command(CollabCommand::BroadcastAwareness(update));
    }

    /// Broadcast a terminal screen snapshot to all peers (host only).
    pub fn broadcast_terminal_snapshot(&self, data: Vec<u8>) {
        self.send_command(CollabCommand::BroadcastTerminalSnapshot { data });
    }

    /// Notify peers that a file was opened (host only).
    pub fn notify_file_opened(&self, file_id: u64, path: &str, snapshot: Vec<u8>) {
        self.send_command(CollabCommand::NotifyFileOpened {
            file_id,
            path: path.to_string(),
            snapshot,
        });
    }

    /// Notify peers that a file was closed (host only).
    pub fn notify_file_closed(&self, file_id: u64) {
        self.send_command(CollabCommand::NotifyFileClosed { file_id });
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

    /// Register a new peer. On the host, restores retained sync states if available.
    pub fn add_peer(&mut self, peer_id: u64, name: String) {
        // Try to restore sync states from a previous connection.
        let sync_states = self.restore_peer_states(peer_id).unwrap_or_default();

        self.peers.insert(
            peer_id,
            PeerInfo {
                peer_id,
                name,
                sync_states,
                awareness: HashMap::new(),
                last_seen: std::time::Instant::now(),
            },
        );
    }

    /// Remove a peer. On the host, retains sync states for potential reconnection.
    pub fn remove_peer(&mut self, peer_id: u64) {
        if let Some(peer) = self.peers.remove(&peer_id) {
            if self.is_host {
                // Retain sync states for 5 minutes in case they reconnect.
                self.disconnected_peers
                    .insert(peer_id, (peer.sync_states, std::time::Instant::now()));
            }
        }
    }

    /// Try to restore a disconnected peer's sync states (host only).
    fn restore_peer_states(&mut self, peer_id: u64) -> Option<HashMap<u64, SyncState>> {
        if let Some((states, disconnected_at)) = self.disconnected_peers.remove(&peer_id) {
            if disconnected_at.elapsed() < Duration::from_secs(300) {
                return Some(states);
            }
        }
        None
    }

    /// Clean up expired disconnected peer states (older than 5 minutes).
    pub fn cleanup_disconnected_peers(&mut self) {
        self.disconnected_peers
            .retain(|_, (_, time)| time.elapsed() < Duration::from_secs(300));
    }

    /// Update a peer's awareness for a specific file.
    pub fn update_peer_awareness(&mut self, update: AwarenessUpdate) {
        if let Some(peer) = self.peers.get_mut(&update.peer_id) {
            peer.last_seen = std::time::Instant::now();
            let file_id = update.file_id;
            peer.awareness.insert(file_id, update);
        }
    }

    /// Shut down the session.
    pub fn shutdown(&self) {
        *self.shutdown.lock().expect("lock poisoned") = true;
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
    mut reader: WireReader,
    writer: WireWriter,
    event_tx: mpsc::Sender<CollabEvent>,
    clients: ClientList,
    file_snapshots: Vec<(u64, String, Vec<u8>)>,
    shutdown: Arc<Mutex<bool>>,
    expected_token: Option<Arc<String>>,
) {
    let writer = Arc::new(Mutex::new(writer));

    // If authentication is required, validate the token first.
    if let Some(ref expected) = expected_token {
        match reader.read_message() {
            Ok((MSG_AUTHENTICATE, payload)) => {
                let token = String::from_utf8_lossy(&payload).to_string();
                if token.trim() != expected.as_str() {
                    tracing::warn!("Peer failed authentication");
                    let _ = writer
                        .lock()
                        .expect("lock poisoned")
                        .write_message(MSG_AUTHENTICATE, b"rejected");
                    return;
                }
                let _ = writer
                    .lock()
                    .expect("lock poisoned")
                    .write_message(MSG_AUTHENTICATE, b"accepted");
            }
            _ => {
                tracing::warn!("Expected auth token, got something else");
                return;
            }
        }
    }

    // Wait for the PeerJoined message.
    let peer_id = match reader.read_message() {
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

    // Send all document snapshots (one per file).
    {
        let mut w = writer.lock().expect("lock poisoned");
        for (file_id, path, snapshot) in &file_snapshots {
            let payload = encode_snapshot_payload(*file_id, path, snapshot);
            if w.write_message(MSG_DOC_SNAPSHOT, &payload).is_err() {
                return;
            }
        }
    }

    // Register this client for broadcasting.
    clients
        .lock()
        .expect("lock poisoned")
        .push((peer_id, writer));

    // Read loop: receive messages from the peer.
    loop {
        if *shutdown.lock().expect("lock poisoned") {
            break;
        }
        match reader.read_message() {
            Ok((msg_type, payload)) => {
                let event = match msg_type {
                    MSG_SYNC => {
                        let (file_id, data) = extract_file_id(&payload);
                        CollabEvent::SyncMessage {
                            peer_id,
                            file_id,
                            data,
                        }
                    }
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
    let mut cl = clients.lock().expect("lock poisoned");
    cl.retain(|(id, _)| *id != peer_id);
}

// ---------------------------------------------------------------------------
// Snapshot payload encoding
// ---------------------------------------------------------------------------

/// Encode a snapshot payload: [8-byte file_id][4-byte path_len][path_bytes][snapshot_bytes].
fn encode_snapshot_payload(file_id: u64, path: &str, snapshot: &[u8]) -> Vec<u8> {
    let path_bytes = path.as_bytes();
    let mut buf = Vec::with_capacity(8 + 4 + path_bytes.len() + snapshot.len());
    buf.extend_from_slice(&file_id.to_be_bytes());
    buf.extend_from_slice(&(path_bytes.len() as u32).to_be_bytes());
    buf.extend_from_slice(path_bytes);
    buf.extend_from_slice(snapshot);
    buf
}

/// Decode a snapshot payload. Returns (file_id, path, snapshot_bytes).
fn decode_snapshot_payload(payload: &[u8]) -> Option<(u64, String, Vec<u8>)> {
    if payload.len() < 12 {
        return None;
    }
    let file_id = u64::from_be_bytes(payload[..8].try_into().ok()?);
    let path_len = u32::from_be_bytes(payload[8..12].try_into().ok()?) as usize;
    if payload.len() < 12 + path_len {
        return None;
    }
    let path = String::from_utf8(payload[12..12 + path_len].to_vec()).ok()?;
    let snapshot = payload[12 + path_len..].to_vec();
    Some((file_id, path, snapshot))
}

// ---------------------------------------------------------------------------
// Client reader loop with reconnection
// ---------------------------------------------------------------------------

/// Client reader loop with automatic reconnection on disconnect.
#[allow(clippy::too_many_arguments)]
fn client_reader_loop(
    initial_reader: WireReader,
    event_tx: mpsc::Sender<CollabEvent>,
    shutdown: Arc<Mutex<bool>>,
    addr: String,
    name: String,
    peer_id: u64,
    writer: Arc<Mutex<WireWriter>>,
    use_tls: bool,
) {
    let mut reader = initial_reader;

    loop {
        // Read messages until disconnected.
        loop {
            if *shutdown.lock().expect("lock poisoned") {
                return;
            }
            match reader.read_message() {
                Ok((msg_type, payload)) => {
                    let event = decode_event(msg_type, payload);
                    if event_tx.send(event).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    if *shutdown.lock().expect("lock poisoned") {
                        return;
                    }
                    break; // Disconnected — enter reconnect loop.
                }
            }
        }

        // Reconnection loop with exponential backoff.
        let mut attempt = 0u32;
        loop {
            if *shutdown.lock().expect("lock poisoned") {
                return;
            }

            attempt += 1;
            let _ = event_tx.send(CollabEvent::Reconnecting { attempt });

            // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 30s max.
            let delay = Duration::from_secs((1u64 << attempt.min(5)).min(30));
            thread::sleep(delay);

            if *shutdown.lock().expect("lock poisoned") {
                return;
            }

            // Attempt to reconnect.
            match TcpStream::connect(&addr) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

                    // Build transport pair: TLS or plaintext.
                    let (new_reader, mut new_writer) = if use_tls {
                        let tls_config = build_tls_client_config();
                        let server_name = match rustls::pki_types::ServerName::try_from("localhost")
                        {
                            Ok(sn) => sn,
                            Err(_) => continue,
                        };
                        let tls_conn = match rustls::ClientConnection::new(tls_config, server_name)
                        {
                            Ok(c) => c,
                            Err(_) => continue,
                        };
                        let tls_stream = rustls::StreamOwned::new(tls_conn, stream);
                        match spawn_tls_relay(tls_stream, shutdown.clone()) {
                            Ok(pair) => pair,
                            Err(_) => continue,
                        }
                    } else {
                        match split_tcp(stream) {
                            Ok(pair) => pair,
                            Err(_) => continue,
                        }
                    };

                    // Re-send PeerJoined.
                    let join_payload = serde_json::to_vec(&serde_json::json!({
                        "peer_id": peer_id,
                        "name": name,
                    }))
                    .unwrap();

                    if new_writer
                        .write_message(MSG_PEER_JOINED, &join_payload)
                        .is_err()
                    {
                        continue;
                    }

                    // Swap the writer so command dispatch uses the new connection.
                    *writer.lock().expect("lock poisoned") = new_writer;
                    reader = new_reader;

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
        MSG_SYNC => {
            let (file_id, data) = extract_file_id(&payload);
            CollabEvent::SyncMessage {
                peer_id: 0,
                file_id,
                data,
            }
        }
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
        MSG_DOC_SNAPSHOT => {
            if let Some((file_id, path, data)) = decode_snapshot_payload(&payload) {
                CollabEvent::DocSnapshot {
                    file_id,
                    path,
                    data,
                }
            } else {
                // Legacy fallback: single-file snapshot without file_id.
                CollabEvent::DocSnapshot {
                    file_id: 0,
                    path: String::new(),
                    data: payload,
                }
            }
        }
        MSG_FILE_OPENED => {
            if let Some((file_id, path, _)) = decode_snapshot_payload(&payload) {
                CollabEvent::FileOpened { file_id, path }
            } else {
                CollabEvent::Error("malformed file-opened message".to_string())
            }
        }
        MSG_FILE_CLOSED => {
            if payload.len() >= 8 {
                let file_id = u64::from_be_bytes(payload[..8].try_into().unwrap());
                CollabEvent::FileClosed { file_id }
            } else {
                CollabEvent::Error("malformed file-closed message".to_string())
            }
        }
        MSG_TERMINAL_SNAPSHOT => CollabEvent::TerminalSnapshot { data: payload },
        _ => CollabEvent::Error(format!("unknown message type: 0x{msg_type:02x}")),
    }
}

/// Broadcast a wire message to all connected clients.
fn broadcast(clients: &ClientList, msg_type: u8, payload: &[u8]) {
    let cl = clients.lock().expect("lock poisoned");
    for (_, writer) in cl.iter() {
        let mut w = writer.lock().expect("lock poisoned");
        let _ = w.write_message(msg_type, payload);
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
    fn test_file_id_determinism() {
        let path = std::path::PathBuf::from("/tmp/test_file.rs");
        let id1 = file_id_from_path(&path);
        let id2 = file_id_from_path(&path);
        assert_eq!(id1, id2, "same path must produce same file_id");

        let other = std::path::PathBuf::from("/tmp/other_file.rs");
        let id3 = file_id_from_path(&other);
        assert_ne!(
            id1, id3,
            "different paths should produce different file_ids"
        );
    }

    #[test]
    fn test_file_id_in_sync_payload() {
        let file_id: u64 = 0xDEAD_BEEF_CAFE_BABE;
        let data = b"sync data here";
        let payload = prepend_file_id(file_id, data);
        let (extracted_id, extracted_data) = extract_file_id(&payload);
        assert_eq!(extracted_id, file_id);
        assert_eq!(extracted_data, data);
    }

    #[test]
    fn test_file_id_legacy_fallback() {
        // Short payload (< 8 bytes) should default to file_id=0.
        let payload = b"short";
        let (file_id, data) = extract_file_id(payload);
        assert_eq!(file_id, 0);
        assert_eq!(data, payload);
    }

    #[test]
    fn test_snapshot_payload_roundtrip() {
        let file_id: u64 = 42;
        let path = "/tmp/test.rs";
        let snapshot = b"crdt snapshot bytes";
        let encoded = encode_snapshot_payload(file_id, path, snapshot);
        let (dec_id, dec_path, dec_snap) = decode_snapshot_payload(&encoded).unwrap();
        assert_eq!(dec_id, file_id);
        assert_eq!(dec_path, path);
        assert_eq!(dec_snap, snapshot);
    }

    #[test]
    fn test_awareness_serde_roundtrip() {
        let update = AwarenessUpdate {
            peer_id: 42,
            name: "alice".to_string(),
            file_id: 123,
            cursor: Some((10, 5)),
            selection: None,
            scroll_row: Some(5),
            scroll_col: Some(0),
        };
        let json = serde_json::to_vec(&update).unwrap();
        let decoded: AwarenessUpdate = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.peer_id, 42);
        assert_eq!(decoded.name, "alice");
        assert_eq!(decoded.file_id, 123);
        assert_eq!(decoded.cursor, Some((10, 5)));
        assert!(decoded.selection.is_none());
        assert_eq!(decoded.scroll_row, Some(5));
        assert_eq!(decoded.scroll_col, Some(0));
    }

    #[test]
    fn test_awareness_file_id_defaults_to_zero() {
        // Deserializing JSON without file_id should default to 0.
        // Also verifies scroll fields default to None when absent.
        let json = r#"{"peer_id":1,"name":"bob","cursor":[0,0],"selection":null}"#;
        let decoded: AwarenessUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.file_id, 0);
        assert!(decoded.scroll_row.is_none());
        assert!(decoded.scroll_col.is_none());
    }

    #[test]
    fn test_host_client_sync() {
        // Start a host with a simple document.
        let mut doc = aura_core::CrdtDoc::with_text("hello").unwrap();
        let snapshot = doc.save_bytes();
        let files = vec![(42u64, "/tmp/test.rs".to_string(), snapshot)];
        let session = CollabSession::host("host", 0, files, &TlsAuthConfig::default()).unwrap();
        let port = session.port.unwrap();

        // Give the listener a moment.
        std::thread::sleep(Duration::from_millis(50));

        // Connect a client.
        let addr = format!("127.0.0.1:{port}");
        let client = CollabSession::join("client", &addr, None, false).unwrap();

        // Wait for the snapshot to arrive.
        std::thread::sleep(Duration::from_millis(200));

        let events = client.poll_events();
        let has_snapshot = events.iter().any(|e| {
            matches!(e, CollabEvent::DocSnapshot { file_id, path, .. }
                if *file_id == 42 && path == "/tmp/test.rs")
        });
        assert!(
            has_snapshot,
            "client should have received a snapshot with file_id and path"
        );

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

    #[test]
    fn test_host_client_tls_sync() {
        // Start a host with TLS enabled.
        let mut doc = aura_core::CrdtDoc::with_text("hello tls").unwrap();
        let snapshot = doc.save_bytes();
        let files = vec![(99u64, "/tmp/tls_test.rs".to_string(), snapshot)];
        let tls_config = TlsAuthConfig {
            use_tls: true,
            ..TlsAuthConfig::default()
        };
        let session = CollabSession::host("host", 0, files, &tls_config).unwrap();
        let port = session.port.unwrap();

        std::thread::sleep(Duration::from_millis(100));

        let addr = format!("127.0.0.1:{port}");
        let client = CollabSession::join("client", &addr, None, true).unwrap();

        // TLS handshake + snapshot exchange needs a bit more time.
        std::thread::sleep(Duration::from_millis(500));

        let events = client.poll_events();
        let has_snapshot = events.iter().any(|e| {
            matches!(e, CollabEvent::DocSnapshot { file_id, path, .. }
                if *file_id == 99 && path == "/tmp/tls_test.rs")
        });
        assert!(
            has_snapshot,
            "TLS client should have received a snapshot with file_id and path"
        );

        let host_events = session.poll_events();
        let has_join = host_events
            .iter()
            .any(|e| matches!(e, CollabEvent::PeerJoined { .. }));
        assert!(has_join, "host should have received PeerJoined over TLS");

        client.shutdown();
        session.shutdown();
    }

    #[test]
    fn test_host_multi_file_snapshots() {
        // Host with two files.
        let mut doc1 = aura_core::CrdtDoc::with_text("file one").unwrap();
        let mut doc2 = aura_core::CrdtDoc::with_text("file two").unwrap();
        let files = vec![
            (1u64, "/tmp/one.rs".to_string(), doc1.save_bytes()),
            (2u64, "/tmp/two.rs".to_string(), doc2.save_bytes()),
        ];
        let session = CollabSession::host("host", 0, files, &TlsAuthConfig::default()).unwrap();
        let port = session.port.unwrap();

        std::thread::sleep(Duration::from_millis(50));

        let addr = format!("127.0.0.1:{port}");
        let client = CollabSession::join("client", &addr, None, false).unwrap();

        std::thread::sleep(Duration::from_millis(200));

        let events = client.poll_events();
        let snapshot_count = events
            .iter()
            .filter(|e| matches!(e, CollabEvent::DocSnapshot { .. }))
            .count();
        assert_eq!(snapshot_count, 2, "client should receive 2 snapshots");

        client.shutdown();
        session.shutdown();
    }
}
