use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::app::App;

mod chat;
mod tree;

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(frame.area());

    chat::render(frame, app, chunks[0]);
    tree::render(frame, app, chunks[1]);
}
