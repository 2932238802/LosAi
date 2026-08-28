use axum::{extract::Request, middleware::Next, response::Response};
use uuid::Uuid;
pub async fn request_id(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::parse_str(v).ok())
        .unwrap_or_else(Uuid::new_v4);
    request.extensions_mut().insert(id);
    let mut response = next.run(request).await;
    if let Ok(value) = id.to_string().parse() {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}
