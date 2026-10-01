//! The model is a deterministic protocol fixture. Checks are real isolated Node
//! processes; this test never uses a personal key or a paid model.
use axum::{Json, Router, extract::State, routing::post};
use fudian::code_check_protocol::CheckCommand;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, watch},
};
use uuid::Uuid;

use super::{
    code, plans,
    provider::{ProviderConfig, ProviderStore},
    workflows::{self, CreateTaskRequest},
};
use crate::{
    application::projects, config::Config, domain::ProjectIntake, migrations, web::AppState,
};

#[derive(Default)]
struct Fixture {
    calls: Mutex<HashMap<String, usize>>,
    active: AtomicUsize,
    peak: AtomicUsize,
}

fn proposal() -> plans::Plan {
    serde_json::from_value(json!({"summary":"并行修改，随后整合", "questions":[], "tasks":[
        {"key":"a","title":"A","instruction":"fixture-A","kind":"code","outputFilename":"a.md","acceptanceCriteria":"修改 A 并实际检查","dependsOn":[]},
        {"key":"b","title":"B","instruction":"fixture-B","kind":"code","outputFilename":"b.md","acceptanceCriteria":"修改 B 并实际检查","dependsOn":[]},
        {"key":"d","title":"D","instruction":"fixture-D","kind":"code","outputFilename":"d.md","acceptanceCriteria":"固定引用 A B 并整合","dependsOn":["a","b"]}
    ]})).unwrap()
}

