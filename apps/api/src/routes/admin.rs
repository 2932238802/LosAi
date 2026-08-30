use crate::{
    error::{GatewayError, Result},
    services::{auth, crypto},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use chrono::Utc;
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Instant;
use uuid::Uuid;

fn check_admin(state: &AppState, headers: &HeaderMap) -> Result<auth::Claims> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or(GatewayError::Unauthorized)?;
    auth::require_admin(state, token)
}

fn chat_completions_endpoint(base_url: &str) -> Result<String> {
    let normalized = base_url.trim().trim_end_matches('/');
    if !(normalized.starts_with("https://") || normalized.starts_with("http://")) {
        return Err(GatewayError::Validation(
            "Provider Base URL 必须以 http:// 或 https:// 开头".to_owned(),
        ));
    }

    let versioned = if normalized.ends_with("/v1") {
        normalized.to_owned()
    } else {
        format!("{normalized}/v1")
    };

    Ok(format!("{versioned}/chat/completions"))
}

fn redact_secret(secret: &str) -> String {
    let suffix = secret
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("****{suffix}")
}

#[derive(Deserialize)]
pub struct ProviderInput {
    pub name: String,
    pub base_url: String,
    pub adapter: Option<String>,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
}

#[derive(Deserialize)]
pub struct CredentialInput {
    pub provider_id: Uuid,
    pub label: String,
    pub secret: String,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
}

#[derive(Deserialize)]
pub struct ModelInput {
    pub model_name: String,
    pub provider_id: Option<Uuid>,
    pub upstream_model: Option<String>,
    pub input_rate_micros: i64,
    pub output_rate_micros: i64,
    #[serde(default = "default_rpm")]
    pub rpm_limit: i32,
    #[serde(default = "default_tpm")]
    pub tpm_limit: i64,
    #[serde(default = "default_concurrency")]
    pub max_concurrency: i32,
    #[serde(default = "default_monthly_requests")]
    pub monthly_request_limit: i64,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
}

fn default_rpm() -> i32 {
    30
}
fn default_tpm() -> i64 {
    100_000
}
fn default_concurrency() -> i32 {
    3
}
fn default_monthly_requests() -> i64 {
    5_000
}

#[derive(Deserialize)]
pub struct RouteInput {
    pub model_id: Uuid,
    pub provider_id: Uuid,
    pub upstream_model: String,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
}

#[derive(Deserialize)]
pub struct ModelCheckInput {
    pub model_id: Uuid,
}

