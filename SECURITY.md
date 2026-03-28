# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in AURA, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email **odtorres891118@gmail.com** with:

1. A description of the vulnerability
2. Steps to reproduce the issue
3. The potential impact
4. Any suggested fixes (optional)

### What to Expect

- **Acknowledgment**: We will acknowledge your report within 48 hours.
- **Assessment**: We will assess the vulnerability and determine its severity within 7 days.
- **Fix**: Critical vulnerabilities will be patched as soon as possible. We aim for a fix within 14 days for high-severity issues.
- **Disclosure**: We will coordinate with you on public disclosure timing. We follow a 90-day responsible disclosure policy.
- **Credit**: We will credit you in the release notes (unless you prefer to remain anonymous).

### Scope

The following are in scope for security reports:

- **Code execution vulnerabilities** in the editor or plugin system
- **Path traversal** issues in file operations
- **Authentication bypass** in collaborative editing sessions
- **Data exposure** through the MCP server or conversation store
- **Denial of service** in the networking layer
- **Command injection** through the embedded terminal or plugin system

### Out of Scope

- Vulnerabilities in dependencies (please report to the upstream project)
- Issues that require physical access to the machine
- Social engineering attacks
- Denial of service through resource exhaustion (e.g., opening very large files)

## Security Best Practices for Users

- **API keys**: Set `ANTHROPIC_API_KEY` as an environment variable, not in config files
- **Collaborative editing**: Use `require_auth = true` in `aura.toml` when hosting sessions on the network
- **Plugins**: Only install Lua plugins from trusted sources — plugins have access to the filesystem
- **MCP server**: The MCP server listens on localhost by default. Only change `bind_address` to `0.0.0.0` when you intend to allow remote access

## Dependencies

AURA uses the following security-relevant dependencies:

- **rustls** for TLS (future encrypted collaboration)
- **rusqlite** (bundled SQLite) for conversation storage
- **mlua** (vendored Lua 5.4) for the plugin runtime

We monitor dependencies for known vulnerabilities and update regularly.
