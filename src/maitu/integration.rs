//! Uses an isolated HTTP fixture, never a paid provider. The live DeepSeek check is separate.
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tokio::{
    net::TcpListener,
    sync::{Mutex, watch},
};
use uuid::Uuid;

use super::{
    provider::{ProviderConfig, ProviderStore},
    workflows::{self, AddSourceRequest, CreateTaskRequest},
};
use crate::{
    application::projects, artifacts::ArtifactStore, config::Config, domain::ProjectIntake,
    migrations, web::AppState,
};

#[derive(Default)]
struct Fixture {
    current: AtomicUsize,
    peak: AtomicUsize,
    calls: Mutex<HashMap<String, usize>>,
}

async fn fixture(
    State(state): State<Arc<Fixture>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    assert_eq!(
        headers.get("authorization").unwrap(),
        "Bearer isolated-test-key"
    );
    assert_eq!(request["max_tokens"], 65536);
    assert!(request.get("contextTokens").is_none());
    let prompt = request["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    let title = prompt.lines().next().unwrap().to_owned();
    let instruction = prompt.lines().nth(1).unwrap_or_default();
    let thinking = !instruction.contains("[no-thinking]");
    assert_eq!(
        request["thinking"]["type"],
        if thinking { "enabled" } else { "disabled" }
    );
    let count = {
        let mut calls = state.calls.lock().await;
        let count = calls.entry(title).or_default();
        *count += 1;
        *count
    };
    let concurrent = state.current.fetch_add(1, Ordering::SeqCst) + 1;
    state.peak.fetch_max(concurrent, Ordering::SeqCst);
    let delay = if instruction.contains("[hold]") {
        7000
    } else if instruction.contains("[slow]") {
        4000
    } else {
        900
    };
    tokio::time::sleep(Duration::from_millis(delay)).await;
    state.current.fetch_sub(1, Ordering::SeqCst);
    if instruction.contains("[fail-once]") && count == 1 {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":{"message":"fixture limit"}})),
        )
            .into_response();
    }
    Json(json!({
        "choices":[{"finish_reason":"stop","message":{
            "reasoning_content": if thinking { Some("fixture reasoning must not appear in artifacts") } else { None },
            "content":format!("Fixture output {count}\n{prompt}")
        }}],
        "usage":{"total_tokens":20,"completion_tokens_details":{"reasoning_tokens": if thinking { 3 } else { 0 }}}
    })).into_response()
}

async fn new_task(
    state: &AppState,
    project: Uuid,
    title: &str,
    instruction: &str,
    parents: Vec<Uuid>,
) -> workflows::TaskRecord {
    workflows::create_task(
        &state.pool,
        project,
        CreateTaskRequest {
            request_id: Uuid::new_v4(),
            title: title.into(),
            instruction: instruction.into(),
            output_filename: format!("{title}.md"),
            source_ids: Vec::new(),
            dependency_ids: parents,
        },
    )
    .await
    .unwrap()
}

