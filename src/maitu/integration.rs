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
use fudian::code_check_protocol::CheckCommand;

/// Fixture credentials are assembled at run time so no literal key text lives
/// in this file; they only authenticate against the local fixture server.
fn fixture_key(name: &str) -> String {
    format!("isolated-{name}-key")
}

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
    let authorization = headers.get("authorization").unwrap().to_str().unwrap();
    let steady_auth = format!("Bearer {}", fixture_key("test"));
    let flaky_auth = format!("Bearer {}", fixture_key("flaky"));
    assert!(authorization == steady_auth || authorization == flaky_auth);
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
    // The "flaky" connection always refuses: auth passes, the service answers 429.
    if authorization == flaky_auth {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":{"message":"fixture limit"}})),
        )
            .into_response();
    }
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
        &state.providers,
        project,
        CreateTaskRequest {
            request_id: Uuid::new_v4(),
            title: title.into(),
            instruction: instruction.into(),
            output_filename: format!("{title}.md"),
            task_kind: "file".into(),
            acceptance_criteria: String::new(),
            source_ids: Vec::new(),
            dependency_ids: parents,
            connection_key: String::new(),
        },
    )
    .await
    .unwrap()
}

async fn wait_attempt_error(state: &AppState, task: Uuid, code: &str) -> workflows::TaskDetail {
    for _ in 0..600 {
        let detail = workflows::detail(&state.pool, task).await.unwrap();
        if detail
            .attempts
            .iter()
            .any(|attempt| attempt.error_code.as_deref() == Some(code))
        {
            return detail;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no attempt recorded {code}");
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
        api_key: fixture_key("test"),
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
    let app_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app_address = app_listener.local_addr().unwrap();
    let app_router = crate::web::router(state.clone());
    let app_server = tokio::spawn(async move {
        axum::serve(
            app_listener,
            app_router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
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
    let download = reqwest::get(format!(
        "http://{app_address}/artifacts/{}",
        first_a.attempts[0].artifact_id.unwrap()
    ))
    .await
    .unwrap();
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download.headers()["content-disposition"],
        "inline; filename*=UTF-8''A.md"
    );
    assert!(download.text().await.unwrap().contains("Fixture output 1"));
    assert_eq!(
        fixture_state.peak.load(Ordering::SeqCst),
        3,
        "three actual HTTP requests must overlap"
    );
    let failed_b = wait_attempt_error(&state, b.id, "provider_rate_limit").await;
    assert_eq!(
        workflows::task(&state.pool, c.id).await.unwrap().status,
        "running"
    );
    let _ = failed_b;
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
    assert!(!input.to_string().contains(&fixture_key("test")));
    workflows::accept(&state.pool, a.id, a_attempt.id, "")
        .await
        .unwrap();
    for _ in 0..100 {
        let bound: Option<Uuid> = sqlx::query_scalar("SELECT source_attempt_id FROM maitu_attempt_dependencies WHERE attempt_id=$1 AND parent_task_id=$2")
            .bind(e_attempt.id).bind(a.id).fetch_one(&state.pool).await.unwrap();
        if bound == Some(a_attempt.id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let independent_d = wait_status(&state, d.id, "produced").await;
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
    workflows::accept(&state.pool, a.id, second_a.id, "")
        .await
        .unwrap();
    let second_b = workflows::start(&state.pool, b.id, Uuid::new_v4())
        .await
        .unwrap();
    let retry_b = wait_status(&state, b.id, "produced").await;
    assert!(
        retry_b.attempts.len() >= 3,
        "the rate-limited first attempt must have auto-retried before this manual retry"
    );
    assert_eq!(retry_b.attempts[2].id, b_attempt.id);
    assert_eq!(retry_b.attempts[2].status, "failed");
    assert_eq!(
        retry_b.attempts[1].connection_key, retry_b.attempts[2].connection_key,
        "a single-connection store retries on the same connection"
    );
    workflows::accept(&state.pool, b.id, second_b.id, "")
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
        state.providers.primary().await.view().configured,
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
    assert_eq!(reopened.primary().await.concurrency, 1);
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
        "非思考 结论",
        "[no-thinking] produce a final answer",
        vec![],
    )
    .await;
    workflows::start(&state.pool, no_thinking.id, Uuid::new_v4())
        .await
        .unwrap();
    let no_thinking_result = wait_status(&state, no_thinking.id, "produced").await;
    let download = reqwest::get(format!(
        "http://{app_address}/artifacts/{}",
        no_thinking_result.attempts[0].artifact_id.unwrap()
    ))
    .await
    .unwrap();
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download.headers()["content-disposition"],
        "inline; filename*=UTF-8''%E9%9D%9E%E6%80%9D%E8%80%83%20%E7%BB%93%E8%AE%BA.md"
    );
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
    // Cancelling a queued attempt must not send any provider request.
    let j = new_task(&state, project, "J", "cancel before request", vec![]).await;
    workflows::start(&state.pool, j.id, Uuid::new_v4())
        .await
        .unwrap();
    workflows::cancel(&state.pool, j.id).await.unwrap();
    let cancelled = wait_status(&state, j.id, "cancelled").await;
    assert_eq!(cancelled.attempts[0].status, "cancelled");
    assert!(
        cancelled.attempts[0].request_started_at.is_none(),
        "a queued cancellation must not reach the provider"
    );

    shutdown.send(true).unwrap();
    recovered_worker.await.unwrap();
    app_server.abort();
    fixture_server.abort();
    println!(
        "Maitu fixture integration passed: real HTTP overlap, failure isolation, automatic bounded retry, explicit retry, immutable inputs, pinned outputs, configurable capacity, thinking modes, final-only artifacts, named downloads and process recovery."
    );
}

#[tokio::test]
#[ignore = "Requires isolated PostgreSQL; run scripts/test-maitu-workflow.sh"]
async fn rate_limited_connection_fails_over_bounded_and_records_usage_per_connection() {
    let config = Config::from_env().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .unwrap();
    migrations::run(&pool).await.unwrap();
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
    let provider_root = config
        .artifact_root
        .with_file_name("maitu-test-connections");
    let providers = ProviderStore::open(provider_root.clone()).await.unwrap();
    let base = format!("http://{address}");
    providers
        .save(ProviderConfig {
            key: "flaky".into(),
            label: "限流连接".into(),
            base_url: base.clone(),
            api_key: fixture_key("flaky"),
            concurrency: 8,
            max_tokens: 65536,
            ..Default::default()
        })
        .await
        .unwrap();
    providers
        .save(ProviderConfig {
            key: "steady".into(),
            label: "可用连接".into(),
            base_url: base.clone(),
            api_key: fixture_key("test"),
            concurrency: 1,
            max_tokens: 65536,
            ..Default::default()
        })
        .await
        .unwrap();
    // The untouched default connection has no credentials; drop it so the
    // fixture store contains exactly the two connections under test.
    providers.delete("deepseek").await.unwrap();
    let state = Arc::new(AppState {
        pool,
        config,
        providers,
        tool_proxy_client: reqwest::Client::new(),
    });
    let project = projects::create_project(
        &state.pool,
        ProjectIntake {
            intent: "验证多连接调度、故障转移与重试边界".into(),
        },
    )
    .await
    .unwrap();
    let (shutdown, signal) = watch::channel(false);
    let worker_state = state.clone();
    let worker = tokio::spawn(async move {
        workflows::run_worker(worker_state, signal).await.unwrap();
    });

    // Automatic task: claimed by the first listed connection ("flaky"), which
    // always returns 429. The retry must land on the other connection.
    let auto = new_task(
        &state,
        project,
        "auto-failover",
        "Produce with any connection",
        vec![],
    )
    .await;
    workflows::start(&state.pool, auto.id, Uuid::new_v4())
        .await
        .unwrap();
    let produced = wait_status(&state, auto.id, "produced").await;
    assert_eq!(produced.attempts.len(), 2, "one failover retry, no more");
    let failed = &produced.attempts[1];
    let success = &produced.attempts[0];
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.connection_key, "flaky");
    assert_eq!(failed.error_code.as_deref(), Some("provider_rate_limit"));
    assert_eq!(
        success.connection_key, "steady",
        "the retry switched connections"
    );
    assert_eq!(success.usage.as_ref().unwrap().0["total_tokens"], 20);
    assert!(failed.usage.is_none());
    let events: Vec<String> = produced
        .events
        .into_values()
        .flat_map(|list| list.into_iter().map(|event| event.message))
        .collect();
    assert!(
        events.iter().any(|message| message.contains("使用连接")),
        "records must name the connection that actually served the request"
    );

    // A task pinned to the failing connection retries on that same connection
    // with backoff and stops after the bounded number of attempts.
    let pinned = new_task(
        &state,
        project,
        "pinned-limited",
        "Produce on the pinned connection",
        vec![],
    )
    .await;
    sqlx::query("UPDATE maitu_tasks SET connection_key='flaky' WHERE id=$1")
        .bind(pinned.id)
        .execute(&state.pool)
        .await
        .unwrap();
    workflows::start(&state.pool, pinned.id, Uuid::new_v4())
        .await
        .unwrap();
    let exhausted = wait_status(&state, pinned.id, "failed").await;
    assert_eq!(
        exhausted.attempts.len(),
        4,
        "original attempt plus three bounded automatic retries"
    );
    assert!(
        exhausted
            .attempts
            .iter()
            .all(|attempt| attempt.connection_key == "flaky")
    );
    assert_eq!(exhausted.task.wait_reason, None);

    // A task pinned to a connection that does not exist explains itself.
    let missing = new_task(
        &state,
        project,
        "pinned-missing",
        "Produce on a missing connection",
        vec![],
    )
    .await;
    sqlx::query("UPDATE maitu_tasks SET connection_key='gone' WHERE id=$1")
        .bind(missing.id)
        .execute(&state.pool)
        .await
        .unwrap();
    workflows::start(&state.pool, missing.id, Uuid::new_v4())
        .await
        .unwrap();
    for _ in 0..100 {
        let task = workflows::task(&state.pool, missing.id).await.unwrap();
        if task
            .wait_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("指定的模型连接当前不可用"))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let stuck = workflows::task(&state.pool, missing.id).await.unwrap();
    assert_eq!(stuck.status, "queued");
    assert!(
        stuck
            .wait_reason
            .unwrap()
            .contains("指定的模型连接当前不可用"),
        "a pinned task on an unusable connection must say so"
    );

    // Queue fairness: a task pinned to the busy connection stays queued without
    // losing its slot and runs as soon as that connection frees up.
    let busy = new_task(
        &state,
        project,
        "busy-steady",
        "[hold] occupy the steady connection",
        vec![],
    )
    .await;
    sqlx::query("UPDATE maitu_tasks SET connection_key='steady' WHERE id=$1")
        .bind(busy.id)
        .execute(&state.pool)
        .await
        .unwrap();
    workflows::start(&state.pool, busy.id, Uuid::new_v4())
        .await
        .unwrap();
    wait_status(&state, busy.id, "running").await;
    let queued = new_task(
        &state,
        project,
        "queued-steady",
        "Produce right after the busy one",
        vec![],
    )
    .await;
    sqlx::query("UPDATE maitu_tasks SET connection_key='steady' WHERE id=$1")
        .bind(queued.id)
        .execute(&state.pool)
        .await
        .unwrap();
    workflows::start(&state.pool, queued.id, Uuid::new_v4())
        .await
        .unwrap();
    let queued_done = wait_status(&state, queued.id, "produced").await;
    assert_eq!(
        queued_done.attempts.len(),
        1,
        "queued tasks must not be retried or dropped"
    );
    assert_eq!(queued_done.attempts[0].connection_key, "steady");
    let busy_done = wait_status(&state, busy.id, "produced").await;
    assert!(
        busy_done.attempts[0].response_received_at.unwrap()
            <= queued_done.attempts[0].request_started_at.unwrap(),
        "the pinned task must wait for the busy connection's free slot"
    );

    // 编码任务在检查服务未运行时，点击执行必须立即以未调用模型的状态失败，
    // 而不是排队等待连接或调度节奏；补充要求后的再次尝试同样保留输入与结论。
    let code_task = workflows::create_task(
        &state.pool,
        &state.providers,
        project,
        CreateTaskRequest {
            request_id: Uuid::new_v4(),
            title: "code-entry".into(),
            instruction: "修改 a.js 并运行检查".into(),
            output_filename: "code-entry.md".into(),
            task_kind: "code".into(),
            acceptance_criteria: String::new(),
            source_ids: Vec::new(),
            dependency_ids: Vec::new(),
            connection_key: String::new(),
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO maitu_code_projects(project_id,import_request_id,source_name,import_hash,initial_commit,accepted_commit,checks,file_count,size_bytes) VALUES($1,$2,'fixture-source','fixture-hash','fixture','fixture',$3,1,16)",
    )
    .bind(project)
    .bind(Uuid::new_v4())
    .bind(sqlx::types::Json(vec![CheckCommand {
        id: "node".into(),
        label: "node".into(),
        program: "node".into(),
        args: vec!["--version".into()],
    }]))
    .execute(&state.pool)
    .await
    .unwrap();
    let refused = workflows::start(&state.pool, code_task.id, Uuid::new_v4())
        .await
        .unwrap();
    assert_eq!(refused.status, "failed");
    assert_eq!(
        refused.error_code.as_deref(),
        Some("code_worker_unavailable")
    );
    assert!(
        refused.request_started_at.is_none() && refused.started_at.is_none(),
        "预检失败不能占用执行或调用模型的时序"
    );
    let retried = workflows::start_with_instruction(
        &state.pool,
        code_task.id,
        Uuid::new_v4(),
        "环境就绪后再处理；原尝试保留",
    )
    .await
    .unwrap();
    assert_eq!(retried.number, 2);
    assert!(retried.request_started_at.is_none());
    let code_detail = workflows::detail(&state.pool, code_task.id).await.unwrap();
    assert_eq!(code_detail.task.status, "failed");
    assert_eq!(code_detail.attempts.len(), 2);
    assert!(
        code_detail
            .attempts
            .iter()
            .all(|attempt| attempt.request_started_at.is_none())
    );
    assert!(
        code_detail.attempts[0].input_snapshot.as_ref().unwrap().0["additionalInstruction"]
            == json!("环境就绪后再处理；原尝试保留")
    );
    assert!(fixture_state.calls.lock().await.get("code-entry").is_none());

    // 两个持续限流的固定任务让 flaky 长时间处于退避；steady 短暂失败一次的
    // 时间窗内所有连接都在冷却，自动选择连接的排队任务必须说明真实原因，
    // 退避结束后恢复常规提示并正常执行。
    let mut hold_ids = Vec::new();
    for name in ["flaky-hold-a", "flaky-hold-b"] {
        let hold = new_task(&state, project, name, "Occupy flaky with failures", vec![]).await;
        sqlx::query("UPDATE maitu_tasks SET connection_key='flaky' WHERE id=$1")
            .bind(hold.id)
            .execute(&state.pool)
            .await
            .unwrap();
        workflows::start(&state.pool, hold.id, Uuid::new_v4())
            .await
            .unwrap();
        hold_ids.push(hold.id);
    }
    wait_attempt_error(&state, hold_ids[0], "provider_rate_limit").await;
    wait_attempt_error(&state, hold_ids[1], "provider_rate_limit").await;
    let blip = new_task(
        &state,
        project,
        "steady-blip",
        "[fail-once] Fail once on steady",
        vec![],
    )
    .await;
    sqlx::query("UPDATE maitu_tasks SET connection_key='steady' WHERE id=$1")
        .bind(blip.id)
        .execute(&state.pool)
        .await
        .unwrap();
    workflows::start(&state.pool, blip.id, Uuid::new_v4())
        .await
        .unwrap();
    let cooling_wait = new_task(
        &state,
        project,
        "cooling-wait",
        "Explain why automatic tasks wait",
        vec![],
    )
    .await;
    workflows::start(&state.pool, cooling_wait.id, Uuid::new_v4())
        .await
        .unwrap();
    let mut saw_cooling_reason = false;
    for _ in 0..200 {
        let task = workflows::task(&state.pool, cooling_wait.id).await.unwrap();
        if task
            .wait_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("所有可用连接都在限流退避"))
        {
            saw_cooling_reason = true;
            break;
        }
        if task.status == "produced" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        saw_cooling_reason,
        "所有连接退避时，自动任务的等待原因必须说明限流退避"
    );
    let cooled_done = wait_status(&state, cooling_wait.id, "produced").await;
    assert!(cooled_done.attempts.len() <= 4);
    let blip_done = wait_status(&state, blip.id, "produced").await;
    assert!(blip_done.attempts.len() <= 2);

    let graph = workflows::snapshot(&state.pool, &state.providers, project)
        .await
        .unwrap();
    assert_eq!(graph.connections.len(), 2);
    assert!(graph.tasks.len() >= 4);
    shutdown.send(true).unwrap();
    worker.await.unwrap();
    fixture_server.abort();
    println!(
        "Maitu multi-connection fixture passed: automatic failover across connections, bounded retries with backoff on one connection, per-connection capacity, queue fairness, usage and connection recorded per attempt."
    );
}
