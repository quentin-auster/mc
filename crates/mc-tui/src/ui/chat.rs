use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::{App, MainView, Panel};
use crate::conversation::tree::{ActivityAction, ActivityKind};

const PREVIEW_CHARS: usize = 240;
const ACTION_PREVIEW_LINES: usize = 6;

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

    match app.main_view {
        MainView::Activity => render_activity(frame, app, chunks[0], border_style),
        MainView::File => render_file(frame, app, chunks[0], border_style),
    }

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

fn render_activity(frame: &mut Frame, app: &App, area: Rect, border_style: Style) {
    let node = &app.tree.nodes[&app.tree.active];
    let mut items = vec![
        message_item(
            "turn ",
            Color::Yellow,
            format!("{}  {}", node.hash, node.label),
        ),
        message_item(
            "ctx  ",
            Color::DarkGray,
            app.tree
                .active_path()
                .into_iter()
                .filter(|id| *id != app.tree.active)
                .map(|id| app.tree.nodes[&id].label.as_str())
                .collect::<Vec<_>>()
                .join(" > "),
        ),
    ];

    if let Some(prompt) = &node.user_content {
        push_section(
            &mut items,
            "you  ",
            Color::Green,
            prompt,
            node.prompt_expanded,
            "/expand prompt",
        );
    } else {
        items.push(message_item(
            "you  ",
            Color::DarkGray,
            "(no prompt on this turn)",
        ));
    }

    if let Some(response) = &node.assistant_content {
        push_section(
            &mut items,
            "mc   ",
            Color::Cyan,
            response,
            node.response_expanded,
            "/expand response",
        );
    } else {
        items.push(message_item(
            "mc   ",
            Color::DarkGray,
            "(no response recorded)",
        ));
    }

    if node.actions.is_empty() {
        items.push(message_item(
            "act  ",
            Color::DarkGray,
            "(no actions recorded)",
        ));
    } else {
        items.push(message_item(
            "act  ",
            Color::Yellow,
            format!("{} action(s); /expand actions", node.actions.len()),
        ));
        for (index, action) in node.actions.iter().enumerate() {
            push_action(&mut items, index, action);
        }
    }

    if let Some(status) = &app.status {
        let color = if status.is_error {
            Color::Red
        } else {
            Color::DarkGray
        };
        items.push(message_item("sys  ", color, &status.message));
    }

    let title = format!(
        " Activity  mode:{}  strategy:{}  {} ",
        app.agent_mode,
        app.edit_strategy,
        app.context_ledger.summary()
    );
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title),
    );
    frame.render_widget(list, area);
}

fn render_file(frame: &mut Frame, app: &App, area: Rect, border_style: Style) {
    let mut items = Vec::new();
    if let Some(buffer) = &app.file_buffer {
        for (index, line) in buffer.content.lines().enumerate() {
            let prefix = format!("{:>4} ", index + 1);
            let style = if index == buffer.cursor_line {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::raw(line.to_string()),
            ])));
        }
        if buffer.content.is_empty() {
            items.push(message_item("   1 ", Color::DarkGray, ""));
        }
    } else {
        items.push(message_item("file ", Color::DarkGray, "no open file"));
    }

    if let Some(status) = &app.status {
        let color = if status.is_error {
            Color::Red
        } else {
            Color::DarkGray
        };
        items.push(message_item("sys  ", color, &status.message));
    }

    let title = match &app.file_buffer {
        Some(buffer) => {
            let dirty = if buffer.is_dirty() { " modified" } else { "" };
            format!(" File  {}{}  /view activity  /save ", buffer.path, dirty)
        }
        None => " File  /open <path> or select from Files ".to_string(),
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title),
    );
    frame.render_widget(list, area);
}

fn push_section(
    items: &mut Vec<ListItem<'static>>,
    prefix: &str,
    color: Color,
    content: &str,
    expanded: bool,
    hint: &str,
) {
    if expanded {
        for line in content.lines() {
            items.push(message_item(prefix, color, line));
        }
    } else {
        items.push(message_item(
            prefix,
            color,
            format!("{}  ({hint})", truncate_preview(content, PREVIEW_CHARS)),
        ));
    }
}

fn push_action(items: &mut Vec<ListItem<'static>>, index: usize, action: &ActivityAction) {
    let color = match action.kind {
        ActivityKind::System => Color::DarkGray,
        ActivityKind::Shell => Color::Yellow,
        ActivityKind::File => Color::Magenta,
        ActivityKind::Diff => Color::Green,
        ActivityKind::Provider => Color::Cyan,
    };
    items.push(message_item(
        format!("{:>2}.  ", index + 1),
        color,
        action.title.clone(),
    ));
    let lines: Vec<&str> = action.detail.lines().collect();
    let shown = if action.expanded {
        lines.len()
    } else {
        lines.len().min(ACTION_PREVIEW_LINES)
    };
    for line in &lines[..shown] {
        let line_color = if matches!(action.kind, ActivityKind::Diff) {
            diff_line_color(line)
        } else {
            Color::Gray
        };
        items.push(message_item("     ", line_color, *line));
    }
    if !action.expanded && lines.len() > shown {
        items.push(message_item(
            "     ",
            Color::DarkGray,
            format!(
                "... {} more lines (/expand {})",
                lines.len() - shown,
                index + 1
            ),
        ));
    }
}

fn diff_line_color(line: &str) -> Color {
    if line.starts_with('+') && !line.starts_with("+++") {
        Color::Green
    } else if line.starts_with('-') && !line.starts_with("---") {
        Color::Red
    } else {
        Color::Gray
    }
}

fn truncate_preview(content: &str, max_chars: usize) -> String {
    let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
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
