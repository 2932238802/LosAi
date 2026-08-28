use crate::config::Config;
use anyhow::Context;
use redis::aio::ConnectionManager;
use reqwest::Client;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub redis: ConnectionManager,
    pub http: Client,
    pub config: Arc<Config>,
}
impl AppState {
    pub async fn connect(config: Arc<Config>) -> anyhow::Result<Self> {
        let db = PgPoolOptions::new()
            .max_connections(20)
            .connect(&config.database_url)
            .await
            .context("PostgreSQL connection failed")?;
        sqlx::migrate!("../../migrations")
            .run(&db)
            .await
            .context("migration failed")?;
        let redis = ConnectionManager::new(redis::Client::open(config.redis_url.as_str())?).await?;
        let http = Client::builder().timeout(config.upstream_timeout).build()?;
        crate::services::auth::ensure_admin(&db, &config.admin_email, &config.admin_password)
            .await?;
        sqlx::query(
            "INSERT INTO providers(name, adapter, base_url, enabled, priority, weight) VALUES($1, 'openai_compatible', $2, true, 100, 100) ON CONFLICT (name) DO NOTHING",
        )
        .bind("AICodeWith")
        .bind("https://api.aicodewith.ai/v1")
        .execute(&db)
        .await
        .context("default provider initialization failed")?;
        Ok(Self {
            db,
            redis,
            http,
            config,
        })
    }
}
