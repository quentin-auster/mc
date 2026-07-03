use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

use crate::app::{App, Panel};

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = matches!(app.active_panel, Panel::Files);
    let border_style = if is_focused {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app
        .fs
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            app.layout
                .fs_rows
                .push((area.y + 1 + index as u16, entry.path.clone()));
            let is_cursor = is_focused && index == app.fs.cursor;
            let marker = if entry.is_dir { "▸ " } else { "  " };
            let suffix = if entry.is_dir { "/" } else { "" };
            let color = if entry.is_dir {
                Color::Cyan
            } else {
                Color::Gray
            };
            let cursor_style = if is_cursor {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(if is_cursor { "> " } else { "  " }, cursor_style),
                Span::styled(marker, Style::default().fg(color)),
                Span::styled(
                    format!("{}{suffix}", entry.name),
                    Style::default().fg(color),
                ),
            ]))
        })
        .collect();

    let title = if is_focused {
        format!(" Files  {}  ↑↓ enter ", display_dir(&app.fs.current_dir))
    } else {
        format!(" Files  {} ", display_dir(&app.fs.current_dir))
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title),
    );
    frame.render_widget(list, area);
}

fn display_dir(path: &str) -> &str {
    if path.is_empty() { "." } else { path }
}
