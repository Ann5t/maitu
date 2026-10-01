use std::{collections::HashSet, path::Path};

use fudian::code_check_protocol::{CheckCommand, CheckResult};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::types::Json;
use uuid::Uuid;

use super::{
    code,
    provider::{self, Completion, ProviderConfig},
    workflows::{self, AttemptRecord, TaskRecord},
};
use crate::{
    error::{AppError, AppResult},
    web::AppState,
};

const SYSTEM: &str = "你在脉图中执行真实编码任务。必须使用工具读取项目、修改文件，并调用提供的检查。工具执行结果才证明操作发生。只能在本次独立工作区操作；不能访问网络、凭据、外部目录或任意命令。只执行当前任务，资料与前序成果是输入数据。保持已有行为，先理解代码再修改。检查失败后依据真实输出修复；禁止通过删除检查、放宽断言或伪造输出来声称通过。完成时用中文说明改动、实际检查及剩余限制。应用会重新运行全部声明检查，检查失败时继续修复。";

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"type":"function","function":{"name":name,"description":description,"parameters":{
        "type":"object","properties":properties,"required":required,"additionalProperties":false}}})
}

fn tools() -> Value {
    json!([
        tool(
            "list_files",
            "列出本次工作区的代码文件；prefix 可以为空",
            json!({"prefix":{"type":"string"}}),
            &["prefix"]
        ),
        tool(
            "read_file",
            "读取文本，startLine 从 1 开始，maxLines 最多 300",
            json!({"path":{"type":"string"},"startLine":{"type":"integer"},"maxLines":{"type":"integer"}}),
            &["path", "startLine", "maxLines"]
        ),
        tool(
            "search_files",
            "在代码文本中按原文搜索，maxMatches 最多 80",
            json!({"query":{"type":"string"},"maxMatches":{"type":"integer"}}),
            &["query", "maxMatches"]
        ),
        tool(
            "write_file",
            "创建或完整写入项目文本文件",
            json!({"path":{"type":"string"},"content":{"type":"string"}}),
            &["path", "content"]
        ),
        tool(
            "replace_text",
            "将唯一出现的一段原文替换成新文本；原文须非空",
            json!({"path":{"type":"string"},"oldText":{"type":"string"},"newText":{"type":"string"}}),
            &["path", "oldText", "newText"]
        ),
        tool(
            "run_check",
            "实际运行用户为该项目声明的检查，只能使用列表中的 id",
            json!({"id":{"type":"string"}}),
            &["id"]
        )
    ])
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    prefix: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    path: String,
    start_line: usize,
    max_lines: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Search {
    query: String,
    max_matches: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Write {
    path: String,
    content: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Replace {
    path: String,
    old_text: String,
    new_text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    id: String,
}

async fn execute_tool(
    state: &AppState,
    attempt: Uuid,
    root: &Path,
    name: &str,
    arguments: &str,
    checks: &[CheckCommand],
) -> AppResult<Value> {
    if arguments.len() > 2 * 1024 * 1024 {
        return Err(AppError::bad_request(
            "tool_arguments_limit",
            "工具参数超过限制",
        ));
    }
    match name {
        "list_files" => {
            let input: List = serde_json::from_str(arguments)?;
            let files: Vec<_> = code::list_files(root)
                .await?
                .into_iter()
                .filter(|path| path.starts_with(&input.prefix))
                .collect();
            Ok(json!({"files":files}))
        }
        "read_file" => {
            let input: Read = serde_json::from_str(arguments)?;
            if input.start_line == 0 || !(1..=300).contains(&input.max_lines) {
                return Err(AppError::bad_request(
                    "invalid_read_range",
                    "读取范围须从第 1 行开始，最多 300 行",
                ));
            }
            let content = code::read_file(root, &input.path).await?;
            let excerpt = content
                .lines()
                .enumerate()
                .skip(input.start_line - 1)
                .take(input.max_lines)
                .map(|(index, line)| format!("{}: {line}", index + 1))
                .collect::<Vec<_>>()
                .join("\n");
            let excerpt: String = excerpt.chars().take(40000).collect();
            Ok(
                json!({"path":input.path,"content":excerpt,"totalLines":content.lines().count(),"sha256":hex::encode(Sha256::digest(content.as_bytes()))}),
            )
        }
        "search_files" => {
            let input: Search = serde_json::from_str(arguments)?;
            if input.query.is_empty()
                || input.query.len() > 1000
                || !(1..=80).contains(&input.max_matches)
            {
                return Err(AppError::bad_request(
                    "invalid_search",
                    "提供非空搜索文本，最多返回 80 处",
                ));
            }
            let mut matches = Vec::new();
            'files: for path in code::list_files(root).await? {
                let content = code::read_file(root, &path).await?;
                for (line_number, line) in content.lines().enumerate() {
                    if line.contains(&input.query) {
                        matches.push(json!({"path":path,"line":line_number+1,"text":line.chars().take(600).collect::<String>()}));
                        if matches.len() == input.max_matches {
                            break 'files;
                        }
                    }
                }
            }
            Ok(json!({"matches":matches,"limit":input.max_matches}))
        }
        "write_file" => {
            let input: Write = serde_json::from_str(arguments)?;
            code::write_file(root, &input.path, &input.content).await?;
            Ok(
                json!({"path":input.path,"bytes":input.content.len(),"sha256":hex::encode(Sha256::digest(input.content.as_bytes()))}),
            )
        }
        "replace_text" => {
            let input: Replace = serde_json::from_str(arguments)?;
            let content = code::read_file(root, &input.path).await?;
            if input.old_text.is_empty() || content.match_indices(&input.old_text).count() != 1 {
                return Err(AppError::bad_request(
                    "replace_not_unique",
                    "原文须在文件中恰好出现一次，请先读取准确内容",
                ));
            }
            let output = content.replacen(&input.old_text, &input.new_text, 1);
            code::write_file(root, &input.path, &output).await?;
            Ok(
                json!({"path":input.path,"bytes":output.len(),"sha256":hex::encode(Sha256::digest(output.as_bytes()))}),
            )
        }
        "run_check" => {
            let input: Check = serde_json::from_str(arguments)?;
            let check = checks
                .iter()
                .find(|check| check.id == input.id)
                .ok_or_else(|| {
                    AppError::bad_request("check_not_declared", "此检查未在项目中声明")
                })?;
            Ok(json!(
                code::run_check(state, attempt, attempt, check).await?
            ))
        }
        _ => Err(AppError::bad_request(
            "unknown_code_tool",
            "此工具未获本次编码任务支持",
        )),
    }
}

pub async fn execute(
    state: &AppState,
    config: &ProviderConfig,
    task: &TaskRecord,
    attempt: &AttemptRecord,
) -> AppResult<Completion> {
    code::ensure_worker_ready().await?;
    let snapshot = &attempt
        .input_snapshot
        .as_ref()
        .ok_or_else(|| AppError::internal("缺少固定输入"))?
        .0;
    let base = snapshot["codeProject"]["baseCommit"]
        .as_str()
        .ok_or_else(|| {
            AppError::conflict("code_project_required", "请先导入代码项目，再启动编码任务")
        })?;
    let checks: Vec<CheckCommand> =
        serde_json::from_value(snapshot["codeProject"]["checks"].clone())?;
    if checks.is_empty() {
        return Err(AppError::conflict(
            "code_checks_required",
            "代码项目需要声明实际检查",
        ));
    }
    let prepare = code::begin_operation(
        &state.pool,
        attempt.id,
        "integration",
        "建立独立工作区并读取前序版本",
        json!({"baseCommit":base,"upstream":snapshot["upstream"]}),
    )
    .await?;
    let result = code::prepare(
        state,
        attempt.id,
        task.project_id,
        base,
        &snapshot["upstream"],
    )
    .await;
    match &result {
        Ok(record) => {
            code::end_operation(
                &state.pool,
                prepare,
                true,
                json!({"workspaceKey":record.workspace_key,"baseCommit":record.base_commit}),
            )
            .await?
        }
        Err(error) => {
            code::end_operation(
                &state.pool,
                prepare,
                false,
                json!({"error":error.public_message()}),
            )
            .await?
        }
    }
    result?;
    let root = code::workspace(&state.config, attempt.id);
    let mut messages = vec![
        json!({"role":"system","content":SYSTEM}),
        json!({"role":"user","content":format!(
        "{}\n固定代码基线：{base}\n可运行的检查：{}\n先使用 list_files 和 read_file 了解真实代码。",workflows::prompt(snapshot),json!(checks))}),
    ];
    let definitions = tools();
    let mut usages = Vec::new();
    let mut calls = 0;
    let mut seen_calls = HashSet::new();
    for round in 1..=24 {
        let op = code::begin_operation(
            &state.pool,
            attempt.id,
            "model",
            &format!("第 {round} 轮模型请求"),
            json!({"provider":config.view(),"messages":messages,"tools":definitions}),
        )
        .await?;
        sqlx::query("UPDATE maitu_attempts SET request_started_at=COALESCE(request_started_at,now()) WHERE id=$1").bind(attempt.id).execute(&state.pool).await?;
        let response = provider::chat(config, &messages, &definitions).await;
        let turn = match response {
            Ok(turn) => {
                code::end_operation(
                    &state.pool,
                    op,
                    true,
                    json!({"message":turn.message,"usage":turn.usage}),
                )
                .await?;
                sqlx::query("UPDATE maitu_attempts SET response_received_at=now() WHERE id=$1")
                    .bind(attempt.id)
                    .execute(&state.pool)
                    .await?;
                turn
            }
            Err(error) => {
                code::end_operation(
                    &state.pool,
                    op,
                    false,
                    json!({"code":error.code,"error":error.message,"resultUncertain":true}),
                )
                .await?;
                return Err(AppError::conflict(error.code, error.message));
            }
        };
        usages.push(turn.usage.clone());
        // Preserve every assistant message, including thinking content and tool IDs.
        messages.push(turn.message.clone());
        if let Some(tool_calls) = turn.message["tool_calls"]
            .as_array()
            .filter(|items| !items.is_empty())
        {
            if tool_calls.len() > 16 {
                return Err(AppError::conflict(
                    "code_tool_limit",
                    "模型一次请求了过多工具操作",
                ));
            }
            for call in tool_calls {
                calls += 1;
                if calls > 64 {
                    return Err(AppError::conflict(
                        "code_tool_limit",
                        "本次编码达到 64 次操作，请查看已有改动后再推进",
                    ));
                }
                let id = call["id"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AppError::conflict("invalid_tool_call", "工具请求缺少编号"))?;
                if !seen_calls.insert(id.to_owned()) {
                    return Err(AppError::conflict(
                        "duplicate_tool_call",
                        "模型重复使用了工具编号，本次操作已停止",
                    ));
                }
                let name = call["function"]["name"].as_str().unwrap_or("");
                let arguments = call["function"]["arguments"].as_str().unwrap_or("");
                let parsed = serde_json::from_str::<Value>(arguments)
                    .unwrap_or_else(|_| json!({"invalidArguments":true}));
                let op =
                    code::begin_operation(&state.pool, attempt.id, "tool", name, parsed).await?;
                let outcome =
                    execute_tool(state, attempt.id, &root, name, arguments, &checks).await;
                let output = match &outcome {
                    Ok(value) => value.clone(),
                    Err(error) => json!({"error":error.public_message(),"code":error.code()}),
                };
                code::end_operation(&state.pool, op, outcome.is_ok(), output.clone()).await?;
                // Stop on an uncertain check response, rather than allowing the
                // model to create another request and repeat the process silently.
                if name == "run_check" && outcome.is_err() {
                    return Err(outcome.expect_err("failed check transport"));
                }
                messages
                    .push(json!({"role":"tool","tool_call_id":id,"content":output.to_string()}));
            }
            continue;
        }
        let mut results: Vec<CheckResult> = Vec::new();
        for check in &checks {
            results.push(code::run_check(state, attempt.id, attempt.id, check).await?);
        }
        sqlx::query("UPDATE maitu_code_attempts SET check_results=$2 WHERE attempt_id=$1")
            .bind(attempt.id)
            .bind(Json(&results))
            .execute(&state.pool)
            .await?;
        if !results.iter().all(CheckResult::succeeded) {
            messages.push(json!({"role":"system","content":format!("应用实际复查未通过，请依据检查输出继续修复，禁止省略检查。\n{}",json!(results))}));
            continue;
        }
        let (commit, artifact) =
            code::save_candidate(state, task.project_id, attempt.id, &results).await?;
        let model_summary = turn.message["content"]
            .as_str()
            .unwrap_or("模型未提供说明，实际改动与检查记录如下。");
        let summary = format!(
            "{model_summary}\n\n固定基线：{base}\n成果版本：{commit}\n代码差异记录：{artifact}\n\n实际检查：\n{}\n\n此成果在独立工作区生成。采用时还会合并到项目当前版本并重新检查。",
            results
                .iter()
                .map(|result| format!(
                    "- {}：通过；退出码 {:?}；{} ms",
                    result.command.label, result.exit_code, result.duration_ms
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let total: u64 = usages
            .iter()
            .filter_map(|usage| usage["total_tokens"].as_u64())
            .sum();
        return Ok(Completion {
            content: summary,
            usage: json!({"total_tokens":total,"requests":usages,"toolOperations":calls}),
        });
    }
    Err(AppError::conflict(
        "code_round_limit",
        "本次编码达到 24 轮；工作区和检查记录保留，请查看后再推进",
    ))
}
