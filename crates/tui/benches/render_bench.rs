//! Benchmarks for TUI rendering performance.
//!
//! Targets:
//! - Keystroke-to-render: <1ms
//! - Frame time (streaming AI): <16ms
//! - Syntax highlighting: reasonable on large files

use aura_core::{AuthorId, Buffer};
use aura_tui::app::App;
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
    app.mode = aura_tui::app::Mode::Insert;

    c.bench_function("keystroke_insert_render_1k", |b| {
        b.iter(|| {
            // Simulate a keystroke: insert a character then render.
            app.buffer.insert(app.cursor.col, "x", AuthorId::human());
            terminal
                .draw(|frame| aura_tui::render::draw(frame, &mut app))
                .unwrap();
            // Undo to keep buffer stable.
            app.buffer.undo();
            black_box(());
        });
    });
}

fn bench_render_with_highlights(c: &mut Criterion) {
    let mut app = make_app(5_000);
    // Ensure highlights are computed.
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
            app.scroll_row = (app.scroll_row + 40) % 9_960;
            app.cursor.row = app.scroll_row + 20;
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
);
criterion_main!(benches);
