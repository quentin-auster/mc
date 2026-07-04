mod agent;
mod app;
mod benchmark;
mod command;
mod context;
mod conversation;
mod diff;
mod edit;
mod provider;
mod telemetry;
mod tui;
mod ui;
mod vim;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("bench") {
        benchmark::run().map_err(anyhow::Error::msg)?;
        return Ok(());
    }

    let mut app = app::App::new();
    tui::run(&mut app).await
}
