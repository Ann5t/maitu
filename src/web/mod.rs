mod handlers;
mod views;

use std::sync::Arc;

use axum::{
    Router,
    http::{HeaderName, HeaderValue},
    routing::{get, post, put},
};
use sqlx::PgPool;
use tower_http::{
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    services::ServeDir,
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

use crate::config::Config;

#[derive(Clone, Debug)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
}

pub fn router(state: Arc<AppState>) -> Router {
    let request_id = HeaderName::from_static("x-request-id");
    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/new", get(handlers::new_project_page))
        .route("/projects", post(handlers::create_project_form))
        .route("/projects/{project_id}", get(handlers::project_page))
        .route(
            "/projects/{project_id}/actions",
            post(handlers::project_action_form),
        )
        .route(
            "/projects/{project_id}/graph",
            post(handlers::graph_action_form),
        )
        .route(
            "/projects/{project_id}/goal-commands",
            post(handlers::goal_command_form),
        )
        .route("/artifacts/{artifact_id}", get(handlers::artifact_download))
        .route("/api/health", get(handlers::api_health))
        .route(
            "/api/projects",
            get(handlers::api_list_projects).post(handlers::api_create_project),
        )
        .route(
            "/api/projects/{project_id}",
            get(handlers::api_project_snapshot),
        )
        .route(
            "/api/projects/{project_id}/actions",
            post(handlers::api_project_action),
        )
        .route(
            "/api/projects/{project_id}/graph",
            post(handlers::api_graph_action),
        )
        .route(
            "/api/v1/projects/{project_id}/goal-graph",
            get(handlers::api_goal_snapshot),
        )
        .route(
            "/api/v1/projects/{project_id}/goal-commands",
            post(handlers::api_goal_command),
        )
        .route(
            "/api/v1/plugins",
            get(handlers::api_plugin_catalog).post(handlers::api_register_plugin),
        )
        .route(
            "/api/v1/plugins/resolve",
            post(handlers::api_resolve_plugin),
        )
        .route(
            "/api/v1/plugins/{plugin_id}/{version}",
            get(handlers::api_plugin_detail),
        )
        .route(
            "/api/v1/environments",
            post(handlers::api_create_environment),
        )
        .route(
            "/api/v1/environments/{environment_id}",
            get(handlers::api_environment_detail),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/environment",
            post(handlers::api_bind_environment),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/tool-calls",
            post(handlers::api_execute_tool),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/inputs",
            get(handlers::api_list_inputs).post(handlers::api_begin_input),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/inputs/{input_id}/chunks",
            put(handlers::api_append_input_chunk),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/inputs/{input_id}/finish",
            post(handlers::api_finish_input),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/inputs/{input_id}/import",
            post(handlers::api_import_input),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/inputs/{input_id}/content",
            get(handlers::api_download_input),
        )
        .route(
            "/api/artifacts/{artifact_id}",
            get(handlers::artifact_download),
        )
        .nest_service("/assets", ServeDir::new("assets"))
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("same-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'",
            ),
        ))
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
