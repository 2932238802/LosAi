use crate::{
    error::{GatewayError, Result},
    routes::api::audit,
    services::{auth, crypto},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use chrono::{DateTime, Utc};
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Instant;
use uuid::Uuid;

fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<auth::Claims> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or(GatewayError::Unauthorized)?;
    auth::require_admin(state, token)
}

fn require_user(state: &AppState, headers: &HeaderMap) -> Result<auth::Claims> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or(GatewayError::Unauthorized)?;
    auth::require_user(state, token)
}

fn normalized_chat_endpoint(base_url: &str) -> Result<String> {
    let normalized = base_url.trim().trim_end_matches('/');
    if !(normalized.starts_with("https://") || normalized.starts_with("http://")) {
        return Err(GatewayError::Validation("Base URL 格式无效".to_owned()));
    }
    let versioned = if normalized.ends_with("/v1") {
        normalized.to_owned()
    } else {
        format!("{normalized}/v1")
    };
    Ok(format!("{versioned}/chat/completions"))
}

#[derive(Deserialize)]
pub struct EnabledInput {
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UserUpdateInput {
    pub email: String,
    pub role: String,
    pub plan_id: Option<Uuid>,
    pub credits_balance: i64,
}

#[derive(Deserialize)]
pub struct PasswordInput {
    pub password: String,
}

pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<UserUpdateInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let email = input.email.trim().to_lowercase();
    if !email.contains('@')
        || input.credits_balance < 0
        || !matches!(input.role.as_str(), "ADMIN" | "CUSTOMER")
    {
        return Err(GatewayError::Validation("用户参数无效".to_owned()));
    }
    let affected = sqlx::query(
        "UPDATE users SET email=$2,role=$3,plan_id=$4,credits_balance=$5,updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(email)
    .bind(&input.role)
    .bind(input.plan_id)
    .bind(input.credits_balance)
    .execute(&state.db)
    .await
    .map_err(|_| GatewayError::Validation("邮箱已存在或套餐无效".to_owned()))?
    .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("用户不存在".to_owned()));
    }
    audit(&state, claims.sub, "UPDATE", "USER", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"message":"用户信息已更新"}),
    ))
}

pub async fn update_user_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<EnabledInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    if claims.sub == id && !input.enabled {
        return Err(GatewayError::Validation(
            "不能禁用当前管理员账号".to_owned(),
        ));
    }
    let mut tx = state.db.begin().await.map_err(|_| GatewayError::Internal)?;
    let affected = sqlx::query("UPDATE users SET enabled=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("用户不存在".to_owned()));
    }
    if !input.enabled {
        sqlx::query("UPDATE api_keys SET enabled=false,updated_at=now() WHERE user_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| GatewayError::Internal)?;
    }
    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    audit(
        &state,
        claims.sub,
        if input.enabled { "ENABLE" } else { "DISABLE" },
        "USER",
        id,
    )
    .await;
    Ok(Json(
        serde_json::json!({"id":id,"enabled":input.enabled,"message":if input.enabled {"用户已启用"} else {"用户已禁用，所属 API 密钥已撤销"}}),
    ))
}

pub async fn reset_user_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<PasswordInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    if input.password.len() < 8 {
        return Err(GatewayError::Validation("密码至少需要 8 位".to_owned()));
    }
    let hash = auth::hash_password(&input.password).map_err(|_| GatewayError::Internal)?;
    let affected = sqlx::query("UPDATE users SET password_hash=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(hash)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("用户不存在".to_owned()));
    }
    audit(&state, claims.sub, "RESET_PASSWORD", "USER", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"message":"用户密码已重置"}),
    ))
}

pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    if claims.sub == id {
        return Err(GatewayError::Validation(
            "不能删除当前管理员账号".to_owned(),
        ));
    }
    let mut tx = state.db.begin().await.map_err(|_| GatewayError::Internal)?;
    let affected = sqlx::query("UPDATE users SET enabled=false,updated_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("用户不存在".to_owned()));
    }
    sqlx::query("UPDATE api_keys SET enabled=false,updated_at=now() WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?;
    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    audit(&state, claims.sub, "ARCHIVE", "USER", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"archived":true,"message":"用户已归档，历史使用记录已保留"}),
    ))
}

fn default_currency() -> String {
    "CNY".to_owned()
}

