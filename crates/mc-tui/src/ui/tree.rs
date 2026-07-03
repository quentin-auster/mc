use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

use crate::app::{App, Panel};
use crate::conversation::tree::Node;

pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = matches!(app.active_panel, Panel::Tree);
    let border_style = if is_focused {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let entries = app.tree.display_entries();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, &(id, depth))| {
            app.layout.tree_rows.push((area.y + 1 + i as u16, id));
            let node = &app.tree.nodes[&id];
            let is_active = id == app.tree.active;
            let is_cursor = is_focused && i == app.tree_cursor;
            ListItem::new(node_line(node, depth, is_active, is_cursor))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(if is_focused {
                " Tree  ↑↓ navigate  enter jump "
            } else {
                " Tree "
            }),
    );
    frame.render_widget(list, area);
}

fn node_line(node: &Node, depth: usize, is_active: bool, is_cursor: bool) -> Line<'static> {
    let indent = "  ".repeat(depth);

    let cursor_prefix = if is_cursor { "> " } else { "  " };

    let (marker, node_style) = if is_active {
        (
            "● ",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
    } else if node.is_branch() {
        ("◆ ", Style::default().fg(Color::Magenta))
    } else if node.is_merge() {
        ("⊕ ", Style::default().fg(Color::Yellow))
    } else {
        ("○ ", Style::default().fg(Color::DarkGray))
    };

    let hash_style = if is_active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let cursor_style = if is_cursor {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Line::from(vec![
        Span::styled(cursor_prefix, cursor_style),
        Span::raw(indent),
        Span::styled(marker, node_style),
        Span::styled(node.hash.clone(), hash_style),
        Span::raw("  "),
        Span::styled(node.label.clone(), node_style),
    ])
}
