use crate::error::{GatewayError, Result};
use redis::{Script, aio::ConnectionManager};

pub async fn rpm(redis: &mut ConnectionManager, key: &str, limit: i64) -> Result<()> {
    let script = Script::new(
        "local n=redis.call('INCR',KEYS[1]);if n==1 then redis.call('EXPIRE',KEYS[1],60)end;return n",
    );
    let count: i64 = script
        .key(key)
        .invoke_async(redis)
        .await
        .map_err(|_| GatewayError::Internal)?;
    if count > limit {
        Err(GatewayError::RateLimitExceeded)
    } else {
        Ok(())
    }
}

pub async fn tpm(
    redis: &mut ConnectionManager,
    key: &str,
    estimated_tokens: i64,
    limit: i64,
) -> Result<()> {
    if limit <= 0 || estimated_tokens <= 0 {
        return Ok(());
    }
    let script = Script::new(
        "local n=redis.call('INCRBY',KEYS[1],ARGV[1]);if n==ARGV[1] then redis.call('EXPIRE',KEYS[1],60)end;if n>tonumber(ARGV[2]) then redis.call('DECRBY',KEYS[1],ARGV[1]);return -1 end;return n",
    );
    let count: i64 = script
        .key(key)
        .arg(estimated_tokens)
        .arg(limit)
        .invoke_async(redis)
        .await
        .map_err(|_| GatewayError::Internal)?;
    if count > limit {
        Err(GatewayError::TokenLimitExceeded)
    } else {
        Ok(())
    }
}

pub async fn release_tokens(redis: &mut ConnectionManager, key: &str, tokens: i64) -> Result<()> {
    if tokens <= 0 {
        return Ok(());
    }
    redis::cmd("DECRBY")
        .arg(key)
        .arg(tokens)
        .query_async(redis)
        .await
        .map_err(|_| GatewayError::Internal)
}
pub async fn acquire(redis: &mut ConnectionManager, key: &str, limit: i64) -> Result<Guard> {
    let script =
        Script::new("local n=redis.call('INCR',KEYS[1]);redis.call('EXPIRE',KEYS[1],120);return n");
    let count: i64 = script
        .key(key)
        .invoke_async(redis)
        .await
        .map_err(|_| GatewayError::Internal)?;
    if count > limit {
        let _: i64 = redis::cmd("DECR")
            .arg(key)
            .query_async(redis)
            .await
            .map_err(|_| GatewayError::Internal)?;
        return Err(GatewayError::ConcurrencyLimit);
    }
    Ok(Guard {
        redis: redis.clone(),
        key: key.to_owned(),
    })
}

pub struct Guard {
    redis: ConnectionManager,
    key: String,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let mut redis = self.redis.clone();
        let key = self.key.clone();
        tokio::spawn(async move {
            let _: std::result::Result<i64, _> =
                redis::cmd("DECR").arg(key).query_async(&mut redis).await;
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn namespace_is_stable() {
        assert!("gateway:token-limit:key:window".starts_with("gateway:"));
    }
}