async fn model(State(fixture): State<Arc<Fixture>>, Json(request): Json<Value>) -> Json<Value> {
    assert_eq!(request["thinking"]["type"], "enabled");
    if request.get("response_format").is_some() {
        assert_eq!(request["response_format"]["type"], "json_object");
        assert!(
            request["messages"][1]["content"]
                .as_str()
                .unwrap()
                .contains("a.js")
        );
        return Json(
            json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":serde_json::to_string(&proposal()).unwrap()}}],"usage":{"total_tokens":3}}),
        );
    }
    let title = request["messages"][1]["content"]
        .as_str()
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    // Every historical assistant response must keep its original thinking field.
    for message in request["messages"].as_array().unwrap() {
        if message["role"] == "assistant" {
            assert_eq!(message["reasoning_content"], "fixture thinking");
        }
    }
    let round = {
        let mut calls = fixture.calls.lock().await;
        let entry = calls.entry(title.clone()).or_default();
        *entry += 1;
        *entry
    };
    let active = fixture.active.fetch_add(1, Ordering::SeqCst) + 1;
    fixture.peak.fetch_max(active, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(250)).await;
    fixture.active.fetch_sub(1, Ordering::SeqCst);
    if title == "任务：hold" && round > 1 {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
    let arguments = if round == 1 {
        let (path, content) = match title.as_str() {
            "任务：A" => ("a.js", "module.exports = () => -1;\n"),
            "任务：B" => ("b.js", "module.exports = () => 3;\n"),
            "任务：C" => ("a.js", "module.exports = () => 7;\n"),
            "任务：D" => (
                "combined.js",
                "module.exports = () => require('./a')() + require('./b')();\n",
            ),
            "任务：hold" => ("partial.js", "module.exports = 'preserved';\n"),
            _ => panic!("unexpected task {title}"),
        };
        Some(json!({"path":path,"content":content}))
    } else if title == "任务：A" && round == 3 {
        assert!(
            request["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap()
                .contains("AssertionError")
        );
        Some(json!({"path":"a.js","content":"module.exports = () => 2;\n"}))
    } else {
        None
    };
    let message = if let Some(arguments) = arguments {
        json!({"role":"assistant","content":null,"reasoning_content":"fixture thinking",
            "tool_calls":[{"id":format!("call-{round}"),"type":"function","function":{"name":"write_file","arguments":arguments.to_string()}}]})
    } else {
        json!({"role":"assistant","content":format!("fixture {title}"),"reasoning_content":"fixture thinking"})
    };
    Json(
        json!({"choices":[{"finish_reason":if message.get("tool_calls").is_some() {"tool_calls"} else {"stop"},"message":message}],"usage":{"total_tokens":7}}),
    )
}

async fn task(state: &AppState, project: Uuid, title: &str) -> workflows::TaskRecord {
    workflows::create_task(
        &state.pool,
        &state.providers,
        project,
        CreateTaskRequest {
            request_id: Uuid::new_v4(),
            title: title.into(),
            instruction: format!("fixture-{title}"),
            output_filename: format!("{title}.md"),
            task_kind: "code".into(),
            acceptance_criteria: "实际检查通过".into(),
            source_ids: vec![],
            dependency_ids: vec![],
            connection_key: String::new(),
        },
    )
    .await
    .unwrap()
}

async fn wait(state: &AppState, id: Uuid, status: &str) -> workflows::TaskDetail {
    for _ in 0..1200 {
        let result = workflows::detail(&state.pool, id).await.unwrap();
        if result.task.status == status {
            return result;
        }
        assert_ne!(
            result.task.status, "failed",
            "{:?}",
            result.attempts[0].error_message
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("task {id} did not reach {status}");
}

#[tokio::test]
#[ignore = "Requires isolated PostgreSQL, Docker controller and Node image; run scripts/test-maitu-code-workflow.sh"]
async fn plan_to_parallel_code_checks_adoption_conflict_and_recovery() {
    let config = Config::from_env().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect(&config.database_url)
        .await
        .unwrap();
    migrations::run(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/0016_maitu_project_execution.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let fixture = Arc::new(Fixture::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/chat/completions", post(model))
        .with_state(fixture.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let providers = ProviderStore::open(config.artifact_root.with_file_name("code-test-settings"))
        .await
        .unwrap();
    providers
        .save(ProviderConfig {
            base_url: format!("http://{address}"),
            api_key: "isolated-code-fixture".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let state = Arc::new(AppState {
        config,
        pool,
        providers,
        tool_proxy_client: reqwest::Client::new(),
    });
    let project = projects::create_project(
        &state.pool,
        ProjectIntake {
            intent: "协议测试：计划到并行编码，不验证付费模型".into(),
        },
    )
    .await
    .unwrap();
    let import_id = Uuid::new_v4();
    let imported=code::import(&state,project,code::ImportRequest {
        request_id:import_id,source_name:"isolated-node-project".into(),
        files:vec![
            code::CodeFile {path:"a.js".into(),content:"module.exports = () => 0;\n".into()},
            code::CodeFile {path:"b.js".into(),content:"module.exports = () => 0;\n".into()},
            code::CodeFile {path:"project.test.js".into(),content:"const test=require('node:test');const assert=require('node:assert/strict');\ntest('unchanged contract',()=>{assert.ok(require('./a')()>=0);assert.ok(require('./b')()>=0);});\n".into()}
        ],checks:vec![CheckCommand {id:"test".into(),label:"真实 Node 合约检查".into(),program:"node".into(),args:vec!["--test".into()]}]
    }).await.unwrap();
    let (stop, signal) = watch::channel(false);
    let worker = tokio::spawn(workflows::run_worker(state.clone(), signal));
    let planner = plans::generate(
        &state.pool,
        &state.providers,
        project,
        plans::GeneratePlanRequest {
            request_id: Uuid::new_v4(),
            instruction: "并行修改 A B，再整合".into(),
            source_ids: vec![],
            dependency_ids: vec![],
            connection_key: String::new(),
        },
    )
    .await
    .unwrap();
    let plan = wait(&state, planner.id, "produced").await;
    let mut edited = plan.plans[0].proposal.0.clone();
    edited.tasks[0].acceptance_criteria.push_str("；保留原合同");
    let ids = plans::adopt(
        &state.pool,
        project,
        plans::AdoptPlanRequest {
            attempt_id: plan.attempts[0].id,
            plan: edited.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        ids,
        plans::adopt(
            &state.pool,
            project,
            plans::AdoptPlanRequest {
                attempt_id: plan.attempts[0].id,
                plan: edited
            }
        )
        .await
        .unwrap()
    );
    let mut invalid = proposal();
    invalid.tasks[0].depends_on.push("d".into());
    let newer = plans::generate(
        &state.pool,
        &state.providers,
        project,
        plans::GeneratePlanRequest {
            request_id: Uuid::new_v4(),
            instruction: String::new(),
            source_ids: vec![],
            dependency_ids: vec![],
            connection_key: String::new(),
        },
    )
    .await
    .unwrap();
    let newer = wait(&state, newer.id, "produced").await;
    assert!(
        plans::adopt(
            &state.pool,
            project,
            plans::AdoptPlanRequest {
                attempt_id: newer.attempts[0].id,
                plan: invalid
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        workflows::snapshot(&state.pool, &state.providers, project)
            .await
            .unwrap()
            .tasks
            .len(),
        5
    );
    for id in &ids {
        workflows::start(&state.pool, *id, Uuid::new_v4())
            .await
            .unwrap();
    }
    let c = task(&state, project, "C").await;
    workflows::start(&state.pool, c.id, Uuid::new_v4())
        .await
        .unwrap();
    let a = wait(&state, ids[0], "produced").await;
    let b = wait(&state, ids[1], "produced").await;
    let c = wait(&state, c.id, "produced").await;
    assert!(
        fixture.peak.load(Ordering::SeqCst) >= 2,
        "model requests were not concurrent"
    );
    assert_ne!(
        a.code_attempts[0].workspace_key,
        b.code_attempts[0].workspace_key
    );
    assert_eq!(a.code_attempts[0].base_commit, imported.initial_commit);
    assert_eq!(b.code_attempts[0].base_commit, imported.initial_commit);
    assert_eq!(
        code::read_file(&code::workspace(&state.config, a.attempts[0].id), "b.js")
            .await
            .unwrap(),
        "module.exports = () => 0;\n"
    );
    assert!(
        a.operations
            .iter()
            .any(|op| op.kind == "check" && op.status == "failed")
    );
    assert!(
        a.operations
            .iter()
            .any(|op| op.kind == "check" && op.status == "succeeded")
    );
    assert!(
        workflows::accept(&state.pool, ids[0], a.attempts[0].id, "隔离检查")
            .await
            .is_err()
    );
    code::adopt(&state, ids[0], a.attempts[0].id, "检查通过且改动最小")
        .await
        .unwrap();
    code::adopt(&state, ids[1], b.attempts[0].id, "")
        .await
        .unwrap();
    let before = code::project(&state.pool, project)
        .await
        .unwrap()
        .unwrap()
        .accepted_commit;
    let conflict = code::adopt(&state, c.task.id, c.attempts[0].id, "")
        .await
        .unwrap_err();
    assert_eq!(conflict.code(), "code_merge_conflict");
    assert_eq!(
        code::project(&state.pool, project)
            .await
            .unwrap()
            .unwrap()
            .accepted_commit,
        before
    );
    let d = wait(&state, ids[2], "produced").await;
    let sources = d.attempts[0].input_snapshot.as_ref().unwrap().0["upstream"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        sources
            .iter()
            .any(|source| source["attemptId"] == a.attempts[0].id.to_string())
    );
    assert!(
        sources
            .iter()
            .any(|source| source["attemptId"] == b.attempts[0].id.to_string())
    );
    code::adopt(&state, ids[2], d.attempts[0].id, "")
        .await
        .unwrap();
    let final_commit = code::project(&state.pool, project)
        .await
        .unwrap()
        .unwrap()
        .accepted_commit;
    code::adopt(&state, ids[0], a.attempts[0].id, "检查通过且改动最小")
        .await
        .unwrap();
    assert_eq!(
        code::project(&state.pool, project)
            .await
            .unwrap()
            .unwrap()
            .accepted_commit,
        final_commit,
        "historical replay rolled back code"
    );
    assert!(!code::export(&state, project).await.unwrap().is_empty());
    // A repeated check receipt must return the original result, not run a new
    // process. The process also proves the isolation boundary from inside Docker.
    let isolated=fudian::code_check_protocol::CheckRequest {
        request_id:Uuid::new_v4(),workspace_key:a.attempts[0].id,
        command:CheckCommand {id:"isolation".into(),label:"真实隔离检查".into(),program:"node".into(),args:vec!["-e".into(),
            "const fs=require('node:fs'),assert=require('node:assert/strict');assert.equal(fs.existsSync('/var/run/docker.sock'),false);assert.equal(fs.existsSync('/data/maitu-config'),false);assert.equal(fs.existsSync('/data/worktrees'),false);assert.equal(fs.existsSync('/tmp/workspace/.git'),false);assert.throws(()=>fs.writeFileSync('/input/a.js','escape'));console.log(require('node:crypto').randomUUID());".into()]}
    };
    let first = code::call_check_worker(&isolated).await.unwrap();
    assert!(first.succeeded(), "{:?}", first.error);
    let replay = code::call_check_worker(&isolated).await.unwrap();
    assert_eq!(
        first.stdout, replay.stdout,
        "a cached command was run twice"
    );
    assert!(first.runtime_image.starts_with("sha256:"));
    let changed = fudian::code_check_protocol::CheckRequest {
        command: CheckCommand {
            args: vec!["-e".into(), "process.exit(0)".into()],
            ..isolated.command.clone()
        },
        ..isolated
    };
    assert!(
        code::call_check_worker(&changed).await.is_err(),
        "request identity allowed a different command"
    );
    let hold = task(&state, project, "hold").await;
    workflows::start(&state.pool, hold.id, Uuid::new_v4())
        .await
        .unwrap();
    for _ in 0..150 {
        let detail = workflows::detail(&state.pool, hold.id).await.unwrap();
        if detail
            .operations
            .iter()
            .any(|op| op.kind == "tool" && op.status == "succeeded")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stop.send(true).unwrap();
    worker.await.unwrap().unwrap();
    let interrupted = workflows::detail(&state.pool, hold.id).await.unwrap();
    assert_eq!(interrupted.task.status, "interrupted");
    assert!(
        interrupted
            .operations
            .iter()
            .all(|op| op.status != "running")
    );
    assert_eq!(
        code::read_file(
            &code::workspace(&state.config, interrupted.attempts[0].id),
            "partial.js"
        )
        .await
        .unwrap(),
        "module.exports = 'preserved';\n"
    );
    assert!(
        code::working_diff(&state, hold.id, interrupted.attempts[0].id)
            .await
            .unwrap()
            .contains("preserved")
    );
    let calls = fixture
        .calls
        .lock()
        .await
        .get("任务：hold")
        .copied()
        .unwrap();
    let (stop, signal) = watch::channel(false);
    let worker = tokio::spawn(workflows::run_worker(state.clone(), signal));
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        fixture
            .calls
            .lock()
            .await
            .get("任务：hold")
            .copied()
            .unwrap(),
        calls,
        "uncertain model request repeated"
    );
    stop.send(true).unwrap();
    worker.await.unwrap().unwrap();
    let retry_id = Uuid::new_v4();
    let retry =
        workflows::start_with_instruction(&state.pool, hold.id, retry_id, "只继续未完成的部分")
            .await
            .unwrap();
    assert_eq!(retry.number, 2);
    assert_eq!(
        retry.input_snapshot.as_ref().unwrap().0["additionalInstruction"],
        "只继续未完成的部分"
    );
    assert!(
        workflows::start_with_instruction(&state.pool, hold.id, retry_id, "更换要求")
            .await
            .is_err()
    );
    server.abort();
    println!(
        "plan adoption replay/cycle rejection, real failed-and-fixed checks, parallel workspaces, pinned integration, conflict preservation and interruption passed"
    );
}