#[derive(Deserialize)]
pub struct PlanUpdateInput {
    pub name: String,
    pub monthly_credits: i64,
    pub rpm_limit: i32,
    pub tpm_limit: i32,
    pub max_concurrency: i32,
    #[serde(default)]
    pub monthly_request_limit: i64,
    #[serde(default)]
    pub price_cents: i64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub description: String,
}

pub async fn update_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<PlanUpdateInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    if input.name.trim().is_empty()
        || input.monthly_credits < 0
        || input.rpm_limit <= 0
        || input.tpm_limit < 0
        || input.max_concurrency <= 0
        || input.monthly_request_limit < 0
        || input.price_cents < 0
        || input.currency.trim().is_empty()
    {
        return Err(GatewayError::Validation("套餐参数无效".to_owned()));
    }
    let affected = sqlx::query("UPDATE plans SET name=$2,monthly_credits=$3,price_cents=$4,currency=$5,description=$6,rpm_limit=$7,tpm_limit=$8,max_concurrency=$9,monthly_request_limit=$10,updated_at=now() WHERE id=$1")
        .bind(id).bind(input.name.trim()).bind(input.monthly_credits).bind(input.price_cents).bind(input.currency.trim().to_uppercase()).bind(input.description.trim()).bind(input.rpm_limit).bind(input.tpm_limit).bind(input.max_concurrency).bind(input.monthly_request_limit)
        .execute(&state.db).await.map_err(|_|GatewayError::Validation("套餐名称已存在或参数无效".to_owned()))?.rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("套餐不存在".to_owned()));
    }
    audit(&state, claims.sub, "UPDATE", "PLAN", id).await;
    Ok(Json(serde_json::json!({"id":id,"message":"套餐已更新"})))
}

pub async fn update_plan_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<EnabledInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let affected = sqlx::query("UPDATE plans SET enabled=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(input.enabled)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("套餐不存在".to_owned()));
    }
    audit(
        &state,
        claims.sub,
        if input.enabled { "ENABLE" } else { "DISABLE" },
        "PLAN",
        id,
    )
    .await;
    Ok(Json(
        serde_json::json!({"id":id,"enabled":input.enabled,"message":if input.enabled{"套餐已启用"}else{"套餐已禁用"}}),
    ))
}

pub async fn delete_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let users = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE plan_id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    if users > 0 {
        sqlx::query("UPDATE plans SET enabled=false,updated_at=now() WHERE id=$1")
            .bind(id)
            .execute(&state.db)
            .await
            .map_err(|_| GatewayError::Internal)?;
        audit(&state, claims.sub, "ARCHIVE", "PLAN", id).await;
        return Ok(Json(
            serde_json::json!({"id":id,"archived":true,"message":"套餐仍被用户使用，已安全禁用"}),
        ));
    }
    let affected = sqlx::query("DELETE FROM plans WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("套餐不存在".to_owned()));
    }
    audit(&state, claims.sub, "DELETE", "PLAN", id).await;
    Ok(Json(serde_json::json!({"id":id,"message":"套餐已删除"})))
}

#[derive(Deserialize)]
pub struct KeyUpdateInput {
    pub name: String,
    pub expires_at: Option<DateTime<Utc>>,
}

async fn update_key_owned(
    state: &AppState,
    id: Uuid,
    owner: Option<Uuid>,
    input: KeyUpdateInput,
) -> Result<()> {
    let name = input.name.trim();
    if name.is_empty()
        || name.chars().count() > 64
        || input.expires_at.is_some_and(|value| value <= Utc::now())
    {
        return Err(GatewayError::Validation(
            "密钥名称或过期时间无效".to_owned(),
        ));
    }
    let affected = if let Some(user_id) = owner {
        sqlx::query(
            "UPDATE api_keys SET name=$3,expires_at=$4,updated_at=now() WHERE id=$1 AND user_id=$2",
        )
        .bind(id)
        .bind(user_id)
        .bind(name)
        .bind(input.expires_at)
        .execute(&state.db)
        .await
    } else {
        sqlx::query("UPDATE api_keys SET name=$2,expires_at=$3,updated_at=now() WHERE id=$1")
            .bind(id)
            .bind(name)
            .bind(input.expires_at)
            .execute(&state.db)
            .await
    }
    .map_err(|_| GatewayError::Validation("密钥参数无效".to_owned()))?
    .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("API 密钥不存在".to_owned()));
    }
    Ok(())
}

