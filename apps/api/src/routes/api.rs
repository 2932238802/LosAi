use crate::{
    error::{GatewayError, Result},
    services::{auth, billing, crypto, limits},
    state::AppState,
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::header::AUTHORIZATION as REQ_AUTHORIZATION;
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};
use std::time::Instant;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct Login {
    pub email: String,
    pub password: String,
}
#[derive(Deserialize)]
pub struct Register {
    pub email: String,
    pub password: String,
    pub confirm_password: String,
}
#[derive(Serialize)]
pub struct Token {
    pub access_token: String,
    pub token_type: &'static str,
    pub role: String,
}
#[derive(Deserialize)]
pub struct UserIn {
    pub email: String,
    pub password: String,
    pub role: Option<String>,
    pub plan_id: Option<Uuid>,
    pub credits_balance: Option<i64>,
}
#[derive(Deserialize)]
pub struct PlanIn {
    pub name: String,
    pub monthly_credits: i64,
    pub rpm_limit: i32,
    pub tpm_limit: Option<i32>,
    pub max_concurrency: i32,
    pub monthly_request_limit: i64,
}
#[derive(Deserialize)]
pub struct KeyIn {
    pub user_id: Option<Uuid>,
    pub name: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}
#[derive(Deserialize, Serialize, Clone)]
pub struct Chat {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub stream: Option<bool>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
#[derive(Deserialize)]
pub struct PageQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreditChange {
    pub amount: i64,
    pub description: Option<String>,
}

fn bearer(headers: &HeaderMap) -> Result<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(GatewayError::Unauthorized)
}
fn admin(state: &AppState, headers: &HeaderMap) -> Result<auth::Claims> {
    auth::require_admin(state, &format!("Bearer {}", bearer(headers)?))
}
fn user(state: &AppState, headers: &HeaderMap) -> Result<auth::Claims> {
    let claims = auth::require_user(state, &format!("Bearer {}", bearer(headers)?))?;
    Ok(claims)
}
fn row_json(row: &PgRow, fields: &[(&str, &str)]) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for (name, kind) in fields {
        let value = match *kind {
            "uuid" => serde_json::json!(row.try_get::<Uuid, _>(*name).ok()),
            "bool" => serde_json::json!(row.try_get::<bool, _>(*name).ok()),
            "i64" => serde_json::json!(row.try_get::<i64, _>(*name).ok()),
            "i32" => serde_json::json!(row.try_get::<i32, _>(*name).ok()),
            _ => serde_json::json!(row.try_get::<String, _>(*name).ok()),
        };
        object.insert((*name).to_owned(), value);
    }
    serde_json::Value::Object(object)
}
pub(crate) async fn audit(state: &AppState, actor: Uuid, action: &str, resource: &str, id: Uuid) {
    let _ = sqlx::query("INSERT INTO audit_logs(actor_user_id,action,resource_type,resource_id) VALUES($1,$2,$3,$4)").bind(actor).bind(action).bind(resource).bind(id).execute(&state.db).await;
}

pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();
    Json(serde_json::json!({"status": if ok {"ok"} else {"degraded"}, "service":"lostoken-api"}))
}
pub async fn login(State(state): State<AppState>, Json(input): Json<Login>) -> Result<Json<Token>> {
    let (access_token, role) = auth::login(&state, &input.email, &input.password).await?;
    Ok(Json(Token {
        access_token,
        token_type: "Bearer",
        role,
    }))
}

pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<Register>,
) -> Result<Json<Token>> {
    let email = input.email.trim().to_lowercase();
    if !email.contains('@') || email.len() > 254 || input.password.len() < 8 {
        return Err(GatewayError::Validation(
            "请输入有效邮箱，密码至少需要 8 位".to_owned(),
        ));
    }
    if input.password != input.confirm_password {
        return Err(GatewayError::Validation("两次输入的密码不一致".to_owned()));
    }
    let password_hash = auth::hash_password(&input.password).map_err(|_| GatewayError::Internal)?;
    let user_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users(email,password_hash,role) VALUES($1,$2,'CUSTOMER') RETURNING id",
    )
    .bind(&email)
    .bind(password_hash)
    .fetch_one(&state.db)
    .await
    .map_err(|_| GatewayError::Validation("该邮箱已经注册".to_owned()))?;
    let access_token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &auth::Claims {
            sub: user_id,
            role: "CUSTOMER".to_owned(),
            exp: (chrono::Utc::now().timestamp() + 86400) as usize,
        },
        &jsonwebtoken::EncodingKey::from_secret(state.config.session_secret.as_bytes()),
    )
    .map_err(|_| GatewayError::Internal)?;
    Ok(Json(Token {
        access_token,
        token_type: "Bearer",
        role: "CUSTOMER".to_owned(),
    }))
}

pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let rows = sqlx::query(
        "SELECT id,email,role,enabled,plan_id,credits_balance,created_at,updated_at FROM users ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|serde_json::json!({"id":r.get::<Uuid,_>("id"),"email":r.get::<String,_>("email"),"role":r.get::<String,_>("role"),"enabled":r.get::<bool,_>("enabled"),"plan_id":r.get::<Option<Uuid>,_>("plan_id"),"credits_balance":r.get::<i64,_>("credits_balance"),"created_at":r.get::<DateTime<Utc>,_>("created_at"),"updated_at":r.get::<DateTime<Utc>,_>("updated_at")})).collect::<Vec<_>>() }),
    ))
}
pub async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UserIn>,
) -> Result<Json<serde_json::Value>> {
    let claims = admin(&state, &headers)?;
    if input.email.trim().is_empty() || input.password.len() < 8 {
        return Err(GatewayError::Validation(
            "invalid email or password".to_owned(),
        ));
    }
    let role = input.role.unwrap_or_else(|| "CUSTOMER".to_owned());
    if !matches!(role.as_str(), "ADMIN" | "CUSTOMER") {
        return Err(GatewayError::Validation("用户角色无效".to_owned()));
    }
    let id=sqlx::query_scalar::<_,Uuid>("INSERT INTO users(email,password_hash,role,plan_id,credits_balance) VALUES($1,$2,$3,$4,$5) RETURNING id").bind(input.email.trim().to_lowercase()).bind(auth::hash_password(&input.password).map_err(|_|GatewayError::Internal)?).bind(role).bind(input.plan_id).bind(input.credits_balance.unwrap_or(0)).fetch_one(&state.db).await.map_err(|_|GatewayError::Validation("用户邮箱已存在或参数无效".to_owned()))?;
    audit(&state, claims.sub, "CREATE", "USER", id).await;
    Ok(Json(serde_json::json!({"id":id,"message":"用户创建成功"})))
}
pub async fn list_plans(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let rows=sqlx::query("SELECT id,name,monthly_credits,rpm_limit,tpm_limit,max_concurrency,monthly_request_limit,enabled FROM plans ORDER BY created_at DESC").fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|row_json(r,&[("id","uuid"),("name","text"),("monthly_credits","i64"),("rpm_limit","i32"),("tpm_limit","i32"),("max_concurrency","i32"),("monthly_request_limit","i64"),("enabled","bool")])).collect::<Vec<_>>() }),
    ))
}
pub async fn create_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PlanIn>,
) -> Result<Json<serde_json::Value>> {
    let claims = admin(&state, &headers)?;
    if input.name.trim().is_empty()
        || input.monthly_credits < 0
        || input.rpm_limit <= 0
        || input.max_concurrency <= 0
    {
        return Err(GatewayError::Validation("套餐参数无效".to_owned()));
    }
    let id=sqlx::query_scalar::<_,Uuid>("INSERT INTO plans(name,monthly_credits,rpm_limit,tpm_limit,max_concurrency,monthly_request_limit) VALUES($1,$2,$3,$4,$5,$6) RETURNING id").bind(input.name.trim()).bind(input.monthly_credits).bind(input.rpm_limit).bind(input.tpm_limit.unwrap_or(0)).bind(input.max_concurrency).bind(input.monthly_request_limit).fetch_one(&state.db).await.map_err(|_|GatewayError::Validation("套餐名称已存在或参数无效".to_owned()))?;
    audit(&state, claims.sub, "CREATE", "PLAN", id).await;
    Ok(Json(serde_json::json!({"id":id,"message":"套餐创建成功"})))
}
pub async fn list_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let rows=sqlx::query("SELECT id,user_id,name,key_prefix,enabled,expires_at,created_at,last_used_at FROM api_keys ORDER BY created_at DESC").fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|serde_json::json!({"id":r.get::<Uuid,_>("id"),"user_id":r.get::<Uuid,_>("user_id"),"name":r.get::<String,_>("name"),"key_prefix":r.get::<String,_>("key_prefix"),"enabled":r.get::<bool,_>("enabled"),"expires_at":r.get::<Option<DateTime<Utc>>,_>("expires_at"),"created_at":r.get::<DateTime<Utc>,_>("created_at"),"last_used_at":r.get::<Option<DateTime<Utc>>,_>("last_used_at")})).collect::<Vec<_>>() }),
    ))
}
pub async fn create_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<KeyIn>,
) -> Result<Json<serde_json::Value>> {
    let claims = admin(&state, &headers)?;
    let owner = input
        .user_id
        .ok_or(GatewayError::Validation("必须指定用户".to_owned()))?;
    create_key_for(&state, owner, input.name, input.expires_at, claims.sub).await
}