async fn wait_status(state: &AppState, task: Uuid, status: &str) -> workflows::TaskDetail {
    for _ in 0..300 {
        let detail = workflows::detail(&state.pool, task).await.unwrap();
        if detail.task.status == status {
            return detail;
        }
        if detail.task.status == "failed" && status != "failed" {
            panic!(
                "unexpected task failure: {:?}",
                detail.attempts[0].error_message
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("task did not reach {status}");
}

#[tokio::test]
#[ignore = "Requires isolated PostgreSQL; run scripts/test-maitu-workflow.sh"]
async fn parallel_files_retry_pinned_dependencies_and_process_recovery() {
    let config = Config::from_env().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .unwrap();
    migrations::run(&pool).await.unwrap();
    // Recovery replays must be safe as well as first-time application.
    sqlx::raw_sql(include_str!(
        "../../migrations/0015_maitu_file_workflows.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let fixture_state = Arc::new(Fixture::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let fixture_router = Router::new()
        .route("/chat/completions", post(fixture))
        .with_state(fixture_state.clone());
    let fixture_server = tokio::spawn(async move {
        axum::serve(listener, fixture_router).await.unwrap();
    });
    let provider_root = config.artifact_root.with_file_name("maitu-test-settings");
    let providers = ProviderStore::open(provider_root.clone()).await.unwrap();
    let provider = ProviderConfig {
        base_url: format!("http://{address}"),
        api_key: "isolated-test-key".into(),
        max_tokens: 65536,
        ..Default::default()
    };
    providers.save(provider.clone()).await.unwrap();
    let state = Arc::new(AppState {
        pool,
        config,
        providers,
        tool_proxy_client: reqwest::Client::new(),
    });
    let project = projects::create_project(
        &state.pool,
        ProjectIntake {
            intent: "验证并行资料任务、版本引用与中断恢复".into(),
        },
    )
    .await
    .unwrap();
    let original = workflows::add_source(
        &state.pool,
        project,
        AddSourceRequest {
            filename: "brief.txt".into(),
            content: "original material".into(),
        },
    )
    .await
    .unwrap();
    let a = new_task(&state, project, "A", "Produce A", vec![]).await;
    let b = new_task(&state, project, "B", "[fail-once] Produce B", vec![]).await;
    let c = new_task(&state, project, "C", "[slow] Produce C", vec![]).await;
    let d = new_task(&state, project, "D", "Use the adopted A output", vec![a.id]).await;
    let e = new_task(
        &state,
        project,
        "E",
        "Use the pinned A and B outputs",
        vec![a.id, b.id],
    )
    .await;
    let a_attempt = workflows::start(&state.pool, a.id, Uuid::new_v4())
        .await
        .unwrap();
    let b_attempt = workflows::start(&state.pool, b.id, Uuid::new_v4())
        .await
        .unwrap();
    let c_attempt = workflows::start(&state.pool, c.id, Uuid::new_v4())
        .await
        .unwrap();
    let d_attempt = workflows::start(&state.pool, d.id, Uuid::new_v4())
        .await
        .unwrap();
    let e_attempt = workflows::start(&state.pool, e.id, Uuid::new_v4())
        .await
        .unwrap();
    workflows::add_source(
        &state.pool,
        project,
        AddSourceRequest {
            filename: "later.txt".into(),
            content: "later material".into(),
        },
    )
    .await
    .unwrap();
    let (shutdown, signal) = watch::channel(false);
    let worker_state = state.clone();
    let worker = tokio::spawn(async move {
        workflows::run_worker(worker_state, signal).await.unwrap();
    });

    let first_a = wait_status(&state, a.id, "produced").await;
    assert_eq!(
        fixture_state.peak.load(Ordering::SeqCst),
        3,
        "three actual HTTP requests must overlap"
    );
    let failed_b = wait_status(&state, b.id, "failed").await;
    assert_eq!(
        failed_b.attempts[0].error_code.as_deref(),
        Some("provider_rate_limit")
    );
    assert_eq!(
        workflows::task(&state.pool, c.id).await.unwrap().status,
        "running"
    );
    let input = &first_a.attempts[0].input_snapshot.as_ref().unwrap().0;
    assert_eq!(
        input["sources"].as_array().unwrap().len(),
        1,
        "materials added after queuing must not change this attempt"
    );
    assert_eq!(input["sources"][0]["sha256"], original.sha256);
    assert_eq!(input["provider"]["maxTokens"], provider.max_tokens);
    assert_eq!(input["provider"]["contextTokens"], provider.context_tokens);
    assert_eq!(input["provider"]["thinkingEnabled"], true);
    assert_eq!(
        first_a.attempts[0].usage.as_ref().unwrap().0["completion_tokens_details"]["reasoning_tokens"],
        3
    );
    assert!(input["estimatedInputTokens"].as_u64().unwrap() > 0);
    assert!(!input.to_string().contains("isolated-test-key"));
    workflows::accept(&state.pool, a.id, a_attempt.id)
        .await
        .unwrap();
    for _ in 0..100 {
        let bound:Option<Uuid> = sqlx::query_scalar("SELECT source_attempt_id FROM maitu_attempt_dependencies WHERE attempt_id=$1 AND parent_task_id=$2")
            .bind(e_attempt.id).bind(a.id).fetch_one(&state.pool).await.unwrap();
        if bound == Some(a_attempt.id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let independent_d = wait_status(&state, d.id, "produced").await;
    assert_eq!(
        workflows::task(&state.pool, c.id).await.unwrap().status,
        "running",
        "D should not wait for unrelated C"
    );
    assert_eq!(
        independent_d.attempts[0].input_snapshot.as_ref().unwrap().0["upstream"][0]["attemptId"],
        a_attempt.id.to_string()
    );
    assert_eq!(
        workflows::start(&state.pool, a.id, a_attempt.id)
            .await
            .unwrap()
            .id,
        a_attempt.id,
        "replayed start must not cause another paid request"
    );
    let second_a = workflows::start(&state.pool, a.id, Uuid::new_v4())
        .await
        .unwrap();
    wait_status(&state, a.id, "produced").await;
    workflows::accept(&state.pool, a.id, second_a.id)
        .await
        .unwrap();
    let second_b = workflows::start(&state.pool, b.id, Uuid::new_v4())
        .await
        .unwrap();
    let retry_b = wait_status(&state, b.id, "produced").await;
    assert_eq!(retry_b.attempts.len(), 2);
    assert_eq!(retry_b.attempts[1].id, b_attempt.id);
    assert_eq!(retry_b.attempts[1].status, "failed");
    workflows::accept(&state.pool, b.id, second_b.id)
        .await
        .unwrap();
    let joined_e = wait_status(&state, e.id, "produced").await;
    let upstream = joined_e.attempts[0].input_snapshot.as_ref().unwrap().0["upstream"]
        .as_array()
        .unwrap();
    assert!(
        upstream
            .iter()
            .any(|item| item["attemptId"] == a_attempt.id.to_string()),
        "waiting downstream must retain its earlier pinned A version"
    );
    assert!(
        !upstream
            .iter()
            .any(|item| item["attemptId"] == second_a.id.to_string())
    );
    let old_output = ArtifactStore::new(state.config.artifact_root.clone());
    let old_path: String = sqlx::query_scalar("SELECT storage_path FROM artifacts WHERE id=$1")
        .bind(first_a.attempts[0].artifact_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let old_content = String::from_utf8(old_output.read(&old_path).await.unwrap()).unwrap();
    assert!(old_content.starts_with("Fixture output 1"));
    assert!(!old_content.contains("fixture reasoning must not appear in artifacts"));
    wait_status(&state, c.id, "produced").await;
    let c_done = workflows::detail(&state.pool, c.id).await.unwrap();
    assert!(
        independent_d.attempts[0].request_started_at.unwrap()
            < c_done.attempts[0].response_received_at.unwrap()
    );
    assert_eq!(c_done.attempts[0].id, c_attempt.id);
    assert_eq!(independent_d.attempts[0].id, d_attempt.id);

    state
        .providers
        .save(ProviderConfig {
            api_key: String::new(),
            concurrency: 1,
            ..provider.clone()
        })
        .await
        .unwrap();
    assert!(
        state.providers.get().await.view().configured,
        "blank key should preserve the existing credential"
    );
    let f = new_task(&state, project, "F", "single slot", vec![]).await;
    let g = new_task(&state, project, "G", "single slot", vec![]).await;
    workflows::start(&state.pool, f.id, Uuid::new_v4())
        .await
        .unwrap();
    workflows::start(&state.pool, g.id, Uuid::new_v4())
        .await
        .unwrap();
    let done_f = wait_status(&state, f.id, "produced").await;
    let done_g = wait_status(&state, g.id, "produced").await;
    assert!(
        done_g.attempts[0].request_started_at.unwrap()
            >= done_f.attempts[0].response_received_at.unwrap(),
        "concurrency configuration must affect actual execution"
    );

    let h = new_task(&state, project, "H", "[hold] interrupted request", vec![]).await;
    let i = new_task(&state, project, "I", "survive queued restart", vec![]).await;
    workflows::start(&state.pool, h.id, Uuid::new_v4())
        .await
        .unwrap();
    workflows::start(&state.pool, i.id, Uuid::new_v4())
        .await
        .unwrap();
    wait_status(&state, h.id, "running").await;
    for _ in 0..100 {
        if fixture_state.calls.lock().await.get("任务：H") == Some(&1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(fixture_state.calls.lock().await.get("任务：H"), Some(&1));
    worker.abort();
    let _ = worker.await;
    drop(shutdown);
    let reopened = ProviderStore::open(provider_root).await.unwrap();
    assert_eq!(reopened.get().await.concurrency, 1);
    let (shutdown, signal) = watch::channel(false);
    let worker_state = state.clone();
    let recovered_worker = tokio::spawn(async move {
        workflows::run_worker(worker_state, signal).await.unwrap();
    });
    let interrupted_h = wait_status(&state, h.id, "interrupted").await;
    assert_eq!(interrupted_h.attempts.len(), 1);
    wait_status(&state, i.id, "produced").await;
    assert_eq!(
        fixture_state.calls.lock().await.get("任务：H"),
        Some(&1),
        "interrupted paid requests must not be automatically replayed"
    );
    let graph = workflows::snapshot(&state.pool, &state.providers, project)
        .await
        .unwrap();
    assert_eq!(graph.tasks.len(), 9);
    assert_eq!(graph.dependencies.len(), 3);
    let pending_parent = new_task(&state, project, "pending-parent", "not started", vec![]).await;
    for number in 0..129 {
        let waiting = new_task(
            &state,
            project,
            &format!("waiting-{number}"),
            "wait for parent",
            vec![pending_parent.id],
        )
        .await;
        workflows::start(&state.pool, waiting.id, Uuid::new_v4())
            .await
            .unwrap();
    }
    let ready = new_task(
        &state,
        project,
        "ready-after-blocked",
        "must receive the free slot",
        vec![],
    )
    .await;
    workflows::start(&state.pool, ready.id, Uuid::new_v4())
        .await
        .unwrap();
    wait_status(&state, ready.id, "produced").await;
    state
        .providers
        .save(ProviderConfig {
            thinking_enabled: false,
            ..provider.clone()
        })
        .await
        .unwrap();
    let no_thinking = new_task(
        &state,
        project,
        "non-thinking",
        "[no-thinking] produce a final answer",
        vec![],
    )
    .await;
    workflows::start(&state.pool, no_thinking.id, Uuid::new_v4())
        .await
        .unwrap();
    let no_thinking_result = wait_status(&state, no_thinking.id, "produced").await;
    assert_eq!(
        no_thinking_result.attempts[0]
            .input_snapshot
            .as_ref()
            .unwrap()
            .0["provider"]["thinkingEnabled"],
        false
    );
    assert_eq!(
        no_thinking_result.attempts[0].usage.as_ref().unwrap().0["completion_tokens_details"]["reasoning_tokens"],
        0
    );
    state
        .providers
        .save(ProviderConfig {
            context_tokens: 256,
            max_tokens: 1,
            ..provider.clone()
        })
        .await
        .unwrap();
    let calls_before = fixture_state.calls.lock().await.values().sum::<usize>();
    let over_budget = new_task(
        &state,
        project,
        "over-context-budget",
        &"长资料".repeat(1000),
        vec![],
    )
    .await;
    workflows::start(&state.pool, over_budget.id, Uuid::new_v4())
        .await
        .unwrap();
    let rejected = wait_status(&state, over_budget.id, "failed").await;
    assert_eq!(
        rejected.attempts[0].error_code.as_deref(),
        Some("context_budget_exceeded")
    );
    assert!(rejected.attempts[0].request_started_at.is_none());
    let rejected_input = &rejected.attempts[0].input_snapshot.as_ref().unwrap().0;
    assert_eq!(rejected_input["provider"]["contextTokens"], 256);
    assert!(rejected_input["estimatedInputTokens"].as_u64().unwrap() > 255);
    assert_eq!(
        fixture_state.calls.lock().await.values().sum::<usize>(),
        calls_before,
        "an over-budget task must fail before sending a provider request"
    );
    shutdown.send(true).unwrap();
    recovered_worker.await.unwrap();
    fixture_server.abort();
    println!(
        "Maitu fixture integration passed: real HTTP overlap, failure isolation, explicit retry, immutable inputs, pinned outputs, configurable capacity, thinking modes, final-only artifacts and process recovery."
    );
}