pub async fn admin_update_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<KeyUpdateInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    update_key_owned(&state, id, None, input).await?;
    audit(&state, claims.sub, "UPDATE", "API_KEY", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"message":"API 密钥已更新"}),
    ))
}
pub async fn user_update_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<KeyUpdateInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_user(&state, &headers)?;
    update_key_owned(&state, id, Some(claims.sub), input).await?;
    Ok(Json(
        serde_json::json!({"id":id,"message":"API 密钥已更新"}),
    ))
}
async fn status_key_owned(
    state: &AppState,
    id: Uuid,
    owner: Option<Uuid>,
    enabled: bool,
) -> Result<()> {
    let affected = if let Some(user_id) = owner {
        sqlx::query("UPDATE api_keys SET enabled=$3,updated_at=now() WHERE id=$1 AND user_id=$2")
            .bind(id)
            .bind(user_id)
            .bind(enabled)
            .execute(&state.db)
            .await
    } else {
        sqlx::query("UPDATE api_keys SET enabled=$2,updated_at=now() WHERE id=$1")
            .bind(id)
            .bind(enabled)
            .execute(&state.db)
            .await
    }
    .map_err(|_| GatewayError::Internal)?
    .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("API 密钥不存在".to_owned()));
    }
    Ok(())
}
pub async fn admin_key_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<EnabledInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    status_key_owned(&state, id, None, input.enabled).await?;
    audit(
        &state,
        claims.sub,
        if input.enabled { "ENABLE" } else { "DISABLE" },
        "API_KEY",
        id,
    )
    .await;
    Ok(Json(
        serde_json::json!({"id":id,"enabled":input.enabled,"message":if input.enabled{"API 密钥已启用"}else{"API 密钥已禁用"}}),
    ))
}
pub async fn user_key_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<EnabledInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_user(&state, &headers)?;
    status_key_owned(&state, id, Some(claims.sub), input.enabled).await?;
    Ok(Json(
        serde_json::json!({"id":id,"enabled":input.enabled,"message":if input.enabled{"API 密钥已启用"}else{"API 密钥已禁用"}}),
    ))
}

async fn delete_key_safe(state: &AppState, id: Uuid, owner: Option<Uuid>) -> Result<bool> {
    let used = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM usage_records WHERE api_key_id=$1)",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|_| GatewayError::Internal)?;
    if used {
        status_key_owned(state, id, owner, false).await?;
        return Ok(true);
    }
    let affected = if let Some(user_id) = owner {
        sqlx::query("DELETE FROM api_keys WHERE id=$1 AND user_id=$2")
            .bind(id)
            .bind(user_id)
            .execute(&state.db)
            .await
    } else {
        sqlx::query("DELETE FROM api_keys WHERE id=$1")
            .bind(id)
            .execute(&state.db)
            .await
    }
    .map_err(|_| GatewayError::Internal)?
    .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("API 密钥不存在".to_owned()));
    }
    Ok(false)
}
pub async fn admin_delete_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let archived = delete_key_safe(&state, id, None).await?;
    audit(
        &state,
        claims.sub,
        if archived { "ARCHIVE" } else { "DELETE" },
        "API_KEY",
        id,
    )
    .await;
    Ok(Json(
        serde_json::json!({"id":id,"archived":archived,"message":if archived{"密钥已有使用记录，已安全禁用"}else{"API 密钥已删除"}}),
    ))
}
pub async fn user_delete_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_user(&state, &headers)?;
    let archived = delete_key_safe(&state, id, Some(claims.sub)).await?;
    Ok(Json(
        serde_json::json!({"id":id,"archived":archived,"message":if archived{"密钥已有使用记录，已安全禁用"}else{"API 密钥已删除"}}),
    ))
}

#[derive(Deserialize)]
pub struct ProviderUpdateInput {
    pub name: String,
    pub base_url: String,
    pub adapter: String,
    pub priority: i32,
    pub weight: i32,
}

