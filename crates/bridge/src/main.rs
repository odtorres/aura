//! `aura-mcp-bridge` — Stdio-to-TCP proxy for connecting Claude Code to AURA's MCP server.
//!
//! Claude Code speaks MCP over **stdio** (Content-Length framed JSON-RPC).
//! AURA's MCP server listens on **TCP** with the same framing.
//! This binary bridges the two transports so Claude Code can call AURA tools.

use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};

/// Discover the running AURA instance's MCP server address.
///
/// Resolution order:
/// 1. `AURA_MCP_PORT` env var (connects to `127.0.0.1:<port>`)
/// 2. `~/.aura/mcp.json` discovery file written by AURA on startup
fn discover_aura() -> Result<(String, u16)> {
    // 1. Explicit env override.
    if let Ok(port_str) = std::env::var("AURA_MCP_PORT") {
        let port: u16 = port_str
            .parse()
            .context("AURA_MCP_PORT is not a valid port number")?;
        return Ok(("127.0.0.1".to_string(), port));
    }

    // 2. Discovery file.
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME environment variable not set")?;
    let discovery_path = home.join(".aura").join("mcp.json");

    let contents = std::fs::read_to_string(&discovery_path).with_context(|| {
        format!(
            "No running AURA instance found (tried {}). Start AURA first.",
            discovery_path.display()
        )
    })?;

    let doc: serde_json::Value =
        serde_json::from_str(&contents).context("Invalid JSON in ~/.aura/mcp.json")?;

    let host = doc["host"].as_str().unwrap_or("127.0.0.1").to_string();
    let port = doc["port"]
        .as_u64()
        .context("Missing or invalid 'port' in ~/.aura/mcp.json")? as u16;

    // Best-effort PID liveness check.
    if let Some(pid) = doc["pid"].as_u64() {
        // On Unix, signal 0 checks existence without sending a signal.
        #[cfg(unix)]
        {
            // SAFETY: kill(pid, 0) is a standard POSIX liveness check.
            let alive = unsafe { libc_kill(pid as i32, 0) } == 0;
            if !alive {
                let _ = std::fs::remove_file(&discovery_path);
                bail!(
                    "AURA instance (PID {}) is no longer running. \
                     Stale discovery file removed. Start AURA first.",
                    pid
                );
            }
        }
        let _ = pid; // suppress unused warning on non-unix
    }

    Ok((host, port))
}

/// Minimal libc kill wrapper to avoid pulling in the `libc` crate.
#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, sig) }
}

/// Connect to the AURA MCP server with a timeout.
fn connect_to_aura(host: &str, port: u16) -> Result<TcpStream> {
    let addr = format!("{}:{}", host, port);
    let stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .context("Invalid address for AURA MCP server")?,
        Duration::from_secs(2),
    )
    .with_context(|| {
        format!(
            "Could not connect to AURA MCP server at {}. Is AURA running?",
            addr
        )
    })?;
    Ok(stream)
}

/// Read a Content-Length framed message from any buffered reader.
fn read_message(reader: &mut impl BufRead) -> io::Result<String> {
    let mut content_length: usize = 0;

    // Read headers until empty line.
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            content_length = len_str
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad content length"))?;
        }
    }

    if content_length == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length header",
        ));
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8 in message body"))
}

/// Write a Content-Length framed message to any writer.
fn write_message(writer: &mut impl Write, msg: &str) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", msg.len())?;
    writer.write_all(msg.as_bytes())?;
    writer.flush()
}

/// Write a JSON-RPC error response to stdout and exit.
fn fatal_jsonrpc_error(message: &str) -> ! {
    let error = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32000,
            "message": message
        }
    });
    let msg = error.to_string();
    let _ = write_message(&mut io::stdout().lock(), &msg);
    process::exit(1);
}

/// Set up file-based logging to `~/.aura/bridge.log`.
///
/// We must never write logs to stdout (that's the MCP transport).
fn init_logging() {
    let log_path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".aura").join("bridge.log"));

    if let Some(path) = log_path {
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            tracing_subscriber::fmt()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_target(false)
                .init();
            return;
        }
    }

    // Fallback: discard logs (never write to stdout).
    tracing_subscriber::fmt()
        .with_writer(io::sink)
        .with_ansi(false)
        .init();
}

fn main() {
    init_logging();

    tracing::info!("aura-mcp-bridge starting");

    // Discover and connect.
    let (host, port) = match discover_aura() {
        Ok(v) => v,
        Err(e) => fatal_jsonrpc_error(&format!("{:#}", e)),
    };

    tracing::info!("Connecting to AURA MCP server at {}:{}", host, port);

    let stream = match connect_to_aura(&host, port) {
        Ok(s) => s,
        Err(e) => fatal_jsonrpc_error(&format!("{:#}", e)),
    };

    let stream_clone = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => fatal_jsonrpc_error(&format!("Failed to clone TCP stream: {}", e)),
    };

    tracing::info!("Connected. Bridging stdio <-> TCP");

    let shutdown = Arc::new(AtomicBool::new(false));

    // Thread 1: stdin (Claude Code) -> TCP (AURA)
    let shutdown_t1 = shutdown.clone();
    let stdin_to_tcp = std::thread::spawn(move || {
        let mut stdin_reader = BufReader::new(io::stdin().lock());
        let mut tcp_writer = stream;

        loop {
            if shutdown_t1.load(Ordering::Relaxed) {
                break;
            }
            match read_message(&mut stdin_reader) {
                Ok(msg) => {
                    tracing::debug!("stdin -> tcp: {} bytes", msg.len());
                    if let Err(e) = write_message(&mut tcp_writer, &msg) {
                        tracing::error!("Failed to write to TCP: {}", e);
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    tracing::info!("stdin EOF, shutting down");
                    break;
                }
                Err(e) => {
                    tracing::error!("Error reading stdin: {}", e);
                    break;
                }
            }
        }
        shutdown_t1.store(true, Ordering::Relaxed);
    });

    // Thread 2: TCP (AURA) -> stdout (Claude Code)
    let shutdown_t2 = shutdown.clone();
    let tcp_to_stdout = std::thread::spawn(move || {
        let mut tcp_reader = BufReader::new(stream_clone);
        let mut stdout_writer = io::stdout().lock();

        loop {
            if shutdown_t2.load(Ordering::Relaxed) {
                break;
            }
            match read_message(&mut tcp_reader) {
                Ok(msg) => {
                    tracing::debug!("tcp -> stdout: {} bytes", msg.len());
                    if let Err(e) = write_message(&mut stdout_writer, &msg) {
                        tracing::error!("Failed to write to stdout: {}", e);
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    tracing::info!("TCP EOF (AURA disconnected), shutting down");
                    break;
                }
                Err(e) => {
                    tracing::error!("Error reading TCP: {}", e);
                    break;
                }
            }
        }
        shutdown_t2.store(true, Ordering::Relaxed);
    });

    // Wait for both threads. When one side closes, the other will follow.
    let _ = stdin_to_tcp.join();
    let _ = tcp_to_stdout.join();

    tracing::info!("aura-mcp-bridge exiting");
}
