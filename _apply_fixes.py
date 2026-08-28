from pathlib import Path
root=Path('/home/losangelous/LosAngelous/Project/LosToken')

p=root/'apps/api/src/app.rs'
s=p.read_text()
s=s.replace('http::header::HeaderName::from_static', 'axum::http::header::HeaderName::from_static')
s=s.replace('.allow_origins(allowed_origins)', '.allow_origin(allowed_origins)')
p.write_text(s)

p=root/'apps/api/src/services/auth.rs'
s=p.read_text()
s=s.replace('let r = sqlx::query', 'let row = sqlx::query')
s=s.replace('pub async fn login(state: &AppState, email: &str, password: &str) -> Result<String> {', 'pub async fn login(state: &AppState, email: &str, password: &str) -> Result<(String, String)> {')
old='''    encode(
        &Header::default(),
        &Claims {
            sub: row.get("id"),
            role: row.get("role"),
            exp: (Utc::now().timestamp() + 86400) as usize,
        },
        &EncodingKey::from_secret(state.config.session_secret.as_bytes()),
    )
    .map_err(|_| GatewayError::Internal)'''
new='''    let role: String = row.get("role");
    let token = encode(
        &Header::default(),
        &Claims { sub: row.get("id"), role: role.clone(), exp: (Utc::now().timestamp() + 86400) as usize },
        &EncodingKey::from_secret(state.config.session_secret.as_bytes()),
    ).map_err(|_| GatewayError::Internal)?;
    Ok((token, role))'''
s=s.replace(old,new)
p.write_text(s)

p=root/'apps/api/src/routes/api.rs'
s=p.read_text()
s=s.replace("pub struct Token {\n    pub access_token: String,\n    pub token_type: &'static str,\n}", "pub struct Token {\n    pub access_token: String,\n    pub token_type: &'static str,\n    pub role: String,\n}")
old='''    let token = auth::login(&state, &input.email, &input.password).await?;
    Ok(Json(Token {
        access_token: token.0,
        token_type: "Bearer",
        role: token.1,
    }))'''
new='''    let (access_token, role) = auth::login(&state, &input.email, &input.password).await?;
    Ok(Json(Token { access_token, token_type: "Bearer", role }))'''
s=s.replace(old,new)
p.write_text(s)

p=root/'apps/api/src/services/limits.rs'
s=p.read_text()
if 'pub async fn release_tokens' not in s:
    s=s.replace('pub async fn acquire(', '''pub async fn release_tokens(redis: &mut ConnectionManager, key: &str, tokens: i64) -> Result<()> {
    if tokens <= 0 { return Ok(()); }
    redis::cmd("DECRBY").arg(key).arg(tokens).query_async(redis).await.map_err(|_| GatewayError::Internal)
}

pub async fn acquire(''')
p.write_text(s)

p=root/'migrations/0003_provider_routing.sql'
p.write_text('''ALTER TABLE providers ADD COLUMN IF NOT EXISTS priority integer NOT NULL DEFAULT 100;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS weight integer NOT NULL DEFAULT 100 CHECK (weight > 0);
ALTER TABLE providers ADD COLUMN IF NOT EXISTS failure_count integer NOT NULL DEFAULT 0;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS cooldown_until timestamptz;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS last_health_check_at timestamptz;
ALTER TABLE provider_credentials ADD COLUMN IF NOT EXISTS last_used_at timestamptz;
CREATE INDEX IF NOT EXISTS idx_providers_routing ON providers(enabled, priority, weight);
CREATE INDEX IF NOT EXISTS idx_credentials_cooldown ON provider_credentials(status, cooldown_until, priority);
''')
