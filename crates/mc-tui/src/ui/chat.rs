use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::{App, Panel};
use crate::diff::DiffLine;

const SHELL_OUTPUT_LIMIT: usize = 20;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_active = matches!(app.active_panel, Panel::Chat);
    let border_style = if is_active {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    // Conversation messages from the active tree path.
    let mut items: Vec<ListItem> = app
        .tree
        .active_path()
        .into_iter()
        .flat_map(|id| {
            let node = &app.tree.nodes[&id];
            let mut v = vec![];
            if let Some(content) = &node.user_content {
                v.push(message_item("you  ", Color::Green, content));
            }
            if let Some(content) = &node.assistant_content {
                v.push(message_item("mc   ", Color::Cyan, content));
            }
            v
        })
        .collect();

    // Shell command log — always visible, not branch-scoped.
    for entry in &app.shell_log {
        let cmd_color = if entry.success {
            Color::Yellow
        } else {
            Color::Red
        };
        items.push(message_item(
            "!    ",
            cmd_color,
            &format!("$ {}", entry.command),
        ));
        let lines: Vec<&str> = entry.output.lines().collect();
        let shown = lines.len().min(SHELL_OUTPUT_LIMIT);
        for line in &lines[..shown] {
            items.push(message_item("     ", Color::DarkGray, *line));
        }
        let hidden = lines.len().saturating_sub(SHELL_OUTPUT_LIMIT);
        if hidden > 0 {
            items.push(message_item(
                "     ",
                Color::DarkGray,
                &format!("… {hidden} more lines"),
            ));
        }
    }

    for message in &app.system_log {
        for line in message.lines() {
            items.push(message_item("sys  ", Color::DarkGray, line));
        }
    }

    for diff in &app.diff_log {
        items.push(message_item("diff ", Color::Yellow, &diff.path));
        for line in &diff.lines {
            let (color, text) = match line {
                DiffLine::Header(text) => (Color::DarkGray, text),
                DiffLine::Context(text) => (Color::Gray, text),
                DiffLine::Added(text) => (Color::Green, text),
                DiffLine::Removed(text) => (Color::Red, text),
            };
            items.push(message_item("     ", color, text));
        }
    }

    // Transient status line (command feedback, errors).
    if let Some(status) = &app.status {
        let color = if status.is_error {
            Color::Red
        } else {
            Color::DarkGray
        };
        items.push(message_item("sys  ", color, &status.message));
    }

    let title = format!(
        " Chat  mode:{}  strategy:{}  {} ",
        app.agent_mode,
        app.edit_strategy,
        app.context_ledger.summary()
    );

    let messages = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title),
    );
    frame.render_widget(messages, chunks[0]);

    let input = Paragraph::new(format!("> {}", app.input)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style),
    );
    frame.render_widget(input, chunks[1]);
    app.layout.input = Some(chunks[1]);

    if is_active {
        let cursor_x = chunks[1].x + 3 + app.input[..app.input_cursor].chars().count() as u16;
        let cursor_y = chunks[1].y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn message_item(
    prefix: impl Into<String>,
    color: Color,
    content: impl Into<String>,
) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled(
            prefix.into(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(content.into()),
    ]))
}