pub async fn providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
    let rows = sqlx::query(
        "SELECT id,name,adapter,base_url,enabled,priority,weight,failure_count,cooldown_until,last_health_check_at FROM providers ORDER BY priority,name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| GatewayError::Internal)?;

    Ok(Json(serde_json::json!({
        "data": rows.iter().map(|row| serde_json::json!({
            "id": row.get::<Uuid,_>("id"),
            "name": row.get::<String,_>("name"),
            "adapter": row.get::<String,_>("adapter"),
            "base_url": row.get::<String,_>("base_url"),
            "enabled": row.get::<bool,_>("enabled"),
            "priority": row.get::<i32,_>("priority"),
            "weight": row.get::<i32,_>("weight"),
            "failure_count": row.get::<i32,_>("failure_count"),
            "cooldown_until": row.get::<Option<chrono::DateTime<Utc>>,_>("cooldown_until"),
            "last_health_check_at": row.get::<Option<chrono::DateTime<Utc>>,_>("last_health_check_at"),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn create_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ProviderInput>,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
    let endpoint = chat_completions_endpoint(&input.base_url)?;
    let base_url = endpoint.trim_end_matches("/chat/completions");
    if input.name.trim().is_empty()
        || input.weight.unwrap_or(100) <= 0
        || input.priority.unwrap_or(100) < 0
    {
        return Err(GatewayError::Validation("Provider 参数无效".to_owned()));
    }

    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO providers(name,adapter,base_url,priority,weight) VALUES($1,$2,$3,$4,$5) RETURNING id",
    )
    .bind(input.name.trim())
    .bind(input.adapter.unwrap_or_else(|| "openai_compatible".to_owned()))
    .bind(base_url)
    .bind(input.priority.unwrap_or(100))
    .bind(input.weight.unwrap_or(100))
    .fetch_one(&state.db)
    .await
    .map_err(|_| GatewayError::Validation("Provider 名称已存在或参数无效".to_owned()))?;

    Ok(Json(
        serde_json::json!({ "id": id, "message": "Provider 创建成功" }),
    ))
}

pub async fn credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
    let rows = sqlx::query(
        "SELECT c.id,c.provider_id,p.name provider_name,c.label,c.secret_fingerprint,c.status,c.priority,c.weight,c.cooldown_until,c.last_error_at,c.last_used_at,c.created_at FROM provider_credentials c JOIN providers p ON p.id=c.provider_id ORDER BY p.name,c.priority,c.created_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| GatewayError::Internal)?;

    Ok(Json(serde_json::json!({
        "data": rows.iter().map(|row| serde_json::json!({
            "id": row.get::<Uuid,_>("id"),
            "provider_id": row.get::<Uuid,_>("provider_id"),
            "provider_name": row.get::<String,_>("provider_name"),
            "label": row.get::<String,_>("label"),
            "secret_fingerprint": row.get::<Option<String>,_>("secret_fingerprint"),
            "status": row.get::<String,_>("status"),
            "priority": row.get::<i32,_>("priority"),
            "weight": row.get::<i32,_>("weight"),
            "cooldown_until": row.get::<Option<chrono::DateTime<Utc>>,_>("cooldown_until"),
            "last_error_at": row.get::<Option<chrono::DateTime<Utc>>,_>("last_error_at"),
            "last_used_at": row.get::<Option<chrono::DateTime<Utc>>,_>("last_used_at"),
            "created_at": row.get::<chrono::DateTime<Utc>,_>("created_at"),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn create_credential(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CredentialInput>,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
    let label = input.label.trim();
    let secret = input.secret.trim();
    if label.is_empty()
        || secret.is_empty()
        || input.priority.unwrap_or(100) < 0
        || input.weight.unwrap_or(100) <= 0
    {
        return Err(GatewayError::Validation("凭证参数无效".to_owned()));
    }

    let provider_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM providers WHERE id=$1)")
            .bind(input.provider_id)
            .fetch_one(&state.db)
            .await
            .map_err(|_| GatewayError::Internal)?;
    if !provider_exists {
        return Err(GatewayError::Validation("目标 Provider 不存在".to_owned()));
    }

    let encrypted = crypto::encrypt(secret, &state.config.credential_encryption_key)
        .map_err(|_| GatewayError::Internal)?;
    let fingerprint = hex::encode(Sha256::digest(secret.as_bytes()))[..16].to_owned();
    let display = redact_secret(secret);
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO provider_credentials(provider_id,label,encrypted_secret,secret_fingerprint,priority,weight) VALUES($1,$2,$3,$4,$5,$6) RETURNING id",
    )
    .bind(input.provider_id)
    .bind(label)
    .bind(encrypted)
    .bind(fingerprint)
    .bind(input.priority.unwrap_or(100))
    .bind(input.weight.unwrap_or(100))
    .fetch_one(&state.db)
    .await
    .map_err(|_| GatewayError::Validation("凭证标签已存在或参数无效".to_owned()))?;

    Ok(Json(serde_json::json!({
        "id": id,
        "masked_secret": display,
        "message": "上游凭证已加密保存",
    })))
}

pub async fn models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
    let rows = sqlx::query(
        "SELECT id,model_name,input_rate_micros,output_rate_micros,rpm_limit,tpm_limit,max_concurrency,monthly_request_limit,enabled FROM models ORDER BY model_name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| GatewayError::Internal)?;
    Ok(Json(serde_json::json!({
        "data": rows.iter().map(|row| serde_json::json!({
            "id": row.get::<Uuid,_>("id"),
            "model_name": row.get::<String,_>("model_name"),
            "input_rate_micros": row.get::<i64,_>("input_rate_micros"),
            "output_rate_micros": row.get::<i64,_>("output_rate_micros"),
            "rpm_limit": row.get::<i32,_>("rpm_limit"),
            "tpm_limit": row.get::<i64,_>("tpm_limit"),
            "max_concurrency": row.get::<i32,_>("max_concurrency"),
            "monthly_request_limit": row.get::<i64,_>("monthly_request_limit"),
            "enabled": row.get::<bool,_>("enabled"),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn create_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ModelInput>,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
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
    let provider_id = input
        .provider_id
        .ok_or_else(|| GatewayError::Validation("新增模型必须选择 Provider".to_owned()))?;
    let upstream_model = input
        .upstream_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GatewayError::Validation("上游模型名称不能为空".to_owned()))?;
    let priority = input.priority.unwrap_or(100);
    let weight = input.weight.unwrap_or(100);
    if priority < 0 || weight <= 0 {
        return Err(GatewayError::Validation(
            "模型路由优先级或权重无效".to_owned(),
        ));
    }

    let mut tx = state.db.begin().await.map_err(|_| GatewayError::Internal)?;
    let provider_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM providers WHERE id=$1 AND enabled)",
    )
    .bind(provider_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| GatewayError::Internal)?;
    if !provider_exists {
        return Err(GatewayError::Validation(
            "Provider 不存在或未启用".to_owned(),
        ));
    }

    let model_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO models(model_name,input_rate_micros,output_rate_micros,rpm_limit,tpm_limit,max_concurrency,monthly_request_limit) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING id",
    )
    .bind(input.model_name.trim())
    .bind(input.input_rate_micros)
    .bind(input.output_rate_micros)
    .bind(input.rpm_limit)
    .bind(input.tpm_limit)
    .bind(input.max_concurrency)
    .bind(input.monthly_request_limit)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| GatewayError::Validation("模型已存在或参数无效".to_owned()))?;

    sqlx::query(
        "INSERT INTO model_routes(model_id,provider_id,upstream_model,priority,weight) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(model_id)
    .bind(provider_id)
    .bind(upstream_model)
    .bind(priority)
    .bind(weight)
    .execute(&mut *tx)
    .await
    .map_err(|_| GatewayError::Validation("模型路由已存在或参数无效".to_owned()))?;

    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    Ok(Json(serde_json::json!({
        "id": model_id,
        "provider_id": provider_id,
        "upstream_model": upstream_model,
        "message": "模型和模型路由创建成功",
    })))
}

pub async fn routes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
    let rows = sqlx::query(
        "SELECT r.id,r.model_id,r.provider_id,r.upstream_model,r.priority,r.weight,r.enabled,m.model_name,p.name provider_name FROM model_routes r JOIN models m ON m.id=r.model_id JOIN providers p ON p.id=r.provider_id ORDER BY r.priority",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| GatewayError::Internal)?;
    Ok(Json(serde_json::json!({
        "data": rows.iter().map(|row| serde_json::json!({
            "id": row.get::<Uuid,_>("id"),
            "model_id": row.get::<Uuid,_>("model_id"),
            "provider_id": row.get::<Uuid,_>("provider_id"),
            "model_name": row.get::<String,_>("model_name"),
            "provider_name": row.get::<String,_>("provider_name"),
            "upstream_model": row.get::<String,_>("upstream_model"),
            "priority": row.get::<i32,_>("priority"),
            "weight": row.get::<i32,_>("weight"),
            "enabled": row.get::<bool,_>("enabled"),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn create_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RouteInput>,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
    if input.upstream_model.trim().is_empty()
        || input.priority.unwrap_or(100) < 0
        || input.weight.unwrap_or(100) <= 0
    {
        return Err(GatewayError::Validation("路由参数无效".to_owned()));
    }
    let id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO model_routes(model_id,provider_id,upstream_model,priority,weight) VALUES($1,$2,$3,$4,$5) RETURNING id",
    )
    .bind(input.model_id)
    .bind(input.provider_id)
    .bind(input.upstream_model.trim())
    .bind(input.priority.unwrap_or(100))
    .bind(input.weight.unwrap_or(100))
    .fetch_one(&state.db)
    .await
    .map_err(|_| GatewayError::Validation("路由已存在或参数无效".to_owned()))?;
    Ok(Json(
        serde_json::json!({ "id": id, "message": "模型路由创建成功" }),
    ))
}

#[derive(Deserialize)]
pub struct ModelStatusInput {
    pub enabled: bool,
}

pub async fn delete_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    let claims = check_admin(&state, &headers)?;
    let mut tx = state.db.begin().await.map_err(|_| GatewayError::Internal)?;

    let deleted = sqlx::query_scalar::<_, Uuid>("DELETE FROM models WHERE id=$1 RETURNING id")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?;

    if deleted.is_none() {
        return Err(GatewayError::Validation("模型不存在或已经删除".to_owned()));
    }

    let still_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM models WHERE id=$1)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| GatewayError::Internal)?;

    if still_exists {
        return Err(GatewayError::Internal);
    }

    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    super::api::audit(&state, claims.sub, "DELETE", "MODEL", id).await;
    Ok(Json(serde_json::json!({
        "deleted": true,
        "id": id,
        "message": "模型已删除"
    })))
}

pub async fn update_model_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<ModelStatusInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = check_admin(&state, &headers)?;
    let mut tx = state.db.begin().await.map_err(|_| GatewayError::Internal)?;
    let result = sqlx::query("UPDATE models SET enabled=$2, updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?;
    if result.rows_affected() == 0 {
        return Err(GatewayError::Validation("模型不存在".to_owned()));
    }
    sqlx::query("UPDATE model_routes SET enabled=$2, updated_at=now() WHERE model_id=$1")
        .bind(id)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?;
    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    super::api::audit(
        &state,
        claims.sub,
        if input.enabled { "ENABLE" } else { "DISABLE" },
        "MODEL",
        id,
    )
    .await;
    Ok(Json(
        serde_json::json!({"id": id, "enabled": input.enabled, "message": if input.enabled { "模型已启用" } else { "模型已禁用，关联路由已停用" }}),
    ))
}

pub async fn check_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ModelCheckInput>,
) -> Result<Json<serde_json::Value>> {
    check_admin(&state, &headers)?;
    let route = sqlx::query(
        "SELECT m.model_name,r.upstream_model,p.id provider_id,p.name provider_name,p.base_url,c.id credential_id,c.encrypted_secret FROM models m JOIN model_routes r ON r.model_id=m.id AND r.enabled JOIN providers p ON p.id=r.provider_id AND p.enabled JOIN provider_credentials c ON c.provider_id=p.id AND c.status='ACTIVE' AND (c.cooldown_until IS NULL OR c.cooldown_until<now()) WHERE m.id=$1 AND m.enabled ORDER BY r.priority,c.priority LIMIT 1",
    )
    .bind(input.model_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| GatewayError::Internal)?
    .ok_or(GatewayError::NoHealthyProvider)?;

    let credential_id = route.get::<Uuid, _>("credential_id");
    let secret = crypto::decrypt(
        route.get("encrypted_secret"),
        &state.config.credential_encryption_key,
    )
    .map_err(|_| GatewayError::Internal)?;
    let endpoint = chat_completions_endpoint(&route.get::<String, _>("base_url"))?;
    let started = Instant::now();
    let upstream = state
        .http
        .post(endpoint)
        .header(AUTHORIZATION, format!("Bearer {secret}"))
        .json(&serde_json::json!({
            "model": route.get::<String, _>("upstream_model"),
            "messages": [{"role": "user", "content": "请只回复：连接正常"}],
            "max_tokens": 8,
            "stream": false,
        }))
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis() as i64;

    match upstream {
        Ok(response) if response.status().is_success() => {
            let _ = sqlx::query(
                "UPDATE providers SET last_health_check_at=now(),failure_count=0 WHERE id=$1",
            )
            .bind(route.get::<Uuid, _>("provider_id"))
            .execute(&state.db)
            .await;
            let _ = sqlx::query(
                "UPDATE provider_credentials SET last_used_at=now(),failure_count=0 WHERE id=$1",
            )
            .bind(credential_id)
            .execute(&state.db)
            .await;
            Ok(Json(serde_json::json!({
                "ok": true,
                "model": route.get::<String, _>("model_name"),
                "provider": route.get::<String, _>("provider_name"),
                "latency_ms": latency_ms,
                "message": "模型检测成功",
            })))
        }
        Ok(response) => {
            let status = response.status().as_u16();
            if matches!(status, 401 | 403) {
                let _ = sqlx::query("UPDATE provider_credentials SET status='INVALID',last_error_at=now(),failure_count=failure_count+1 WHERE id=$1")
                    .bind(credential_id)
                    .execute(&state.db)
                    .await;
            } else if status == 429 {
                let _ = sqlx::query("UPDATE provider_credentials SET status='COOLDOWN',cooldown_until=now()+interval '60 seconds',last_error_at=now(),failure_count=failure_count+1 WHERE id=$1")
                    .bind(credential_id)
                    .execute(&state.db)
                    .await;
            } else {
                let _ = sqlx::query("UPDATE provider_credentials SET last_error_at=now(),failure_count=failure_count+1 WHERE id=$1")
                    .bind(credential_id)
                    .execute(&state.db)
                    .await;
            }
            Ok(Json(serde_json::json!({
                "ok": false,
                "model": route.get::<String, _>("model_name"),
                "provider": route.get::<String, _>("provider_name"),
                "latency_ms": latency_ms,
                "message": format!("模型检测失败：上游返回 HTTP {status}"),
            })))
        }
        Err(error) => Ok(Json(serde_json::json!({
            "ok": false,
            "model": route.get::<String, _>("model_name"),
            "provider": route.get::<String, _>("provider_name"),
            "latency_ms": latency_ms,
            "message": if error.is_timeout() { "模型检测失败：上游请求超时" } else { "模型检测失败：无法连接上游服务" },
        }))),
    }
}
