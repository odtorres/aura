//! Benchmarks for CRDT and buffer operations.
//!
//! Target: <1ms per edit operation on a 10K line file.

use aura_core::{AuthorId, Buffer};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Generate a buffer with `n` lines of text.
fn make_buffer(lines: usize) -> Buffer {
    let mut buf = Buffer::new();
    let mut text = String::new();
    for i in 0..lines {
        text.push_str(&format!(
            "// Line {i}: some typical source code content here\n"
        ));
    }
    buf.insert(0, &text, AuthorId::human());
    buf
}

fn bench_insert_char(c: &mut Criterion) {
    let mut buf = make_buffer(10_000);
    let author = AuthorId::human();
    let mid = buf.len_chars() / 2;

    c.bench_function("insert_char_10k_lines", |b| {
        b.iter(|| {
            buf.insert(black_box(mid), "x", author.clone());
        });
    });
}

fn bench_insert_char_ai(c: &mut Criterion) {
    let mut buf = make_buffer(10_000);
    let author = AuthorId::ai("agent-1");
    let mid = buf.len_chars() / 2;

    c.bench_function("insert_char_ai_10k_lines", |b| {
        b.iter(|| {
            buf.insert(black_box(mid), "x", author.clone());
        });
    });
}

fn bench_delete_char(c: &mut Criterion) {
    let mut buf = make_buffer(10_000);
    let author = AuthorId::human();
    let mid = buf.len_chars() / 2;

    c.bench_function("delete_char_10k_lines", |b| {
        b.iter(|| {
            buf.delete(black_box(mid), mid + 1, author.clone());
            // Re-insert to keep the buffer the same size.
            buf.insert(mid, "x", author.clone());
        });
    });
}

fn bench_undo(c: &mut Criterion) {
    let mut buf = make_buffer(10_000);
    let author = AuthorId::human();

    c.bench_function("undo_10k_lines", |b| {
        b.iter(|| {
            let mid = buf.len_chars() / 2;
            buf.insert(mid, "x", author.clone());
            buf.undo();
        });
    });
}

fn bench_line_author_query(c: &mut Criterion) {
    let buf = make_buffer(10_000);

    c.bench_function("line_author_query_10k", |b| {
        b.iter(|| {
            for line in (0..10_000).step_by(100) {
                black_box(buf.line_author(line));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_insert_char,
    bench_insert_char_ai,
    bench_delete_char,
    bench_undo,
    bench_line_author_query,
);
criterion_main!(benches);
