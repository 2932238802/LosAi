use crate::{
    error::{GatewayError, Result},
    services::auth,
    state::AppState,
};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{Postgres, QueryBuilder, Row};
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub struct LogQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub model: Option<String>,
    pub status_code: Option<i32>,
    pub error_code: Option<String>,
    pub provider_id: Option<Uuid>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub stream: Option<bool>,
}

fn paging(q: &LogQuery, default_size: i64) -> (i64, i64) {
    (
        q.page.unwrap_or(1).max(1),
        q.page_size.unwrap_or(default_size).clamp(1, 100),
    )
}

fn add_log_filters<'a>(
    builder: &mut QueryBuilder<'a, Postgres>,
    q: &'a LogQuery,
    user_id: Option<Uuid>,
) {
    let mut first = true;
    let mut and = |builder: &mut QueryBuilder<'a, Postgres>| {
        if first {
            builder.push(" WHERE ");
            first = false;
        } else {
            builder.push(" AND ");
        }
    };
    if let Some(id) = user_id {
        and(builder);
        builder.push("user_id = ").push_bind(id);
    }
    if let Some(model) = &q.model {
        and(builder);
        builder.push("model = ").push_bind(model);
    }
    if let Some(code) = q.status_code {
        and(builder);
        builder.push("status_code = ").push_bind(code);
    }
    if let Some(code) = &q.error_code {
        and(builder);
        builder.push("error_code = ").push_bind(code);
    }
    if let Some(id) = q.provider_id {
        and(builder);
        builder.push("provider_id = ").push_bind(id);
    }
    if let Some(time) = q.start_time {
        and(builder);
        builder.push("created_at >= ").push_bind(time);
    }
    if let Some(time) = q.end_time {
        and(builder);
        builder.push("created_at <= ").push_bind(time);
    }
    if let Some(stream) = q.stream {
        and(builder);
        builder.push("stream = ").push_bind(stream);
    }
}

fn bearer(h: &HeaderMap) -> &str {
    h.get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

pub async fn admin_logs(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<LogQuery>,
) -> Result<Json<serde_json::Value>> {
    auth::require_admin(&s, bearer(&h))?;
    let (page, size) = paging(&q, 50);
    let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM request_logs");
    add_log_filters(&mut count, &q, None);
    let total: i64 = count
        .build_query_scalar()
        .fetch_one(&s.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let mut query = QueryBuilder::<Postgres>::new(
        "SELECT request_id,user_id,api_key_id,provider_id,model,status_code,latency_ms,error_code,stream,input_tokens,output_tokens,credits,created_at FROM request_logs",
    );
    add_log_filters(&mut query, &q, None);
    query
        .push(" ORDER BY created_at DESC LIMIT ")
        .push_bind(size)
        .push(" OFFSET ")
        .push_bind((page - 1) * size);
    let rows = query
        .build()
        .fetch_all(&s.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    Ok(Json(json_logs(rows, true, page, size, total)))
}

pub async fn user_logs(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<LogQuery>,
) -> Result<Json<serde_json::Value>> {
    let claims = auth::require_user(&s, bearer(&h))?;
    let (page, size) = paging(&q, 20);
    let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM request_logs");
    add_log_filters(&mut count, &q, Some(claims.sub));
    let total: i64 = count
        .build_query_scalar()
        .fetch_one(&s.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let mut query = QueryBuilder::<Postgres>::new(
        "SELECT request_id,user_id,api_key_id,provider_id,model,status_code,latency_ms,error_code,stream,input_tokens,output_tokens,credits,created_at FROM request_logs",
    );
    add_log_filters(&mut query, &q, Some(claims.sub));
    query
        .push(" ORDER BY created_at DESC LIMIT ")
        .push_bind(size)
        .push(" OFFSET ")
        .push_bind((page - 1) * size);
    let rows = query
        .build()
        .fetch_all(&s.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    Ok(Json(json_logs(rows, false, page, size, total)))
}

fn json_logs(
    rows: Vec<sqlx::postgres::PgRow>,
    admin: bool,
    page: i64,
    size: i64,
    total: i64,
) -> serde_json::Value {
    let data = rows.iter().map(|r| serde_json::json!({
        "request_id": r.get::<Uuid,_>("request_id"),
        "user_id": if admin { serde_json::json!(r.get::<Option<Uuid>,_>("user_id")) } else { serde_json::Value::Null },
        "api_key_id": if admin { serde_json::json!(r.get::<Option<Uuid>,_>("api_key_id")) } else { serde_json::Value::Null },
        "provider_id": if admin { serde_json::json!(r.get::<Option<Uuid>,_>("provider_id")) } else { serde_json::Value::Null },
        "model": r.get::<Option<String>,_>("model"),
        "status_code": r.get::<i32,_>("status_code"),
        "latency_ms": r.get::<i64,_>("latency_ms"),
        "error_code": r.get::<Option<String>,_>("error_code"),
        "stream": r.get::<bool,_>("stream"),
        "input_tokens": r.get::<i64,_>("input_tokens"),
        "output_tokens": r.get::<i64,_>("output_tokens"),
        "credits": r.get::<i64,_>("credits"),
        "created_at": r.get::<DateTime<Utc>,_>("created_at")
    })).collect::<Vec<_>>();
    serde_json::json!({"data": data, "page": page, "page_size": size, "total": total, "total_pages": if total == 0 { 0 } else { (total + size - 1) / size }})
}

pub async fn model_stats(
    State(s): State<AppState>,
    h: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    auth::require_admin(&s, bearer(&h))?;
    let rows = sqlx::query("SELECT model,COUNT(*)::bigint requests,COALESCE(SUM(input_tokens+output_tokens),0)::bigint tokens,COALESCE(SUM(credits),0)::bigint credits,COALESCE(AVG(latency_ms),0)::double precision avg_latency_ms,COALESCE(percentile_cont(0.95) WITHIN GROUP (ORDER BY latency_ms),0)::double precision p95_latency_ms FROM request_logs GROUP BY model ORDER BY requests DESC")
        .fetch_all(&s.db).await.map_err(|_| GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data": rows.iter().map(|r| serde_json::json!({"model":r.get::<Option<String>,_>("model"),"requests":r.get::<i64,_>("requests"),"tokens":r.get::<i64,_>("tokens"),"credits":r.get::<i64,_>("credits"),"avg_latency_ms":r.get::<f64,_>("avg_latency_ms"),"p95_latency_ms":r.get::<f64,_>("p95_latency_ms")})).collect::<Vec<_>>() }),
    ))
}
