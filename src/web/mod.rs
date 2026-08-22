mod goal_projection;
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
        .route("/ideas", get(handlers::ideas_page))
        .route("/ideas/new", get(handlers::new_idea_page))
        .route("/ideas/{idea_id}", get(handlers::idea_page))
        .route("/ideas/commands", post(handlers::idea_command_form))
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
        .route(
            "/projects/{project_id}/sessions/{session_id}/plugin-install-requests",
            post(handlers::plugin_install_request_form),
        )
        .route("/artifacts/{artifact_id}", get(handlers::artifact_download))
        .route("/api/health", get(handlers::api_health))
        .route(
            "/api/projects",
            get(handlers::api_list_projects).post(handlers::api_create_project),
        )
        .route(
            "/api/v1/ideas",
            get(handlers::api_list_ideas).post(handlers::api_create_idea),
        )
        .route("/api/v1/ideas/{idea_id}", get(handlers::api_idea_snapshot))
        .route(
            "/api/v1/ideas/{idea_id}/sources",
            post(handlers::api_attach_idea_source),
        )
        .route(
            "/api/v1/ideas/{idea_id}/sources/{source_id}/content",
            get(handlers::api_download_idea_source),
        )
        .route(
            "/api/v1/ideas/{idea_id}/commands",
            post(handlers::api_idea_command),
        )
        .route(
            "/api/v1/project-proposals/{proposal_id}/commands",
            post(handlers::api_project_proposal_command),
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
            "/api/v1/projects/{project_id}/goal-branches/{goal_branch_id}/workspace",
            get(handlers::api_goal_workspace),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/runner-jobs",
            post(handlers::api_prepare_runner_job),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/runner-jobs/{job_id}/finalize",
            post(handlers::api_finalize_runner_job),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/runner-jobs/{job_id}/fail",
            post(handlers::api_fail_runner_job),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/action-runs",
            get(handlers::api_list_action_runs).post(handlers::api_enqueue_action_run),
        )
        .route(
            "/api/v1/projects/{project_id}/action-runs/{action_run_id}",
            get(handlers::api_get_action_run),
        )
        .route(
            "/api/v1/projects/{project_id}/action-runs/{action_run_id}/cancel",
            post(handlers::api_cancel_action_run),
        )
        .route(
            "/api/v1/projects/{project_id}/action-runs/{action_run_id}/resolve",
            post(handlers::api_resume_action_run),
        )
        .route(
            "/api/v1/scheduler/workers",
            post(handlers::api_register_scheduler_worker),
        )
        .route(
            "/api/v1/scheduler/claim",
            post(handlers::api_claim_action_run),
        )
        .route(
            "/api/v1/scheduler/action-runs/{action_run_id}/heartbeat",
            post(handlers::api_heartbeat_action_run),
        )
        .route(
            "/api/v1/scheduler/action-runs/{action_run_id}/complete",
            post(handlers::api_complete_action_run),
        )
        .route(
            "/api/v1/scheduler/action-runs/{action_run_id}/fail",
            post(handlers::api_fail_action_run),
        )
        .route(
            "/api/v1/scheduler/action-runs/{action_run_id}/integrations/{integration_id}/prepare",
            post(handlers::api_prepare_integration),
        )
        .route(
            "/api/v1/scheduler/action-runs/{action_run_id}/integrations/{integration_id}/finalize",
            post(handlers::api_finalize_integration),
        )
        .route(
            "/api/v1/scheduler/action-runs/{action_run_id}/tool-lease/activate",
            post(handlers::api_activate_tool_lease),
        )
        .route(
            "/api/v1/scheduler/action-runs/{action_run_id}/tool-lease/finish",
            post(handlers::api_finish_tool_lease),
        )
        .route(
            "/api/v1/scheduler/reconcile",
            post(handlers::api_reconcile_action_runs),
        )
        .route(
            "/api/v1/scheduler/tool-leases/{tool_lease_id}/cleanup",
            post(handlers::api_acknowledge_tool_cleanup),
        )
        .route(
            "/api/v1/projects/{project_id}/notifications",
            get(handlers::api_list_notifications),
        )
        .route(
            "/api/v1/projects/{project_id}/notifications/{notification_id}/read",
            post(handlers::api_mark_notification_read),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/tool-leases",
            post(handlers::api_create_tool_lease),
        )
        .route(
            "/api/v1/projects/{project_id}/tool-leases/{tool_lease_id}",
            get(handlers::api_get_tool_lease),
        )
        .route(
            "/api/v1/projects/{project_id}/tool-leases/{tool_lease_id}/stop",
            post(handlers::api_request_tool_lease_stop),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/context",
            get(handlers::api_session_context),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/context/entries",
            get(handlers::api_session_context_catalog),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/context/read",
            post(handlers::api_read_session_context),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/context/rebuild",
            post(handlers::api_rebuild_session_context),
        )
        .route(
            "/api/v1/plugins",
            get(handlers::api_plugin_catalog).post(handlers::api_register_plugin),
        )
        .route("/api/v1/plugins/seal", post(handlers::api_seal_plugin))
        .route(
            "/api/v1/plugins/install-statement",
            post(handlers::api_plugin_install_statement),
        )
        .route(
            "/api/v1/plugins/install",
            post(handlers::api_install_signed_plugin),
        )
        .route(
            "/api/v1/plugin-publishers",
            post(handlers::api_register_plugin_publisher),
        )
        .route(
            "/api/v1/plugin-publishers/{publisher_id}/revoke",
            post(handlers::api_revoke_plugin_publisher),
        )
        .route(
            "/api/v1/plugin-installations/{installation_id}/revoke",
            post(handlers::api_revoke_plugin_installation),
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
            "/api/v1/plugins/{plugin_id}/{version}/install-proof",
            get(handlers::api_plugin_install_proof),
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
            "/api/v1/projects/{project_id}/sessions/{session_id}/plugin-install-requests",
            post(handlers::api_create_plugin_install_request),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/tool-executions",
            post(handlers::api_prepare_real_tool),
        )
        .route(
            "/api/v1/projects/{project_id}/sessions/{session_id}/tool-executions/{execution_id}/runner-jobs/{job_id}/finalize",
            post(handlers::api_finalize_real_tool),
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
