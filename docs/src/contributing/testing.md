# Testing

## Running Tests

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p aura-core
cargo test -p aura-tui
cargo test -p aura-ai

# Run a specific test
cargo test -p aura-core buffer::tests::test_insert
```

## Test Strategy

### Unit Tests

All buffer operations in `aura-core` have unit tests covering:

- Insert/delete at various positions (start, middle, end)
- UTF-8 handling (multi-byte characters, emojis)
- Cursor movement and clamping
- Undo/redo (global and per-author)
- Edge cases (empty buffer, single character, very long lines)

### CRDT Sync Tests

Collaborative editing has dedicated tests covering:

- Bidirectional sync convergence between two CrdtDocs
- Concurrent edits on forked documents merge correctly
- Save/load roundtrip preserves document state
- Fork produces an independent copy
- Buffer-level remote sync with snapshot + incremental reconciliation
- Multi-file session with multiple document snapshots

### Property-Based Tests

AURA uses [proptest](https://crates.io/crates/proptest) for property-based testing on the buffer:

- Random sequences of insert/delete operations should never corrupt state
- Buffer length invariants hold after any operation sequence
- Cursor-to-char and char-to-cursor round-trip correctly

Property tests catch edge cases that manual test cases miss, especially around unicode boundaries and concurrent edit sequences.

### Snapshot Tests

TUI output will be snapshot-tested using [insta](https://crates.io/crates/insta) to catch rendering regressions.

## Benchmarks

AURA uses [criterion](https://crates.io/crates/criterion) for performance benchmarks:

```bash
# Run all benchmarks
cargo bench --workspace

# Run benchmarks for core crate
cargo bench -p aura-core
```

### Performance Targets

| Operation | Target |
|-----------|--------|
| CRDT edit operation | < 1ms on 10K line file |
| Keystroke-to-render | < 1ms |
| Frame time (AI streaming) | < 16ms |
| File open (100K lines) | No perceptible lag |

## Code Quality Checks

```bash
# Clippy with all warnings
cargo clippy --workspace -- -W clippy::all

# Format check
cargo fmt --all -- --check

# Fix formatting
cargo fmt --all

# Verify no unwrap() in library code (should only appear in test modules)
grep -rn "\.unwrap()" crates/*/src/*.rs | grep -v "test\|#\[cfg(test" | grep -v "unwrap_or"
```

The project convention is **no `unwrap()` in library code**. Use `?`, `if let Ok(...)`, or `.expect("reason")` for provably-safe operations instead.

## Documentation Tests

```bash
# Build rustdoc and check for warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Open rustdoc locally
cargo doc --workspace --no-deps --open
```

## CI

The CI pipeline (`.github/workflows/ci.yml`) runs on every push and PR:

1. Format check (`cargo fmt --check`)
2. Clippy lint (`cargo clippy -- -W clippy::all -D warnings`)
3. All tests (`cargo test --workspace`)
4. Documentation build (mdBook + rustdoc)