pub async fn models(State(state): State<AppState>) -> Result<Json<serde_json::Value>> {
    let rows = sqlx::query("SELECT model_name FROM models WHERE enabled ORDER BY model_name")
        .fetch_all(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"object":"list","data":rows.iter().map(|r|serde_json::json!({"id":r.get::<String,_>("model_name"),"object":"model","owned_by":"lostoken"})).collect::<Vec<_>>()}),
    ))
}

async fn create_key_for(
    state: &AppState,
    owner: Uuid,
    name: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    actor: Uuid,
) -> Result<Json<serde_json::Value>> {
    let secret = crypto::generate_virtual_key();
    let prefix = secret.chars().take(12).collect::<String>();
    let hash = crypto::hash_key(&secret, &state.config.key_hash_pepper);
    let id=sqlx::query_scalar::<_,Uuid>("INSERT INTO api_keys(user_id,name,key_prefix,key_hash,expires_at) VALUES($1,$2,$3,$4,$5) RETURNING id").bind(owner).bind(name.unwrap_or_else(||"默认密钥".to_owned())).bind(&prefix).bind(hash).bind(expires_at).fetch_one(&state.db).await.map_err(|_|GatewayError::Internal)?;
    audit(state, actor, "CREATE", "API_KEY", id).await;
    Ok(Json(
        serde_json::json!({"id":id,"secret":secret,"message":"API key created; secret shown once"}),
    ))
}

