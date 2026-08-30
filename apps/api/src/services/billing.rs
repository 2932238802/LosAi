use crate::error::{GatewayError, Result};
use sqlx::PgPool;
use uuid::Uuid;

pub fn calculate_credits(
    input_tokens: i64,
    output_tokens: i64,
    input_rate: i64,
    output_rate: i64,
) -> Result<i64> {
    if [input_tokens, output_tokens, input_rate, output_rate]
        .iter()
        .any(|value| *value < 0)
    {
        return Err(GatewayError::Validation(
            "Token 和费率不能为负数".to_owned(),
        ));
    }
    let input = input_tokens
        .checked_mul(input_rate)
        .ok_or(GatewayError::Internal)?;
    let output = output_tokens
        .checked_mul(output_rate)
        .ok_or(GatewayError::Internal)?;
    input.checked_add(output).ok_or(GatewayError::Internal)
}

pub struct Settlement {
    pub user_id: Uuid,
    pub api_key_id: Uuid,
    pub provider_id: Option<Uuid>,
    pub credential_id: Option<Uuid>,
    pub request_id: Uuid,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub credits: i64,
    pub stream: bool,
    pub status: &'static str,
    pub status_code: i32,
    pub latency_ms: i64,
    pub error_code: Option<&'static str>,
    pub usage_source: &'static str,
    pub precharged_credits: i64,
}

pub async fn settle(pool: &PgPool, entry: &Settlement) -> Result<bool> {
    let mut tx = pool.begin().await.map_err(|_| GatewayError::Internal)?;
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM usage_records WHERE request_id=$1 FOR UPDATE")
            .bind(entry.request_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| GatewayError::Internal)?;
    if exists.is_some() {
        return Ok(false);
    }
    let changed = sqlx::query("UPDATE users SET credits_balance=credits_balance-$1,updated_at=now() WHERE id=$2 AND credits_balance >= $1")
        .bind(entry.credits).bind(entry.user_id).execute(&mut *tx).await.map_err(|_| GatewayError::Internal)?.rows_affected();
    if changed == 0 {
        return Err(GatewayError::InsufficientCredits);
    }
    let balance_after: i64 = sqlx::query_scalar("SELECT credits_balance FROM users WHERE id=$1")
        .bind(entry.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| GatewayError::Internal)?;
    let balance_before = balance_after
        .checked_add(entry.credits)
        .ok_or(GatewayError::Internal)?;
    sqlx::query("INSERT INTO usage_records(request_id,user_id,api_key_id,provider_id,credential_id,model,input_tokens,output_tokens,credits,stream,status,usage_source,precharged_credits) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(entry.request_id).bind(entry.user_id).bind(entry.api_key_id).bind(entry.provider_id).bind(entry.credential_id).bind(&entry.model).bind(entry.input_tokens).bind(entry.output_tokens).bind(entry.credits).bind(entry.stream).bind(entry.status).bind(entry.usage_source).bind(entry.precharged_credits)
        .execute(&mut *tx).await.map_err(|_| GatewayError::Internal)?;
    sqlx::query("INSERT INTO credit_ledger(user_id,amount,balance_before,balance_after,entry_type,description,request_id) VALUES($1,$2,$3,$4,'USAGE_DEBIT',$5,$6) ON CONFLICT DO NOTHING")
        .bind(entry.user_id).bind(-entry.credits).bind(balance_before).bind(balance_after)
        .bind(format!("API 使用: {}", entry.model)).bind(entry.request_id)
        .execute(&mut *tx).await.map_err(|_| GatewayError::Internal)?;
    sqlx::query("INSERT INTO request_logs(request_id,user_id,api_key_id,provider_id,model,status_code,latency_ms,error_code,stream,input_tokens,output_tokens,credits) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT (request_id) DO UPDATE SET provider_id=EXCLUDED.provider_id,model=EXCLUDED.model,status_code=EXCLUDED.status_code,latency_ms=EXCLUDED.latency_ms,error_code=EXCLUDED.error_code,stream=EXCLUDED.stream,input_tokens=EXCLUDED.input_tokens,output_tokens=EXCLUDED.output_tokens,credits=EXCLUDED.credits")
        .bind(entry.request_id).bind(entry.user_id).bind(entry.api_key_id).bind(entry.provider_id).bind(&entry.model).bind(entry.status_code).bind(entry.latency_ms).bind(entry.error_code).bind(entry.stream).bind(entry.input_tokens).bind(entry.output_tokens).bind(entry.credits)
        .execute(&mut *tx).await.map_err(|_| GatewayError::Internal)?;
    tx.commit().await.map_err(|_| GatewayError::Internal)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::calculate_credits;
    #[test]
    fn uses_integer_pricing() {
        assert_eq!(calculate_credits(10, 5, 2, 3).unwrap(), 35);
    }
    #[test]
    fn rejects_negative_values() {
        assert!(calculate_credits(-1, 0, 1, 1).is_err());
    }
}
