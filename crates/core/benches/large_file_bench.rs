//! Benchmarks for large file handling (100K+ lines).
//!
//! Targets:
//! - Insert/delete: <1ms on 100K line files
//! - CRDT compact: reasonable time after many edits
//! - Cursor conversion: fast on large buffers

use aura_core::{AuthorId, Buffer};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Generate a buffer with `n` lines of realistic source code.
fn make_large_buffer(lines: usize) -> Buffer {
    let mut buf = Buffer::new();
    let mut text = String::with_capacity(lines * 60);
    for i in 0..lines {
        text.push_str(&format!(
            "fn function_{i}(x: i32) -> i32 {{ x + {i} }} // line\n"
        ));
    }
    buf.insert(0, &text, AuthorId::human());
    buf
}

fn bench_insert_100k(c: &mut Criterion) {
    let mut buf = make_large_buffer(100_000);
    let author = AuthorId::human();
    let mid = buf.len_chars() / 2;

    c.bench_function("insert_char_100k_lines", |b| {
        b.iter(|| {
            buf.insert(black_box(mid), "x", author.clone());
        });
    });
}

fn bench_delete_100k(c: &mut Criterion) {
    let mut buf = make_large_buffer(100_000);
    let author = AuthorId::human();
    let mid = buf.len_chars() / 2;

    c.bench_function("delete_char_100k_lines", |b| {
        b.iter(|| {
            buf.delete(black_box(mid), mid + 1, author.clone());
            buf.insert(mid, "x", author.clone());
        });
    });
}

fn bench_cursor_conversion_100k(c: &mut Criterion) {
    let buf = make_large_buffer(100_000);

    c.bench_function("cursor_to_char_100k", |b| {
        b.iter(|| {
            let cursor = aura_core::Cursor::new(50_000, 10);
            black_box(buf.cursor_to_char_idx(&cursor));
        });
    });
}

fn bench_line_read_100k(c: &mut Criterion) {
    let buf = make_large_buffer(100_000);

    c.bench_function("line_read_100k", |b| {
        b.iter(|| {
            // Simulate reading a screenful of lines (50 lines).
            for line in 50_000..50_050 {
                black_box(buf.line(line));
            }
        });
    });
}

fn bench_compact_after_edits(c: &mut Criterion) {
    c.bench_function("compact_after_1000_edits", |b| {
        b.iter_with_setup(
            || {
                let mut buf = make_large_buffer(10_000);
                let author = AuthorId::human();
                // Perform 1000 edits to build up history.
                for i in 0..1_000 {
                    let pos = (i * 47) % buf.len_chars().max(1);
                    buf.insert(pos, "x", author.clone());
                }
                buf
            },
            |mut buf| {
                buf.crdt_mut().compact();
                black_box(&buf);
            },
        );
    });
}

criterion_group!(
    benches,
    bench_insert_100k,
    bench_delete_100k,
    bench_cursor_conversion_100k,
    bench_line_read_100k,
    bench_compact_after_edits,
);
criterion_main!(benches);
