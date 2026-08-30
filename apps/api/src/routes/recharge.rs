use crate::{
    error::{GatewayError, Result},
    services::auth,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

fn token(headers: &HeaderMap) -> Result<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(GatewayError::Unauthorized)
}
fn customer(state: &AppState, headers: &HeaderMap) -> Result<auth::Claims> {
    auth::require_user(state, &format!("Bearer {}", token(headers)?))
}
fn admin(state: &AppState, headers: &HeaderMap) -> Result<auth::Claims> {
    auth::require_admin(state, &format!("Bearer {}", token(headers)?))
}
fn offer_json(row: &sqlx::postgres::PgRow) -> serde_json::Value {
    let base = row.get::<i64, _>("base_los");
    let bonus = row.get::<i64, _>("bonus_los");
    serde_json::json!({"id":row.get::<Uuid,_>("id"),"name":row.get::<String,_>("name"),"amount_cents":row.get::<i64,_>("amount_cents"),"base_los":base,"bonus_los":bonus,"total_los":base.saturating_add(bonus),"description":row.get::<String,_>("description"),"enabled":row.try_get::<bool,_>("enabled").unwrap_or(true),"sort_order":row.try_get::<i32,_>("sort_order").unwrap_or(100)})
}

pub async fn user_offers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    customer(&state, &headers)?;
    let rows=sqlx::query("SELECT id,name,amount_cents,base_los,bonus_los,description,enabled,sort_order FROM recharge_offers WHERE enabled=true ORDER BY sort_order,name").fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(offer_json).collect::<Vec<_>>(),"exchange_rate":{"cny_yuan":1,"los":50}}),
    ))
}

#[derive(Deserialize)]
pub struct CreateOrderInput {
    pub offer_id: Option<Uuid>,
    pub amount_cents: Option<i64>,
    pub note: Option<String>,
}
pub async fn user_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    let claims = customer(&state, &headers)?;
    let rows=sqlx::query("SELECT id,amount_cents,base_los,bonus_los,total_los,status,note,created_at,reviewed_at FROM recharge_orders WHERE user_id=$1 ORDER BY created_at DESC").bind(claims.sub).fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|serde_json::json!({"id":r.get::<Uuid,_>("id"),"amount_cents":r.get::<i64,_>("amount_cents"),"base_los":r.get::<i64,_>("base_los"),"bonus_los":r.get::<i64,_>("bonus_los"),"total_los":r.get::<i64,_>("total_los"),"status":r.get::<String,_>("status"),"note":r.get::<String,_>("note"),"created_at":r.get::<DateTime<Utc>,_>("created_at"),"reviewed_at":r.get::<Option<DateTime<Utc>>,_>("reviewed_at")})).collect::<Vec<_>>() }),
    ))
}
pub async fn create_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateOrderInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = customer(&state, &headers)?;
    let row=match input.offer_id { Some(id)=>sqlx::query("SELECT id,amount_cents,base_los,bonus_los FROM recharge_offers WHERE id=$1 AND enabled=true").bind(id).fetch_optional(&state.db).await.map_err(|_|GatewayError::Internal)?, None=>None };
    let (offer_id, amount, base, bonus) = match row {
        Some(r) => (
            Some(r.get::<Uuid, _>("id")),
            r.get("amount_cents"),
            r.get("base_los"),
            r.get("bonus_los"),
        ),
        None => {
            let amount = input
                .amount_cents
                .ok_or_else(|| GatewayError::Validation("请选择充值档位或填写金额".to_owned()))?;
            if amount < 1000 || amount % 100 != 0 {
                return Err(GatewayError::Validation(
                    "最低充值10元，金额必须为整元".to_owned(),
                ));
            }
            (
                None,
                amount,
                (amount / 100)
                    .checked_mul(50)
                    .ok_or(GatewayError::Internal)?,
                0,
            )
        }
    };
    let total = base.checked_add(bonus).ok_or(GatewayError::Internal)?;
    let id=sqlx::query_scalar::<_,Uuid>("INSERT INTO recharge_orders(user_id,offer_id,amount_cents,base_los,bonus_los,total_los,note) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING id").bind(claims.sub).bind(offer_id).bind(amount).bind(base).bind(bonus).bind(total).bind(input.note.unwrap_or_default().trim()).fetch_one(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"id":id,"status":"PENDING","total_los":total,"message":"充值申请已提交，请线下付款后等待管理员确认"}),
    ))
}

