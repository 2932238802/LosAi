use crate::{
    error::{GatewayError, Result},
    state::AppState,
};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub role: String,
    pub exp: usize,
}

pub fn hash_password(value: &str) -> anyhow::Result<String> {
    Ok(Argon2::default()
        .hash_password(value.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .to_string())
}

pub fn verify_password(value: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded)
        .map(|hash| {
            Argon2::default()
                .verify_password(value.as_bytes(), &hash)
                .is_ok()
        })
        .unwrap_or(false)
}

pub async fn ensure_admin(db: &PgPool, email: &str, password: &str) -> anyhow::Result<()> {
    if sqlx::query("SELECT id FROM users WHERE email=$1")
        .bind(email)
        .fetch_optional(db)
        .await?
        .is_none()
    {
        sqlx::query("INSERT INTO users(email,password_hash,role)VALUES($1,$2,'ADMIN')")
            .bind(email)
            .bind(hash_password(password)?)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub async fn login(state: &AppState, email: &str, password: &str) -> Result<(String, String)> {
    let row = sqlx::query("SELECT id,password_hash,role,enabled FROM users WHERE email=$1")
        .bind(email)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .ok_or(GatewayError::Unauthorized)?;
    if !row.get::<bool, _>("enabled") || !verify_password(password, row.get("password_hash")) {
        return Err(GatewayError::Unauthorized);
    }
    let role: String = row.get("role");
    let token = encode(
        &Header::default(),
        &Claims {
            sub: row.get("id"),
            role: role.clone(),
            exp: (Utc::now().timestamp() + 86400) as usize,
        },
        &EncodingKey::from_secret(state.config.session_secret.as_bytes()),
    )
    .map_err(|_| GatewayError::Internal)?;
    Ok((token, role))
}

fn claims(state: &AppState, value: &str) -> Result<Claims> {
    let token = value
        .strip_prefix("Bearer ")
        .ok_or(GatewayError::Unauthorized)?;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.config.session_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| GatewayError::Unauthorized)
}

pub fn require_user(state: &AppState, value: &str) -> Result<Claims> {
    let data = claims(state, value)?;
    if data.role == "ADMIN" {
        return Err(GatewayError::Forbidden);
    }
    Ok(data)
}

pub fn require_admin(state: &AppState, value: &str) -> Result<Claims> {
    let data = claims(state, value)?;
    if data.role != "ADMIN" {
        return Err(GatewayError::Forbidden);
    }
    Ok(data)
}
