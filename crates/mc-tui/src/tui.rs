use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton,
        MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::layout::Rect;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::time::Duration;

use crate::app::{App, Panel};

pub async fn run(app: &mut App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| crate::ui::render(f, app))?;

        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => handle_key(app, key),
                Event::Mouse(mouse) => handle_mouse(app, mouse),
                _ => {}
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
            KeyCode::Up => {
                app.tree_cursor_up();
                return;
            }
            KeyCode::Down => {
                app.tree_cursor_down();
                return;
            }
            KeyCode::Enter => {
                app.tree_cursor_jump();
                return;
            }
            _ => {}
        },
        Panel::Chat => match key.code {
            KeyCode::Left if is_word_jump(key.modifiers) => {
                app.cursor_word_left();
                return;
            }
            KeyCode::Right if is_word_jump(key.modifiers) => {
                app.cursor_word_right();
                return;
            }
            KeyCode::Left => {
                app.cursor_left();
                return;
            }
            KeyCode::Right => {
                app.cursor_right();
                return;
            }
            KeyCode::Home => {
                app.cursor_home();
                return;
            }
            KeyCode::End => {
                app.cursor_end();
                return;
            }
            _ => {}
        },
    }

    match key.code {
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Tab => app.toggle_panel(),
        KeyCode::Enter => app.process_input(),
        KeyCode::Backspace => {
            app.backspace();
        }
        KeyCode::Delete => {
            app.delete_forward();
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            app.insert_char(c);
        }
        KeyCode::Up if matches!(app.active_panel, Panel::Chat) => app.history_up(),
        KeyCode::Down if matches!(app.active_panel, Panel::Chat) => app.history_down(),
        _ => {}
    }
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return;
    }

    if app
        .layout
        .input
        .is_some_and(|area| contains(area, mouse.column, mouse.row))
        || app
            .layout
            .chat
            .is_some_and(|area| contains(area, mouse.column, mouse.row))
    {
        app.focus_chat();
        return;
    }

    if app
        .layout
        .tree
        .is_some_and(|area| contains(area, mouse.column, mouse.row))
    {
        app.click_tree_row(mouse.row);
    }
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
}

fn is_word_jump(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::ALT) || modifiers.contains(KeyModifiers::SUPER)
}
