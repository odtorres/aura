//! AURA — AI-native Universal Reactive Authoring editor.
//!
//! Usage:
//!   aura              # Open scratch buffer
//!   aura `<file>`     # Open a file

use aura_core::Buffer;
use aura_tui::app::App;

fn main() -> anyhow::Result<()> {
    // Initialize logging (writes to file so it doesn't pollute the TUI).
    let log_file = std::fs::File::create("/tmp/aura.log").ok();
    if let Some(file) = log_file {
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_env_filter("aura=debug")
            .init();
    }

    // Parse CLI args.
    let args: Vec<String> = std::env::args().collect();
    let explicit_file = args.len() > 1;
    let buffer = if explicit_file {
        Buffer::from_file(&args[1])?
    } else {
        Buffer::new()
    };

    // Set up the terminal.
    let mut terminal = ratatui::init();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;

    // Run the editor.
    let mut app = App::new(buffer);

    // When launched without a specific file, restore the previous session.
    if !explicit_file {
        app.restore_session();
    }

    let result = app.run(&mut terminal);

    // Restore the terminal.
    crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableMouseCapture
    )?;
    ratatui::restore();

    result
}