pub async fn user_dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    let claims = user(&state, &headers)?;
    let r=sqlx::query("SELECT u.email,u.credits_balance,COALESCE(p.name,'未订阅') plan,COALESCE(p.rpm_limit,0) rpm,COALESCE(p.tpm_limit,0) tpm,COALESCE(p.max_concurrency,0) concurrency,(SELECT COUNT(*) FROM api_keys WHERE user_id=u.id) key_count,COALESCE((SELECT COUNT(*) FROM usage_records WHERE user_id=u.id),0)::bigint total_requests,COALESCE((SELECT SUM(input_tokens) FROM usage_records WHERE user_id=u.id),0)::bigint input_tokens,COALESCE((SELECT SUM(output_tokens) FROM usage_records WHERE user_id=u.id),0)::bigint output_tokens,COALESCE((SELECT SUM(credits) FROM usage_records WHERE user_id=u.id),0)::bigint total_spent FROM users u LEFT JOIN plans p ON p.id=u.plan_id WHERE u.id=$1").bind(claims.sub).fetch_one(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"email":r.get::<String,_>("email"),"balance":r.get::<i64,_>("credits_balance"),"plan":r.get::<String,_>("plan"),"rpm":r.get::<i32,_>("rpm"),"tpm":r.get::<i32,_>("tpm"),"concurrency":r.get::<i32,_>("concurrency"),"keyCount":r.get::<i64,_>("key_count"),"totalRequests":r.get::<i64,_>("total_requests"),"todayRequests":0,"inputTokens":r.get::<i64,_>("input_tokens"),"outputTokens":r.get::<i64,_>("output_tokens"),"totalSpent":r.get::<i64,_>("total_spent")}),
    ))
}
pub async fn user_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    let claims = user(&state, &headers)?;
    let r = sqlx::query("SELECT email,role,enabled,created_at FROM users WHERE id=$1")
        .bind(claims.sub)
        .fetch_one(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"email":r.get::<String,_>("email"),"role":r.get::<String,_>("role"),"enabled":r.get::<bool,_>("enabled"),"created_at":r.get::<DateTime<Utc>,_>("created_at")}),
    ))
}
pub async fn user_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    let claims = user(&state, &headers)?;
    let r=sqlx::query("SELECT COALESCE(p.name,'未订阅') name,u.credits_balance,COALESCE(p.rpm_limit,0) rpm_limit,COALESCE(p.tpm_limit,0) tpm_limit,COALESCE(p.max_concurrency,0) max_concurrency FROM users u LEFT JOIN plans p ON p.id=u.plan_id WHERE u.id=$1").bind(claims.sub).fetch_one(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"name":r.get::<String,_>("name"),"balance":r.get::<i64,_>("credits_balance"),"rpm_limit":r.get::<i32,_>("rpm_limit"),"tpm_limit":r.get::<i32,_>("tpm_limit"),"max_concurrency":r.get::<i32,_>("max_concurrency")}),
    ))
}
pub async fn user_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    let claims = user(&state, &headers)?;
    let rows=sqlx::query("SELECT id,name,key_prefix,enabled,expires_at,created_at,last_used_at FROM api_keys WHERE user_id=$1 ORDER BY created_at DESC").bind(claims.sub).fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|serde_json::json!({"id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"key_prefix":r.get::<String,_>("key_prefix"),"enabled":r.get::<bool,_>("enabled"),"expires_at":r.get::<Option<DateTime<Utc>>,_>("expires_at"),"created_at":r.get::<DateTime<Utc>,_>("created_at"),"last_used_at":r.get::<Option<DateTime<Utc>>,_>("last_used_at")})).collect::<Vec<_>>() }),
    ))
}
pub async fn user_create_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<KeyIn>,
) -> Result<Json<serde_json::Value>> {
    let claims = user(&state, &headers)?;
    create_key_for(&state, claims.sub, input.name, input.expires_at, claims.sub).await
}
pub async fn disable_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<StatusCode> {
    let claims = user(&state, &headers)?;
    let n = sqlx::query("UPDATE api_keys SET enabled=false WHERE id=$1 AND user_id=$2")
        .bind(id)
        .bind(claims.sub)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if n == 0 {
        return Err(GatewayError::Validation("key not found".to_owned()));
    }
    Ok(StatusCode::NO_CONTENT)
}
pub async fn delete_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<StatusCode> {
    let claims = user(&state, &headers)?;
    let n = sqlx::query("DELETE FROM api_keys WHERE id=$1 AND user_id=$2")
        .bind(id)
        .bind(claims.sub)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?
        .rows_affected();
    if n == 0 {
        return Err(GatewayError::Validation("key not found".to_owned()));
    }
    Ok(StatusCode::NO_CONTENT)
}
pub async fn user_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<serde_json::Value>> {
    let claims = user(&state, &headers)?;
    let size = q.page_size.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * size;
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM usage_records WHERE user_id=$1")
        .bind(claims.sub)
        .fetch_one(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let rows = sqlx::query("SELECT request_id,model,input_tokens,output_tokens,credits,stream,status,created_at FROM usage_records WHERE user_id=$1 ORDER BY created_at DESC LIMIT $2 OFFSET $3")
        .bind(claims.sub)
        .bind(size)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let total_pages = if total == 0 {
        0
    } else {
        (total + size - 1) / size
    };
    Ok(Json(serde_json::json!({
        "data": rows.iter().map(|r| serde_json::json!({
            "request_id": r.get::<Uuid, _>("request_id"),
            "model": r.get::<String, _>("model"),
            "input_tokens": r.get::<i64, _>("input_tokens"),
            "output_tokens": r.get::<i64, _>("output_tokens"),
            "total_tokens": r.get::<i64, _>("input_tokens") + r.get::<i64, _>("output_tokens"),
            "credits": r.get::<i64, _>("credits"),
            "stream": r.get::<bool, _>("stream"),
            "status": r.get::<String, _>("status"),
            "created_at": r.get::<DateTime<Utc>, _>("created_at"),
        })).collect::<Vec<_>>(),
        "page": page,
        "page_size": size,
        "total": total,
        "total_pages": total_pages,
    })))
}
pub async fn user_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<serde_json::Value>> {
    let claims = user(&state, &headers)?;
    let size = q.page_size.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * size;
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM request_logs WHERE user_id=$1")
        .bind(claims.sub)
        .fetch_one(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let rows = sqlx::query("SELECT request_id,model,status_code,latency_ms,error_code,stream,input_tokens,output_tokens,credits,created_at FROM request_logs WHERE user_id=$1 ORDER BY created_at DESC LIMIT $2 OFFSET $3")
        .bind(claims.sub)
        .bind(size)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let total_pages = if total == 0 {
        0
    } else {
        (total + size - 1) / size
    };
    Ok(Json(serde_json::json!({
        "data": rows.iter().map(|r| serde_json::json!({
            "request_id": r.get::<Uuid, _>("request_id"),
            "model": r.get::<Option<String>, _>("model"),
            "status_code": r.get::<i32, _>("status_code"),
            "latency_ms": r.get::<i64, _>("latency_ms"),
            "error_code": r.get::<Option<String>, _>("error_code"),
            "stream": r.get::<bool, _>("stream"),
            "input_tokens": r.get::<i64, _>("input_tokens"),
            "output_tokens": r.get::<i64, _>("output_tokens"),
            "credits": r.get::<i64, _>("credits"),
            "created_at": r.get::<DateTime<Utc>, _>("created_at"),
        })).collect::<Vec<_>>(),
        "page": page,
        "page_size": size,
        "total": total,
        "total_pages": total_pages,
    })))
}

pub async fn admin_credit_change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Json(input): Json<CreditChange>,
) -> Result<Json<serde_json::Value>> {
    let claims = admin(&state, &headers)?;
    if input.amount == 0 || input.description.as_deref().unwrap_or("").len() > 500 {
        return Err(GatewayError::Validation(
            "充值金额不能为 0，备注不能超过 500 个字符".to_owned(),
        ));
    }
    let mut tx = state.db.begin().await.map_err(|_| GatewayError::Internal)?;
    let before: i64 =
        sqlx::query_scalar("SELECT credits_balance FROM users WHERE id=$1 FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| GatewayError::Internal)?
            .ok_or(GatewayError::Validation("用户不存在".to_owned()))?;
    let after = before
        .checked_add(input.amount)
        .ok_or(GatewayError::Validation("余额超出范围".to_owned()))?;
    if after < 0 {
        return Err(GatewayError::Validation("余额不能小于 0".to_owned()));
    }
    sqlx::query("UPDATE users SET credits_balance=$1,updated_at=now() WHERE id=$2")
        .bind(after)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let kind = if input.amount > 0 {
        "ADMIN_TOP_UP"
    } else {
        "ADMIN_ADJUSTMENT"
    };
    sqlx::query("INSERT INTO credit_ledger(user_id,amount,balance_before,balance_after,entry_type,description,created_by) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(user_id)
        .bind(input.amount)
        .bind(before)
        .bind(after)
        .bind(kind)
        .bind(input.description.unwrap_or_default())
        .bind(claims.sub)
        .execute(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?;
    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    audit(&state, claims.sub, kind, "CREDITS", user_id).await;
    Ok(Json(
        serde_json::json!({"user_id": user_id, "balance": after, "message": "Credits 已更新"}),
    ))
}

pub async fn user_credits_ledger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<serde_json::Value>> {
    let claims = user(&state, &headers)?;
    let size = q.page_size.unwrap_or(20).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM credit_ledger WHERE user_id=$1")
        .bind(claims.sub)
        .fetch_one(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let rows = sqlx::query("SELECT id,amount,balance_before,balance_after,entry_type,description,request_id,created_at FROM credit_ledger WHERE user_id=$1 ORDER BY created_at DESC LIMIT $2 OFFSET $3")
        .bind(claims.sub)
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let data = rows.iter().map(|r| serde_json::json!({
        "id": r.get::<Uuid, _>("id"), "amount": r.get::<i64, _>("amount"),
        "balance_before": r.get::<i64, _>("balance_before"), "balance_after": r.get::<i64, _>("balance_after"),
        "entry_type": r.get::<String, _>("entry_type"), "description": r.get::<String, _>("description"),
        "request_id": r.get::<Option<Uuid>, _>("request_id"), "created_at": r.get::<DateTime<Utc>, _>("created_at")
    })).collect::<Vec<_>>();
    Ok(Json(
        serde_json::json!({"data": data, "page": page, "page_size": size, "total": total,
        "total_pages": if total == 0 { 0 } else { (total + size - 1) / size }}),
    ))
}

pub async fn admin_credits_ledger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let size = q.page_size.unwrap_or(50).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let rows = sqlx::query("SELECT id,amount,balance_before,balance_after,entry_type,description,request_id,created_by,created_at FROM credit_ledger WHERE user_id=$1 ORDER BY created_at DESC LIMIT $2 OFFSET $3")
        .bind(user_id)
        .bind(size)
        .bind((page - 1) * size)
        .fetch_all(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data": rows.iter().map(|r| serde_json::json!({
        "id": r.get::<Uuid, _>("id"), "amount": r.get::<i64, _>("amount"),
        "balance_before": r.get::<i64, _>("balance_before"), "balance_after": r.get::<i64, _>("balance_after"),
        "entry_type": r.get::<String, _>("entry_type"), "description": r.get::<String, _>("description"),
        "request_id": r.get::<Option<Uuid>, _>("request_id"), "created_by": r.get::<Option<Uuid>, _>("created_by"),
        "created_at": r.get::<DateTime<Utc>, _>("created_at")
    })).collect::<Vec<_>>() }),
    ))
}

