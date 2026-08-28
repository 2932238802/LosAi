pub mod app;
pub mod config;
pub mod error;
pub mod middleware;
pub mod routes;
pub mod services;
pub mod state;

pub async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let config = config::Config::from_env()?;
    let state = state::AppState::connect(config).await?;
    let address = state.config.bind_addr.clone();
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "gateway listening");
    axum::serve(listener, app::router(state)).await?;
    Ok(())
}
