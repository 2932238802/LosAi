use crate::{
    middleware::request_id,
    routes::{admin, analytics, api, crud},
    state::AppState,
};
use axum::{
    Router,
    http::{
        HeaderValue, Method,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    middleware,
    routing::{get, patch, post},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub fn router(s: AppState) -> Router {
    let allowed_origins = s
        .config
        .cors_origins
        .iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();
    let cors = CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::DELETE,
            Method::PATCH,
            Method::OPTIONS,
        ])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            axum::http::header::HeaderName::from_static("x-request-id"),
        ])
        .allow_credentials(false);

    Router::new()
        .route("/health", get(api::health))
        .route("/auth/login", post(api::login))
        .route("/auth/register", post(api::register))
        .route("/v1/models", get(api::models))
        .route("/v1/chat/completions", post(api::chat))
        .route("/user/dashboard", get(api::user_dashboard))
        .route("/user/profile", get(api::user_profile))
        .route("/user/subscription", get(api::user_subscription))
        .route("/user/usage", get(api::user_usage))
        .route("/user/request-logs", get(analytics::user_logs))
        .route(
            "/user/api-keys",
            get(api::user_keys).post(api::user_create_key),
        )
        .route("/user/api-keys/{id}/disable", post(api::disable_key))
        .route(
            "/user/api-keys/{id}",
            patch(crud::user_update_key).delete(crud::user_delete_key),
        )
        .route("/user/api-keys/{id}/status", patch(crud::user_key_status))
        .route("/admin/users", get(api::list_users).post(api::create_user))
        .route(
            "/admin/users/{id}",
            patch(crud::update_user).delete(crud::delete_user),
        )
        .route("/admin/users/{id}/status", patch(crud::update_user_status))
        .route(
            "/admin/users/{id}/password",
            patch(crud::reset_user_password),
        )
        .route("/admin/plans", get(api::list_plans).post(api::create_plan))
        .route(
            "/admin/plans/{id}",
            patch(crud::update_plan).delete(crud::delete_plan),
        )
        .route("/admin/plans/{id}/status", patch(crud::update_plan_status))
        .route("/admin/api-keys", get(api::list_keys).post(api::create_key))
        .route(
            "/admin/api-keys/{id}",
            patch(crud::admin_update_key).delete(crud::admin_delete_key),
        )
        .route("/admin/api-keys/{id}/status", patch(crud::admin_key_status))
        .route(
            "/admin/providers",
            get(admin::providers).post(admin::create_provider),
        )
        .route(
            "/admin/providers/{id}",
            patch(crud::update_provider).delete(crud::delete_provider),
        )
        .route("/admin/providers/{id}/status", patch(crud::provider_status))
        .route("/admin/providers/{id}/check", post(crud::check_provider))
        .route(
            "/admin/credentials",
            get(admin::credentials).post(admin::create_credential),
        )
        .route(
            "/admin/credentials/{id}",
            patch(crud::update_credential).delete(crud::delete_credential),
        )
        .route(
            "/admin/credentials/{id}/status",
            patch(crud::credential_status),
        )
        .route(
            "/admin/credentials/{id}/check",
            post(crud::check_credential),
        )
        .route(
            "/admin/models/{id}",
            patch(crud::update_model).delete(admin::delete_model),
        )
        .route(
            "/admin/models/{id}/status",
            patch(admin::update_model_status),
        )
        .route("/admin/models/check", post(admin::check_model))
        .route(
            "/admin/models",
            get(admin::models).post(admin::create_model),
        )
        .route(
            "/admin/routes",
            get(admin::routes).post(admin::create_route),
        )
        .route(
            "/admin/routes/{id}",
            patch(crud::update_route).delete(crud::delete_route),
        )
        .route("/admin/routes/{id}/status", patch(crud::route_status))
        .route("/admin/routes/{id}/check", post(crud::check_route))
        .route("/admin/dashboard", get(api::admin_dashboard))
        .route("/admin/usage", get(api::admin_usage))
        .route("/admin/request-logs", get(analytics::admin_logs))
        .route("/admin/analytics/models", get(analytics::model_stats))
        .route("/admin/audit-logs", get(crud::audit_logs))
        .layer(middleware::from_fn(request_id::request_id))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(s)
}