pub async fn update_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<ProviderUpdateInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let endpoint = normalized_chat_endpoint(&input.base_url)?;
    let base = endpoint.trim_end_matches("/chat/completions");
    if input.name.trim().is_empty()
        || input.adapter.trim().is_empty()
        || input.priority < 0
        || input.weight <= 0
    {
        return Err(GatewayError::Validation("Provider 参数无效".to_owned()));
    }
    let affected=sqlx::query("UPDATE providers SET name=$2,base_url=$3,adapter=$4,priority=$5,weight=$6,updated_at=now() WHERE id=$1").bind(id).bind(input.name.trim()).bind(base).bind(input.adapter.trim()).bind(input.priority).bind(input.weight).execute(&state.db).await.map_err(|_|GatewayError::Validation("Provider 名称已存在或参数无效".to_owned()))?.rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("Provider 不存在".to_owned()));
    }
    audit(&state, claims.sub, "UPDATE", "PROVIDER", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"message":"Provider 已更新"}),
    ))
}
pub async fn provider_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<EnabledInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let mut tx = state.db.begin().await.map_err(|_| GatewayError::Internal)?;
    let affected = sqlx::query("UPDATE providers SET enabled=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("Provider 不存在".to_owned()));
    }
    if !input.enabled {
        sqlx::query("UPDATE model_routes SET enabled=false,updated_at=now() WHERE provider_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| GatewayError::Internal)?;
    }
    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    audit(
        &state,
        claims.sub,
        if input.enabled { "ENABLE" } else { "DISABLE" },
        "PROVIDER",
        id,
    )
    .await;
    Ok(Json(
        serde_json::json!({"id":id,"enabled":input.enabled,"message":if input.enabled{"Provider 已启用"}else{"Provider 已禁用，关联路由已停用"}}),
    ))
}
pub async fn delete_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let linked=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM model_routes WHERE provider_id=$1 UNION ALL SELECT 1 FROM provider_credentials WHERE provider_id=$1)").bind(id).fetch_one(&state.db).await.map_err(|_|GatewayError::Internal)?;
    if linked {
        sqlx::query("UPDATE providers SET enabled=false,updated_at=now() WHERE id=$1")
            .bind(id)
            .execute(&state.db)
            .await
            .map_err(|_| GatewayError::Internal)?;
        audit(&state, claims.sub, "ARCHIVE", "PROVIDER", id).await;
        return Ok(Json(
            serde_json::json!({"id":id,"archived":true,"message":"Provider 存在关联资源，已安全禁用"}),
        ));
    }
    let affected = sqlx::query("DELETE FROM providers WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("Provider 不存在".to_owned()));
    }
    audit(&state, claims.sub, "DELETE", "PROVIDER", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"message":"Provider 已删除"}),
    ))
}
pub async fn check_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let row = sqlx::query("SELECT base_url FROM providers WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .ok_or_else(|| GatewayError::Validation("Provider 不存在".to_owned()))?;
    let endpoint = normalized_chat_endpoint(row.get("base_url"))?;
    let started = Instant::now();
    let result = state
        .http
        .get(endpoint.trim_end_matches("/chat/completions").to_owned() + "/models")
        .send()
        .await;
    let latency = started.elapsed().as_millis() as i64;
    let ok = result
        .as_ref()
        .is_ok_and(|response| response.status().is_success() || response.status().as_u16() == 401);
    sqlx::query("UPDATE providers SET last_health_check_at=now(),failure_count=CASE WHEN $2 THEN 0 ELSE failure_count+1 END WHERE id=$1").bind(id).bind(ok).execute(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"id":id,"ok":ok,"latency_ms":latency,"message":if ok{"Provider 网络可达"}else{"Provider 无法连接"}}),
    ))
}

#[derive(Deserialize)]
pub struct CredentialUpdateInput {
    pub label: String,
    pub secret: Option<String>,
    pub priority: i32,
    pub weight: i32,
}
#[derive(Deserialize)]
pub struct CredentialStatusInput {
    pub status: String,
}

