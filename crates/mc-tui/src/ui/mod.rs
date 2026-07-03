use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::app::App;

mod chat;
mod filesystem;
mod tree;

pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(frame.area());

    app.layout.chat = Some(chunks[0]);
    app.layout.tree_rows.clear();
    app.layout.fs_rows.clear();

    let sidebar = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[1]);
    app.layout.tree = Some(sidebar[0]);
    app.layout.files = Some(sidebar[1]);

    chat::render(frame, app, chunks[0]);
    tree::render(frame, app, sidebar[0]);
    filesystem::render(frame, app, sidebar[1]);
}
