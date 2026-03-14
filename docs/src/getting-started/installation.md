# Installation

## From crates.io

The simplest way to install AURA:

```bash
cargo install aura-editor
```

This downloads, compiles, and installs the `aura` binary into `~/.cargo/bin/`.

## Pre-built Binaries

Pre-built binaries are available from [GitHub Releases](https://github.com/odtorres/aura/releases) for:

- **macOS**: `aarch64-apple-darwin` (Apple Silicon), `x86_64-apple-darwin` (Intel)
- **Linux**: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
- **Windows**: `x86_64-pc-windows-msvc`

### Shell installer

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh
```

## Homebrew

```bash
brew install aura-editor/tap/aura
```

## Building from Source

### Prerequisites

- **Rust 1.75+** — install via [rustup](https://rustup.rs/)
- A C compiler (for tree-sitter grammar compilation and SQLite)

### Build

```bash
git clone https://github.com/odtorres/aura.git
cd aura
cargo build --release
```

The binary is at `target/release/aura`.

### Install locally

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
