use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::app::App;

mod chat;
mod tree;

pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(frame.area());

    app.layout.chat = Some(chunks[0]);
    app.layout.tree = Some(chunks[1]);
    app.layout.tree_rows.clear();

    chat::render(frame, app, chunks[0]);
    tree::render(frame, app, chunks[1]);
}