pub async fn admin_dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let r=sqlx::query("SELECT COUNT(*)::bigint requests,COALESCE(SUM(input_tokens+output_tokens),0)::bigint tokens,COALESCE(SUM(credits),0)::bigint credits,COUNT(DISTINCT user_id)::bigint users FROM usage_records").fetch_one(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"requests": r.get::<i64,_>("requests"), "tokens": r.get::<i64,_>("tokens"), "credits": r.get::<i64,_>("credits"), "users": r.get::<i64,_>("users")}),
    ))
}
pub async fn admin_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let size = q.page_size.unwrap_or(50).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let rows=sqlx::query("SELECT request_id,user_id,model,input_tokens,output_tokens,credits,status,created_at FROM usage_records ORDER BY created_at DESC LIMIT $1 OFFSET $2").bind(size).bind((page-1)*size).fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|serde_json::json!({"request_id":r.get::<Uuid,_>("request_id"),"user_id":r.get::<Uuid,_>("user_id"),"model":r.get::<String,_>("model"),"input_tokens":r.get::<i64,_>("input_tokens"),"output_tokens":r.get::<i64,_>("output_tokens"),"credits":r.get::<i64,_>("credits"),"status":r.get::<String,_>("status"),"created_at":r.get::<DateTime<Utc>,_>("created_at")})).collect::<Vec<_>>()}),
    ))
}
pub async fn admin_request_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let size = q.page_size.unwrap_or(50).clamp(1, 100);
    let page = q.page.unwrap_or(1).max(1);
    let rows=sqlx::query("SELECT request_id,user_id,model,status_code,latency_ms,error_code,created_at FROM request_logs ORDER BY created_at DESC LIMIT $1 OFFSET $2").bind(size).bind((page-1)*size).fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|serde_json::json!({"request_id":r.get::<Uuid,_>("request_id"),"user_id":r.get::<Option<Uuid>,_>("user_id"),"model":r.get::<Option<String>,_>("model"),"status_code":r.get::<i32,_>("status_code"),"latency_ms":r.get::<i64,_>("latency_ms"),"error_code":r.get::<Option<String>,_>("error_code"),"created_at":r.get::<DateTime<Utc>,_>("created_at")})).collect::<Vec<_>>() }),
    ))
}

