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
    let mut file_arg: Option<String> = None;
    let mut collab_host = false;
    let mut collab_join: Option<String> = None;
    let mut collab_name: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" => collab_host = true,
            "--join" => {
                i += 1;
                if i < args.len() {
                    collab_join = Some(args[i].clone());
                }
            }
            "--name" => {
                i += 1;
                if i < args.len() {
                    collab_name = Some(args[i].clone());
                }
            }
            other => {
                if file_arg.is_none() {
                    file_arg = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    let explicit_file = file_arg.is_some();
    let buffer = if let Some(ref path) = file_arg {
        Buffer::from_file(path)?
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

    // Override display name if provided.
    if let Some(name) = collab_name {
        app.config.collab.display_name = name;
    }

    // Start collab session if requested.
    if collab_host {
        app.start_collab_host();
    } else if let Some(addr) = collab_join {
        app.join_collab_session(&addr);
    }

    // When launched without a specific file, restore the previous session.
    if !explicit_file {
        app.restore_session();
    }

    let result = app.run(&mut terminal);

    // Restore the terminal.
    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();

    result
}