#[derive(Deserialize)]
pub struct OfferInput {
    pub name: String,
    pub amount_cents: i64,
    pub base_los: i64,
    #[serde(default)]
    pub bonus_los: i64,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub enabled: bool,
}
fn validate_offer(i: &OfferInput) -> Result<()> {
    if i.name.trim().is_empty()
        || i.amount_cents <= 0
        || i.base_los <= 0
        || i.bonus_los < 0
        || i.sort_order < 0
    {
        return Err(GatewayError::Validation("充值档位参数无效".to_owned()));
    }
    Ok(())
}
pub async fn admin_offers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let rows=sqlx::query("SELECT id,name,amount_cents,base_los,bonus_los,description,enabled,sort_order FROM recharge_offers ORDER BY sort_order,name").fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(offer_json).collect::<Vec<_>>() }),
    ))
}
pub async fn create_offer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(i): Json<OfferInput>,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    validate_offer(&i)?;
    let id=sqlx::query_scalar::<_,Uuid>("INSERT INTO recharge_offers(name,amount_cents,base_los,bonus_los,description,enabled,sort_order) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING id").bind(i.name.trim()).bind(i.amount_cents).bind(i.base_los).bind(i.bonus_los).bind(i.description.trim()).bind(i.enabled).bind(i.sort_order).fetch_one(&state.db).await.map_err(|_|GatewayError::Validation("充值档位名称已存在".to_owned()))?;
    Ok(Json(
        serde_json::json!({"id":id,"message":"充值档位已创建"}),
    ))
}
pub async fn update_offer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(i): Json<OfferInput>,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    validate_offer(&i)?;
    let n=sqlx::query("UPDATE recharge_offers SET name=$2,amount_cents=$3,base_los=$4,bonus_los=$5,description=$6,enabled=$7,sort_order=$8,updated_at=now() WHERE id=$1").bind(id).bind(i.name.trim()).bind(i.amount_cents).bind(i.base_los).bind(i.bonus_los).bind(i.description.trim()).bind(i.enabled).bind(i.sort_order).execute(&state.db).await.map_err(|_|GatewayError::Internal)?.rows_affected();
    if n == 0 {
        return Err(GatewayError::Validation("充值档位不存在".to_owned()));
    }
    Ok(Json(serde_json::json!({"message":"充值档位已更新"})))
}
pub async fn delete_offer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    sqlx::query("DELETE FROM recharge_offers WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| GatewayError::Internal)?;
    Ok(Json(serde_json::json!({"message":"充值档位已删除"})))
}

pub async fn admin_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    admin(&state, &headers)?;
    let rows=sqlx::query("SELECT r.id,r.user_id,u.email,r.amount_cents,r.base_los,r.bonus_los,r.total_los,r.status,r.note,r.created_at,r.reviewed_at FROM recharge_orders r JOIN users u ON u.id=r.user_id ORDER BY CASE WHEN r.status='PENDING' THEN 0 ELSE 1 END,r.created_at DESC").fetch_all(&state.db).await.map_err(|_|GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"data":rows.iter().map(|r|serde_json::json!({"id":r.get::<Uuid,_>("id"),"user_id":r.get::<Uuid,_>("user_id"),"email":r.get::<String,_>("email"),"amount_cents":r.get::<i64,_>("amount_cents"),"base_los":r.get::<i64,_>("base_los"),"bonus_los":r.get::<i64,_>("bonus_los"),"total_los":r.get::<i64,_>("total_los"),"status":r.get::<String,_>("status"),"note":r.get::<String,_>("note"),"created_at":r.get::<DateTime<Utc>,_>("created_at"),"reviewed_at":r.get::<Option<DateTime<Utc>>,_>("reviewed_at")})).collect::<Vec<_>>() }),
    ))
}
#[derive(Deserialize)]
pub struct ReviewInput {
    pub status: String,
    pub note: Option<String>,
}
pub async fn review_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(i): Json<ReviewInput>,
) -> Result<Json<serde_json::Value>> {
    let claims = admin(&state, &headers)?;
    if !matches!(i.status.as_str(), "APPROVED" | "REJECTED") {
        return Err(GatewayError::Validation("充值审核状态无效".to_owned()));
    }
    let mut tx = state.db.begin().await.map_err(|_| GatewayError::Internal)?;
    let r =
        sqlx::query("SELECT user_id,total_los,status FROM recharge_orders WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| GatewayError::Internal)?
            .ok_or_else(|| GatewayError::Validation("充值订单不存在".to_owned()))?;
    if r.get::<String, _>("status") != "PENDING" {
        return Err(GatewayError::Validation("该充值订单已处理".to_owned()));
    }
    let uid = r.get::<Uuid, _>("user_id");
    let total = r.get::<i64, _>("total_los");
    sqlx::query("UPDATE recharge_orders SET status=$2,note=$3,reviewed_by=$4,reviewed_at=now(),updated_at=now() WHERE id=$1").bind(id).bind(&i.status).bind(i.note.unwrap_or_default().trim()).bind(claims.sub).execute(&mut *tx).await.map_err(|_|GatewayError::Internal)?;
    if i.status == "APPROVED" {
        let before: i64 =
            sqlx::query_scalar("SELECT credits_balance FROM users WHERE id=$1 FOR UPDATE")
                .bind(uid)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| GatewayError::Internal)?;
        let after = before.checked_add(total).ok_or(GatewayError::Internal)?;
        sqlx::query("UPDATE users SET credits_balance=$2,updated_at=now() WHERE id=$1")
            .bind(uid)
            .bind(after)
            .execute(&mut *tx)
            .await
            .map_err(|_| GatewayError::Internal)?;
        sqlx::query("INSERT INTO credit_ledger(user_id,amount,balance_before,balance_after,entry_type,description,created_by) VALUES($1,$2,$3,$4,'ADMIN_TOP_UP',$5,$6)").bind(uid).bind(total).bind(before).bind(after).bind("充值订单到账").bind(claims.sub).execute(&mut *tx).await.map_err(|_|GatewayError::Internal)?;
    }
    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    Ok(Json(
        serde_json::json!({"id":id,"status":i.status,"message":"充值审核完成"}),
    ))
}