pub async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(request_id): Extension<Uuid>,
    Json(mut input): Json<Chat>,
) -> Result<Response> {
    let started = Instant::now();
    let raw = bearer(&headers)?;
    let hash = crypto::hash_key(raw, &state.config.key_hash_pepper);
    let key=sqlx::query("SELECT k.id,k.user_id,k.enabled,k.expires_at,u.enabled user_enabled,u.credits_balance,p.enabled plan_enabled,p.monthly_request_limit,COALESCE(p.rpm_limit,60) rpm,COALESCE(p.tpm_limit,0) tpm,COALESCE(p.max_concurrency,2) concurrency FROM api_keys k JOIN users u ON u.id=k.user_id LEFT JOIN plans p ON p.id=u.plan_id WHERE k.key_hash=$1").bind(hash).fetch_optional(&state.db).await.map_err(|_|GatewayError::Internal)?.ok_or(GatewayError::InvalidApiKey)?;
    if !key.get::<bool, _>("enabled") {
        return Err(GatewayError::KeyDisabled);
    }
    if !key.get::<bool, _>("user_enabled") {
        return Err(GatewayError::UserDisabled);
    }
    if !key.get::<Option<bool>, _>("plan_enabled").unwrap_or(false) {
        return Err(GatewayError::Validation(
            "当前用户未配置有效套餐".to_owned(),
        ));
    }
    if key.get::<i64, _>("credits_balance") <= 0 {
        return Err(GatewayError::InsufficientCredits);
    }
    let monthly_limit = key
        .get::<Option<i64>, _>("monthly_request_limit")
        .unwrap_or(0);
    if monthly_limit > 0 {
        let used: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_logs WHERE user_id=$1 AND created_at >= date_trunc('month', now())")
            .bind(key.get::<Uuid, _>("user_id"))
            .fetch_one(&state.db)
            .await
            .map_err(|_| GatewayError::Internal)?;
        if used >= monthly_limit {
            return Err(GatewayError::RateLimitExceeded);
        }
    }
    if key
        .get::<Option<DateTime<Utc>>, _>("expires_at")
        .is_some_and(|v| v <= Utc::now())
    {
        return Err(GatewayError::KeyExpired);
    }
    let key_id = key.get::<Uuid, _>("id");
    let user_id = key.get::<Uuid, _>("user_id");
    let mut redis = state.redis.clone();
    let rpm_limit = key
        .try_get::<i32, _>("rpm")
        .map_err(|_| GatewayError::Internal)? as i64;
    let tpm_limit = key
        .try_get::<i32, _>("tpm")
        .map_err(|_| GatewayError::Internal)? as i64;
    let concurrency_limit = key
        .try_get::<i32, _>("concurrency")
        .map_err(|_| GatewayError::Internal)? as i64;
    limits::rpm(
        &mut redis,
        &format!("gateway:ratelimit:{key_id}"),
        rpm_limit,
    )
    .await?;
    let estimate = serde_json::to_vec(&input.messages)
        .map(|v| (v.len() as i64 / 4).max(1))
        .unwrap_or(1);
    let requested_output_tokens = input
        .extra
        .get("max_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(1024)
        .clamp(1, 16384);
    limits::tpm(
        &mut redis,
        &format!("gateway:token-limit:{key_id}"),
        estimate,
        tpm_limit,
    )
    .await?;
    let _guard = limits::acquire(
        &mut redis,
        &format!("gateway:concurrency:{key_id}"),
        concurrency_limit,
    )
    .await?;
    let client_model = input.model.clone();
    let route=sqlx::query("SELECT r.upstream_model,p.id provider_id,p.base_url,c.id credential_id,c.encrypted_secret,m.input_rate_micros,m.output_rate_micros FROM models m JOIN model_routes r ON r.model_id=m.id AND r.enabled JOIN providers p ON p.id=r.provider_id AND p.enabled JOIN provider_credentials c ON c.provider_id=p.id AND c.status='ACTIVE' AND (c.cooldown_until IS NULL OR c.cooldown_until<now()) WHERE m.model_name=$1 AND m.enabled ORDER BY r.priority,c.priority LIMIT 1").bind(&client_model).fetch_optional(&state.db).await.map_err(|_|GatewayError::Internal)?.ok_or(GatewayError::NoHealthyProvider)?;
    let provider_id = route.get::<Uuid, _>("provider_id");
    let credential_id = route.get::<Uuid, _>("credential_id");
    let secret = crypto::decrypt(
        route.get("encrypted_secret"),
        &state.config.credential_encryption_key,
    )
    .map_err(|_| GatewayError::Internal)?;
    input.model = route.get("upstream_model");
    let streaming = input.stream.unwrap_or(false);
    let upstream_base_url = route
        .get::<String, _>("base_url")
        .trim()
        .trim_end_matches('/')
        .to_owned();
    let upstream_base_url = if upstream_base_url.ends_with("/v1") {
        upstream_base_url
    } else {
        format!("{upstream_base_url}/v1")
    };
    let response = state
        .http
        .post(format!("{upstream_base_url}/chat/completions"))
        .header(REQ_AUTHORIZATION, format!("Bearer {secret}"))
        .json(&input)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                GatewayError::UpstreamTimeout
            } else {
                GatewayError::UpstreamError
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        if matches!(status.as_u16(), 401 | 403) {
            let _ = sqlx::query(
                "UPDATE provider_credentials SET status='INVALID',last_error_at=now() WHERE id=$1",
            )
            .bind(credential_id)
            .execute(&state.db)
            .await;
        }
        if status.as_u16() == 429 {
            let _=sqlx::query("UPDATE provider_credentials SET status='COOLDOWN',cooldown_until=now()+interval '60 seconds',last_error_at=now() WHERE id=$1").bind(credential_id).execute(&state.db).await;
        }
        return Err(GatewayError::UpstreamError);
    }
    if streaming {
        let mut upstream = response.bytes_stream();
        let db = state.db.clone();
        let model = client_model.clone();
        let input_rate = route.get::<i64, _>("input_rate_micros");
        let output_rate = route.get::<i64, _>("output_rate_micros");
        let estimated_input_tokens = estimate;
        let estimated_output_tokens = requested_output_tokens;
        let body = async_stream::stream! {
            let mut input_tokens = 0i64;
            let mut output_tokens = 0i64;
            while let Some(item) = upstream.next().await {
                match item {
                    Ok(bytes) => {
                        if let Ok(text) = std::str::from_utf8(&bytes) {
                            for line in text.lines() {
                                if let Some(data) = line.strip_prefix("data: ")
                                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(data)
                                    && let Some(usage) = value.get("usage")
                                {
                                    input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(input_tokens);
                                    output_tokens = usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(output_tokens);
                                }
                            }
                        }
                        yield Ok::<_, std::io::Error>(bytes);
                    }
                    Err(_) => break,
                }
            }
            let reported_credits = billing::calculate_credits(input_tokens, output_tokens, input_rate, output_rate).unwrap_or(0);
            let has_usage = input_tokens > 0 || output_tokens > 0;
            let (final_input, final_output, credits, usage_source) = if has_usage {
                (input_tokens, output_tokens, reported_credits, "PROVIDER_REPORTED")
            } else {
                let estimated = billing::calculate_credits(estimated_input_tokens, estimated_output_tokens, input_rate, output_rate).unwrap_or(0);
                (estimated_input_tokens, estimated_output_tokens, estimated, "ESTIMATED")
            };
            let entry = billing::Settlement { user_id, api_key_id: key_id, provider_id: Some(provider_id), credential_id: Some(credential_id), request_id, model, input_tokens: final_input, output_tokens: final_output, credits, stream: true, status: "COMPLETED", status_code: 200, latency_ms: started.elapsed().as_millis() as i64, error_code: None, usage_source, precharged_credits: credits };
            let _ = billing::settle(&db, &entry).await;
        };
        return Ok((
            StatusCode::OK,
            [
                (CONTENT_TYPE, "text/event-stream"),
                (CACHE_CONTROL, "no-cache"),
            ],
            axum::body::Body::from_stream(body),
        )
            .into_response());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| GatewayError::UpstreamError)?;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| GatewayError::UpstreamError)?;
    let usage = json.get("usage").cloned().unwrap_or_default();
    let reported_input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let reported_output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let has_usage = reported_input_tokens > 0 || reported_output_tokens > 0;
    let (input_tokens, output_tokens, usage_source) = if has_usage {
        (
            reported_input_tokens,
            reported_output_tokens,
            "PROVIDER_REPORTED",
        )
    } else {
        (estimate, requested_output_tokens, "ESTIMATED")
    };
    let credits = billing::calculate_credits(
        input_tokens,
        output_tokens,
        route.get("input_rate_micros"),
        route.get("output_rate_micros"),
    )?;
    let entry = billing::Settlement {
        user_id,
        api_key_id: key_id,
        provider_id: Some(provider_id),
        credential_id: Some(credential_id),
        request_id,
        model: client_model,
        input_tokens,
        output_tokens,
        credits,
        stream: false,
        status: "COMPLETED",
        status_code: 200,
        latency_ms: started.elapsed().as_millis() as i64,
        error_code: None,
        usage_source,
        precharged_credits: credits,
    };
    billing::settle(&state.db, &entry).await?;
    Ok((StatusCode::OK, Json(json)).into_response())
}
