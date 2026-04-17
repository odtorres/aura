//! Benchmarks for TUI rendering performance.
//!
//! Targets:
//! - Keystroke-to-render: <1ms
//! - Frame time (streaming AI): <16ms
//! - Syntax highlighting: reasonable on large files

use aura_core::{AuthorId, Buffer};
use aura_tui::app::{App, Mode};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Create an app with a buffer of `n` lines.
fn make_app(lines: usize) -> App {
    let mut buf = Buffer::new();
    let mut text = String::with_capacity(lines * 60);
    for i in 0..lines {
        text.push_str(&format!(
            "fn function_{i}(x: i32) -> i32 {{ x + {i} }} // line\n"
        ));
    }
    buf.insert(0, &text, AuthorId::human());
    App::new(buf)
}

fn bench_render_frame(c: &mut Criterion) {
    let mut app = make_app(1_000);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    c.bench_function("render_frame_1k_lines", |b| {
        b.iter(|| {
            terminal
                .draw(|frame| aura_tui::render::draw(frame, &mut app))
                .unwrap();
            black_box(());
        });
    });
}

fn bench_render_frame_10k(c: &mut Criterion) {
    let mut app = make_app(10_000);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    c.bench_function("render_frame_10k_lines", |b| {
        b.iter(|| {
            terminal
                .draw(|frame| aura_tui::render::draw(frame, &mut app))
                .unwrap();
            black_box(());
        });
    });
}

fn bench_keystroke_insert_render(c: &mut Criterion) {
    let mut app = make_app(1_000);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    // Switch to insert mode.
    app.mode = Mode::Insert;

    c.bench_function("keystroke_insert_render_1k", |b| {
        b.iter(|| {
            // Simulate a keystroke: insert a character then render.
            let col = app.cursor().col;
            app.buffer_mut().insert(col, "x", AuthorId::human());
            terminal
                .draw(|frame| aura_tui::render::draw(frame, &mut app))
                .unwrap();
            // Undo to keep buffer stable.
            app.buffer_mut().undo();
            black_box(());
        });
    });
}

fn bench_render_with_highlights(c: &mut Criterion) {
    let mut app = make_app(5_000);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    // Pre-render once to populate highlight cache.
    terminal
        .draw(|frame| aura_tui::render::draw(frame, &mut app))
        .unwrap();

    c.bench_function("render_with_highlights_5k", |b| {
        b.iter(|| {
            terminal
                .draw(|frame| aura_tui::render::draw(frame, &mut app))
                .unwrap();
            black_box(());
        });
    });
}

fn bench_scroll_and_render(c: &mut Criterion) {
    let mut app = make_app(10_000);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    c.bench_function("scroll_and_render_10k", |b| {
        b.iter(|| {
            // Simulate scrolling through the file.
            let scroll = app.tab().scroll_row;
            let new_scroll = (scroll + 40) % 9_960;
            app.tab_mut().scroll_row = new_scroll;
            app.tab_mut().cursor.row = new_scroll + 20;
            terminal
                .draw(|frame| aura_tui::render::draw(frame, &mut app))
                .unwrap();
            black_box(());
        });
    });
}

/// Render a 100K-line file repeatedly without any buffer edits. Exercises the
/// per-frame hot paths (bracket-depth scan, scope queries) that used to be
/// O(total_lines). With caches in place, steady-state renders should stay in
/// the <1 ms range.
fn bench_render_frame_100k(c: &mut Criterion) {
    let mut app = make_app(100_000);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    // Prime caches.
    terminal
        .draw(|frame| aura_tui::render::draw(frame, &mut app))
        .unwrap();

    c.bench_function("render_frame_100k_lines", |b| {
        b.iter(|| {
            terminal
                .draw(|frame| aura_tui::render::draw(frame, &mut app))
                .unwrap();
            black_box(());
        });
    });
}

/// Scroll through a 100K-line file. Verifies that scrolling alone doesn't
/// invalidate cached bracket depths (they should be a one-time build).
fn bench_scroll_and_render_100k(c: &mut Criterion) {
    let mut app = make_app(100_000);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    // Prime caches.
    terminal
        .draw(|frame| aura_tui::render::draw(frame, &mut app))
        .unwrap();

    c.bench_function("scroll_and_render_100k", |b| {
        b.iter(|| {
            let scroll = app.tab().scroll_row;
            let new_scroll = (scroll + 40) % 99_960;
            app.tab_mut().scroll_row = new_scroll;
            app.tab_mut().cursor.row = new_scroll + 20;
            terminal
                .draw(|frame| aura_tui::render::draw(frame, &mut app))
                .unwrap();
            black_box(());
        });
    });
}

criterion_group!(
    benches,
    bench_render_frame,
    bench_render_frame_10k,
    bench_keystroke_insert_render,
    bench_render_with_highlights,
    bench_scroll_and_render,
    bench_render_frame_100k,
    bench_scroll_and_render_100k,
);
criterion_main!(benches);
