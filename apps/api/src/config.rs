use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{env, sync::Arc, time::Duration};

#[derive(Debug)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub bind_addr: String,
    pub cors_origins: Vec<String>,
    pub session_secret: String,
    pub key_hash_pepper: Vec<u8>,
    pub credential_encryption_key: [u8; 32],
    pub admin_email: String,
    pub admin_password: String,
    pub upstream_timeout: Duration,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Arc<Self>> {
        let key = STANDARD
            .decode(required("CREDENTIAL_ENCRYPTION_KEY")?)
            .context("CREDENTIAL_ENCRYPTION_KEY must be base64")?;
        let credential_encryption_key: [u8; 32] = key
            .try_into()
            .map_err(|_| anyhow::anyhow!("CREDENTIAL_ENCRYPTION_KEY must decode to 32 bytes"))?;
        let session_secret = required("SESSION_SECRET")?;
        let key_hash_pepper = required("KEY_HASH_PEPPER")?.into_bytes();
        if session_secret.len() < 32 || key_hash_pepper.len() < 32 {
            bail!("secrets must be at least 32 characters")
        }

        Ok(Arc::new(Self {
            database_url: required("DATABASE_URL")?,
            redis_url: required("REDIS_URL")?,
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8080"),
            cors_origins: env_or("CORS_ORIGINS", "http://localhost:5173")
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect(),
            session_secret,
            key_hash_pepper,
            credential_encryption_key,
            admin_email: env_or("ADMIN_EMAIL", "admin@example.com"),
            admin_password: required("ADMIN_PASSWORD")?,
            upstream_timeout: Duration::from_secs(env_or("UPSTREAM_TIMEOUT_SECS", "90").parse()?),
        }))
    }
}
fn required(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| format!("missing {name}"))
}
fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}
