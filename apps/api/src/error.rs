use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("invalid API key")]
    InvalidApiKey,
    #[error("API key is disabled")]
    KeyDisabled,
    #[error("API key has expired")]
    KeyExpired,
    #[error("user is disabled")]
    UserDisabled,
    #[error("rate limit exceeded")]
    RateLimitExceeded,
    #[error("token limit exceeded")]
    TokenLimitExceeded,
    #[error("concurrency limit reached")]
    ConcurrencyLimit,
    #[error("insufficient credits")]
    InsufficientCredits,
    #[error("model is not available")]
    ModelNotAvailable,
    #[error("no healthy provider is available")]
    NoHealthyProvider,
    #[error("upstream request timed out")]
    UpstreamTimeout,
    #[error("upstream request failed")]
    UpstreamError,
    #[error("authentication required")]
    Unauthorized,
    #[error("administrative permission required")]
    Forbidden,
    #[error("invalid request: {0}")]
    Validation(String),
    #[error("internal gateway error")]
    Internal,
}

impl GatewayError {
    fn map(&self) -> (StatusCode, &'static str) {
        match self {
            Self::InvalidApiKey => (StatusCode::UNAUTHORIZED, "INVALID_API_KEY"),
            Self::KeyDisabled => (StatusCode::FORBIDDEN, "KEY_DISABLED"),
            Self::KeyExpired => (StatusCode::FORBIDDEN, "KEY_EXPIRED"),
            Self::UserDisabled => (StatusCode::FORBIDDEN, "USER_DISABLED"),
            Self::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMIT_EXCEEDED"),
            Self::TokenLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, "TPM_LIMIT_EXCEEDED"),
            Self::ConcurrencyLimit => (StatusCode::TOO_MANY_REQUESTS, "CONCURRENCY_LIMIT"),
            Self::InsufficientCredits => (StatusCode::PAYMENT_REQUIRED, "INSUFFICIENT_CREDITS"),
            Self::ModelNotAvailable => (StatusCode::NOT_FOUND, "MODEL_NOT_AVAILABLE"),
            Self::NoHealthyProvider => (StatusCode::SERVICE_UNAVAILABLE, "NO_HEALTHY_PROVIDER"),
            Self::UpstreamTimeout => (StatusCode::GATEWAY_TIMEOUT, "UPSTREAM_TIMEOUT"),
            Self::UpstreamError => (StatusCode::BAD_GATEWAY, "UPSTREAM_ERROR"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        }
    }
}

#[derive(Serialize)]
struct Envelope {
    error: Body,
}
#[derive(Serialize)]
struct Body {
    code: &'static str,
    message: String,
    request_id: Uuid,
}
impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, code) = self.map();
        (
            status,
            Json(Envelope {
                error: Body {
                    code,
                    message: self.to_string(),
                    request_id: Uuid::new_v4(),
                },
            }),
        )
            .into_response()
    }
}
pub type Result<T> = std::result::Result<T, GatewayError>;
