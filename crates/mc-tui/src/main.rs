mod agent;
mod app;
mod command;
mod context;
mod conversation;
mod diff;
mod edit;
mod provider;
mod tui;
mod ui;
mod vim;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = app::App::new();
    tui::run(&mut app).await
}