pub async fn update_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<CredentialUpdateInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    if input.label.trim().is_empty() || input.priority < 0 || input.weight <= 0 {
        return Err(GatewayError::Validation("凭证参数无效".to_owned()));
    }
    let affected=if let Some(secret)=input.secret.as_deref().map(str::trim).filter(|v|!v.is_empty()){let encrypted=crypto::encrypt(secret,&state.config.credential_encryption_key).map_err(|_|GatewayError::Internal)?;let fingerprint=hex::encode(Sha256::digest(secret.as_bytes()))[..16].to_owned();sqlx::query("UPDATE provider_credentials SET label=$2,encrypted_secret=$3,secret_fingerprint=$4,priority=$5,weight=$6,status='ACTIVE',cooldown_until=NULL,updated_at=now() WHERE id=$1").bind(id).bind(input.label.trim()).bind(encrypted).bind(fingerprint).bind(input.priority).bind(input.weight).execute(&state.db).await}else{sqlx::query("UPDATE provider_credentials SET label=$2,priority=$3,weight=$4,updated_at=now() WHERE id=$1").bind(id).bind(input.label.trim()).bind(input.priority).bind(input.weight).execute(&state.db).await}.map_err(|_|GatewayError::Validation("凭证标签已存在或参数无效".to_owned()))?.rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("凭证不存在".to_owned()));
    }
    audit(&state, claims.sub, "UPDATE", "CREDENTIAL", id).await;
    Ok(Json(serde_json::json!({"id":id,"message":"凭证已更新"})))
}
pub async fn credential_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<CredentialStatusInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    if !matches!(input.status.as_str(), "ACTIVE" | "DISABLED") {
        return Err(GatewayError::Validation(
            "凭证状态只允许 ACTIVE 或 DISABLED".to_owned(),
        ));
    }
    let affected=sqlx::query("UPDATE provider_credentials SET status=$2,cooldown_until=NULL,updated_at=now() WHERE id=$1").bind(id).bind(&input.status).execute(&state.db).await.map_err(|_|GatewayError::Internal)?.rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("凭证不存在".to_owned()));
    }
    audit(&state, claims.sub, &input.status, "CREDENTIAL", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"status":input.status,"message":"凭证状态已更新"}),
    ))
}
pub async fn delete_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let affected = sqlx::query("DELETE FROM provider_credentials WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("凭证不存在".to_owned()));
    }
    audit(&state, claims.sub, "DELETE", "CREDENTIAL", id).await;
    Ok(Json(serde_json::json!({"id":id,"message":"凭证已删除"})))
}
pub async fn check_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let row=sqlx::query("SELECT c.encrypted_secret,p.base_url FROM provider_credentials c JOIN providers p ON p.id=c.provider_id WHERE c.id=$1").bind(id).fetch_optional(&state.db).await.map_err(|_|GatewayError::Internal)?.ok_or_else(||GatewayError::Validation("凭证不存在".to_owned()))?;
    let secret = crypto::decrypt(
        row.get("encrypted_secret"),
        &state.config.credential_encryption_key,
    )
    .map_err(|_| GatewayError::Internal)?;
    let endpoint = normalized_chat_endpoint(row.get("base_url"))?;
    let started = Instant::now();
    let response = state
        .http
        .get(endpoint.trim_end_matches("/chat/completions").to_owned() + "/models")
        .header(AUTHORIZATION, format!("Bearer {secret}"))
        .send()
        .await;
    let latency = started.elapsed().as_millis() as i64;
    let (ok, status) = match response {
        Ok(value) => (value.status().is_success(), value.status().as_u16()),
        Err(_) => (false, 0),
    };
    let new_status = if ok {
        "ACTIVE"
    } else if matches!(status, 401 | 403) {
        "INVALID"
    } else {
        "ACTIVE"
    };
    sqlx::query("UPDATE provider_credentials SET status=$2,last_used_at=CASE WHEN $3 THEN now() ELSE last_used_at END,last_error_at=CASE WHEN $3 THEN last_error_at ELSE now() END,updated_at=now() WHERE id=$1").bind(id).bind(new_status).bind(ok).execute(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"id":id,"ok":ok,"status_code":status,"latency_ms":latency,"message":if ok{"凭证检测成功"}else{"凭证检测失败"}}),
    ))
}

#[derive(Deserialize)]
pub struct ModelUpdateInput {
    pub model_name: String,
    pub input_rate_micros: i64,
    pub output_rate_micros: i64,
    pub rpm_limit: i32,
    pub tpm_limit: i64,
    pub max_concurrency: i32,
    pub monthly_request_limit: i64,
}
pub async fn update_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<ModelUpdateInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    if input.model_name.trim().is_empty()
        || input.input_rate_micros < 0
        || input.output_rate_micros < 0
        || input.rpm_limit <= 0
        || input.tpm_limit < 0
        || input.max_concurrency <= 0
        || input.monthly_request_limit < 0
    {
        return Err(GatewayError::Validation("模型参数无效".to_owned()));
    }
    let affected=sqlx::query("UPDATE models SET model_name=$2,input_rate_micros=$3,output_rate_micros=$4,rpm_limit=$5,tpm_limit=$6,max_concurrency=$7,monthly_request_limit=$8,updated_at=now() WHERE id=$1").bind(id).bind(input.model_name.trim()).bind(input.input_rate_micros).bind(input.output_rate_micros)
        .bind(input.rpm_limit)
        .bind(input.tpm_limit)
        .bind(input.max_concurrency)
        .bind(input.monthly_request_limit)
        .execute(&state.db).await.map_err(|_|GatewayError::Validation("模型名称已存在或参数无效".to_owned()))?.rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("模型不存在".to_owned()));
    }
    audit(&state, claims.sub, "UPDATE", "MODEL", id).await;
    Ok(Json(serde_json::json!({"id":id,"message":"模型已更新"})))
}

