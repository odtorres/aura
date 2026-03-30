#!/usr/bin/env python3
"""
MCP bridge: connects Claude Code (stdio) to AURA's MCP/ACP server (TCP).

Claude Code spawns this script as an MCP server. The script reads
~/.aura/acp.json to discover AURA's port, connects via TCP, and
relays JSON-RPC messages between Claude Code's stdio and AURA's TCP.

Usage in .claude/settings.json:
{
  "mcpServers": {
    "aura": {
      "command": "python3",
      "args": ["/path/to/aura-editor/scripts/aura-mcp-bridge.py"]
    }
  }
}
"""

import json
import os
import socket
import sys
import threading


def find_aura_port():
    """Read the ACP discovery file to find AURA's port."""
    home = os.path.expanduser("~")
    for filename in ["acp.json", "mcp.json"]:
        path = os.path.join(home, ".aura", filename)
        if os.path.exists(path):
            try:
                with open(path) as f:
                    data = json.load(f)
                return data.get("host", "127.0.0.1"), data.get("port")
            except (json.JSONDecodeError, KeyError):
                continue
    return None, None


def read_jsonrpc_message(stream):
    """Read a Content-Length framed JSON-RPC message."""
    headers = {}
    while True:
        line = b""
        while not line.endswith(b"\r\n"):
            ch = stream.read(1)
            if not ch:
                return None
            line += ch
        line = line.strip()
        if not line:
            break
        if line.startswith(b"Content-Length:"):
            headers["Content-Length"] = int(line.split(b":")[1].strip())

    length = headers.get("Content-Length")
    if length is None:
        return None

    body = stream.read(length)
    if not body or len(body) < length:
        return None

    return body.decode("utf-8")


def write_jsonrpc_message(stream, body):
    """Write a Content-Length framed JSON-RPC message."""
    encoded = body.encode("utf-8")
    header = f"Content-Length: {len(encoded)}\r\n\r\n"
    stream.write(header.encode("utf-8"))
    stream.write(encoded)
    stream.flush()


def tcp_to_stdout(sock, sock_file):
    """Read from TCP socket and write to stdout."""
    while True:
        try:
            msg = read_jsonrpc_message(sock_file)
            if msg is None:
                break
            write_jsonrpc_message(sys.stdout.buffer, msg)
        except Exception:
            break


def main():
    host, port = find_aura_port()
    if port is None:
        # AURA is not running — send an error response for any request
        error_msg = json.dumps({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32603,
                "message": "AURA editor is not running. Start AURA first."
            }
        })
        write_jsonrpc_message(sys.stdout.buffer, error_msg)
        sys.exit(1)

    # Connect to AURA's ACP/MCP server
    try:
        sock = socket.create_connection((host, port), timeout=5)
    except Exception as e:
        error_msg = json.dumps({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32603,
                "message": f"Cannot connect to AURA at {host}:{port}: {e}"
            }
        })
        write_jsonrpc_message(sys.stdout.buffer, error_msg)
        sys.exit(1)

    sock_file = sock.makefile("rb")

    # Start thread to relay TCP responses to stdout
    relay_thread = threading.Thread(
        target=tcp_to_stdout, args=(sock, sock_file), daemon=True
    )
    relay_thread.start()

    # Main loop: read from stdin, forward to TCP
    stdin = sys.stdin.buffer
    try:
        while True:
            msg = read_jsonrpc_message(stdin)
            if msg is None:
                break
            # Forward to AURA
            encoded = msg.encode("utf-8")
            header = f"Content-Length: {len(encoded)}\r\n\r\n"
            sock.sendall(header.encode("utf-8"))
            sock.sendall(encoded)
    except (BrokenPipeError, ConnectionResetError):
        pass
    finally:
        sock.close()


if __name__ == "__main__":
    main()
