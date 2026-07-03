mod agent;
mod app;
mod command;
mod conversation;
mod tui;
mod ui;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = app::App::new();
    tui::run(&mut app).await
}