#[derive(Deserialize)]
pub struct RouteUpdateInput {
    pub provider_id: Uuid,
    pub upstream_model: String,
    pub priority: i32,
    pub weight: i32,
}
pub async fn update_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<RouteUpdateInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    if input.upstream_model.trim().is_empty() || input.priority < 0 || input.weight <= 0 {
        return Err(GatewayError::Validation("路由参数无效".to_owned()));
    }
    let affected=sqlx::query("UPDATE model_routes SET provider_id=$2,upstream_model=$3,priority=$4,weight=$5,updated_at=now() WHERE id=$1").bind(id).bind(input.provider_id).bind(input.upstream_model.trim()).bind(input.priority).bind(input.weight).execute(&state.db).await.map_err(|_|GatewayError::Validation("路由冲突或参数无效".to_owned()))?.rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("路由不存在".to_owned()));
    }
    audit(&state, claims.sub, "UPDATE", "MODEL_ROUTE", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"message":"模型路由已更新"}),
    ))
}
pub async fn route_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<EnabledInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let affected = sqlx::query("UPDATE model_routes SET enabled=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(input.enabled)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("路由不存在".to_owned()));
    }
    audit(
        &state,
        claims.sub,
        if input.enabled { "ENABLE" } else { "DISABLE" },
        "MODEL_ROUTE",
        id,
    )
    .await;
    Ok(Json(
        serde_json::json!({"id":id,"enabled":input.enabled,"message":if input.enabled{"路由已启用"}else{"路由已禁用"}}),
    ))
}
pub async fn delete_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let claims = require_admin(&state, &headers)?;
    let affected = sqlx::query("DELETE FROM model_routes WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if affected == 0 {
        return Err(GatewayError::Validation("路由不存在".to_owned()));
    }
    audit(&state, claims.sub, "DELETE", "MODEL_ROUTE", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"message":"模型路由已删除"}),
    ))
}
pub async fn check_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let row=sqlx::query("SELECT r.upstream_model,p.name provider_name,p.base_url,c.encrypted_secret FROM model_routes r JOIN providers p ON p.id=r.provider_id AND p.enabled JOIN provider_credentials c ON c.provider_id=p.id AND c.status='ACTIVE' WHERE r.id=$1 AND r.enabled ORDER BY c.priority LIMIT 1").bind(id).fetch_optional(&state.db).await.map_err(|_|GatewayError::Internal)?.ok_or(GatewayError::NoHealthyProvider)?;
    let secret = crypto::decrypt(
        row.get("encrypted_secret"),
        &state.config.credential_encryption_key,
    )
    .map_err(|_| GatewayError::Internal)?;
    let endpoint = normalized_chat_endpoint(row.get("base_url"))?;
    let started = Instant::now();
    let response=state.http.post(endpoint).header(AUTHORIZATION,format!("Bearer {secret}")).json(&serde_json::json!({"model":row.get::<String,_>("upstream_model"),"messages":[{"role":"user","content":"请只回复：连接正常"}],"max_tokens":8,"stream":false})).send().await;
    let latency = started.elapsed().as_millis() as i64;
    let (ok, status) = match response {
        Ok(value) => (value.status().is_success(), value.status().as_u16()),
        Err(_) => (false, 0),
    };
    Ok(Json(
        serde_json::json!({"id":id,"ok":ok,"status_code":status,"latency_ms":latency,"provider":row.get::<String,_>("provider_name"),"message":if ok{"路由检测成功"}else{"路由检测失败"}}),
    ))
}

pub async fn audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let rows=sqlx::query("SELECT a.id,a.action,a.resource_type,a.resource_id,a.created_at,u.email actor_email FROM audit_logs a LEFT JOIN users u ON u.id=a.actor_user_id ORDER BY a.created_at DESC LIMIT 200").fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|serde_json::json!({"id":r.get::<Uuid,_>("id"),"action":r.get::<String,_>("action"),"resource_type":r.get::<String,_>("resource_type"),"resource_id":r.get::<Option<Uuid>,_>("resource_id"),"actor_email":r.get::<Option<String>,_>("actor_email"),"created_at":r.get::<DateTime<Utc>,_>("created_at")})).collect::<Vec<_>>() }),
    ))
}
