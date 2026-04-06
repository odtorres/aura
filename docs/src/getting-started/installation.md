# Installation

## Shell Installer (Recommended)

The quickest way to install AURA on macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh
```

This downloads the latest pre-built binary for your platform and installs it.

## Pre-built Binaries

Pre-built binaries are available from [GitHub Releases](https://github.com/odtorres/aura/releases) for:

- **macOS**: `aarch64-apple-darwin` (Apple Silicon), `x86_64-apple-darwin` (Intel)
- **Linux**: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
- **Windows**: `x86_64-pc-windows-msvc`

Download the appropriate archive, extract, and place the `aura` binary in your `$PATH`.

## Cargo (from source)

Install directly from the GitHub repository:

```bash
cargo install --git https://github.com/odtorres/aura.git aura
```

This clones, compiles, and installs the `aura` binary into `~/.cargo/bin/`.

**Requirements:** Rust 1.75+ and a C compiler (for tree-sitter grammars and SQLite).

## Homebrew (macOS & Linux)

```bash
brew tap odtorres/aura
brew install aura
```

Installs the latest pre-built binary. Updates with `brew upgrade aura`.

## Building from Source

```bash
git clone https://github.com/odtorres/aura.git
cd aura
cargo build --release
```

The binary is at `target/release/aura`. To install locally:

```bash
cargo install --path crates/editor
```

## Verifying Installation

```bash
aura --version
```

Or open a file to confirm everything works:

```bash
aura path/to/file.rs
```
