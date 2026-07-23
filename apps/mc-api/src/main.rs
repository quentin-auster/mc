use std::net::{Ipv4Addr, SocketAddr};

use axum::{Router, routing::get};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn app() -> Router {
    Router::new().route("/health", get(|| async { "ok" }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    if std::env::args().nth(1).as_deref() == Some("migrate") {
        let database_url = std::env::var("DATABASE_URL")?;
        mc_infrastructure::migrate(&database_url).await?;
        return Ok(());
    }

    let port = std::env::var("MC_API_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8080);
    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let listener = TcpListener::bind(address).await?;

    info!(service = "mc-api", %address, "service listening");
    axum::serve(listener, app()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::app;

    #[tokio::test]
    async fn health_endpoint_reports_ok() {
        let response = app()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert!(response.status().is_success());
    }
}
