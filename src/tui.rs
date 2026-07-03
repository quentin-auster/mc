use std::time::Duration;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, Panel};

pub async fn run(app: &mut App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| crate::ui::render(f, app))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key);
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: event::KeyEvent) {
    if key.modifiers == KeyModifiers::CONTROL {
        if let KeyCode::Char('c') = key.code {
            app.should_quit = true;
        }
        return;
    }

    // Panel-specific arrow key handling.
    match app.active_panel {
        Panel::Tree => match key.code {
            KeyCode::Up => { app.tree_cursor_up(); return; }
            KeyCode::Down => { app.tree_cursor_down(); return; }
            KeyCode::Enter => { app.tree_cursor_jump(); return; }
            _ => {}
        },
        Panel::Chat => match key.code {
            KeyCode::Up => { app.history_up(); return; }
            KeyCode::Down => { app.history_down(); return; }
            _ => {}
        },
    }

    match key.code {
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Tab => app.toggle_panel(),
        KeyCode::Enter => app.process_input(),
        KeyCode::Backspace => { app.input.pop(); }
        KeyCode::Char(c) => app.input.push(c),
        _ => {}
    }
}
