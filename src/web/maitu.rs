use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
};
use maud::{DOCTYPE, Markup, html};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    application::projects,
    error::{AppError, AppResult},
    maitu::{
        code,
        plans::{self, AdoptPlanRequest, GeneratePlanRequest},
        provider::ProviderConfig,
        workflows::{self, AcceptRequest, AddSourceRequest, CreateTaskRequest, StartRequest},
    },
};

use super::AppState;

fn icon_project() -> Markup {
    html! {
        svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
            circle cx="5" cy="5.5" r="2.2" {}
            circle cx="5" cy="14.5" r="2.2" {}
            circle cx="15" cy="10" r="2.2" {}
            path d="M7.1 6.4 12.9 9.1M7.1 13.6 12.9 10.9" {}
        }
    }
}

fn icon_idea() -> Markup {
    html! {
        svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
            path d="M10 2.8a4.6 4.6 0 0 1 2.8 8.3c-.6.5-.9 1.1-1 1.9l-.1.5H8.3l-.1-.5c-.1-.8-.4-1.4-1-1.9A4.6 4.6 0 0 1 10 2.8z" {}
            path d="M8.4 16.2h3.2M8.9 18h2.2" {}
        }
    }
}

fn icon_connection() -> Markup {
    html! {
        svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
            path d="M4 8a6 6 0 0 1 12 0v1.5a1.5 1.5 0 0 1-1.5 1.5h-1.6a1.4 1.4 0 0 0 0 2.8h.6a1.4 1.4 0 0 1 0 2.8H14a6.2 6.2 0 0 1-4.5-1.9" {}
            path d="M4 8h2.4a1.4 1.4 0 0 1 0 2.8h-.6a1.4 1.4 0 0 0 0 2.8h1" {}
        }
    }
}

pub(crate) fn shell(title: &str, mode: &str, project_id: Option<Uuid>, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · 脉图" }
                link rel="icon" href="/assets/icon.svg";
                link rel="stylesheet" href="/assets/maitu.css";
                script src="/assets/theme.js" defer {}
                script src="/assets/maitu.js?v=ui-5" defer {}
                @if mode == "ideas" || mode == "idea" { script src="/assets/maitu-ideas.js?v=4" defer {} }
                @if mode == "project" { script src="/assets/maitu-goals.js?v=4" defer {} }
            }
            body data-maitu-mode=(mode) data-project-id=(project_id.map(|id| id.to_string()).unwrap_or_default()) {
                div class="maitu-shell" {
                    aside class="maitu-nav" {
                        a class="maitu-brand" href="/" { span class="maitu-brand-mark" aria-hidden="true" { "脉" } strong { "脉图" } }
                        nav aria-label="主要导航" {
                            a href="/" class=(if mode == "dashboard" || mode == "project" { "is-active" } else { "" }) { (icon_project()) span { "项目" } }
                            a href="/maitu/ideas" class=(if mode == "ideas" || mode == "idea" { "is-active" } else { "" }) { (icon_idea()) span { "想法" } }
                            a href="/maitu/settings" class=(if mode == "settings" { "is-active" } else { "" }) { (icon_connection()) span { "模型连接" } }
                        }
                        div class="maitu-nav-bottom" {
                            div class="maitu-theme-toggle" role="group" aria-label="颜色主题" {
                                button type="button" data-theme-set="light" aria-label="浅色主题" { "浅色" }
                                button type="button" data-theme-set="dark" aria-label="深色主题" { "深色" }
                            }
                        }
                    }
                    main class="maitu-main" { (content) }
                }
                div id="maitu-feedback" class="maitu-feedback" role="status" hidden {}
            }
        }
    }
}

pub async fn dashboard(State(state): State<Arc<AppState>>) -> AppResult<Markup> {
    let projects = projects::list_projects(&state.pool).await?;
    let counts: Vec<(Uuid, i64, i64)> = sqlx::query_as("SELECT project_id,count(*),count(*) FILTER (WHERE status='running') FROM maitu_tasks GROUP BY project_id")
        .fetch_all(&state.pool).await?;
    let counts: HashMap<_, _> = counts
        .into_iter()
        .map(|(id, total, running)| (id, (total, running)))
        .collect();
    let connections = state.providers.list().await;
    let usable = connections.iter().filter(|c| c.usable()).count();
    let capacity: usize = connections
        .iter()
        .filter(|c| c.usable())
        .map(|c| c.concurrency)
        .sum();
    let content = html! {
        header class="maitu-page-head" {
            div { h1 { "项目" } p { "把资料变成任务，独立推进。" } }
            a class="maitu-capacity" href="/maitu/settings" title="查看模型连接" {
                @if usable > 0 { (connections.len()) " 个连接 · 最多 " (capacity) " 项并行" } @else { "连接模型 API" }
            }
        }
        section class="maitu-intake" aria-labelledby="maitu-intake-title" {
            h2 id="maitu-intake-title" { "今天想推进什么？" }
            form id="maitu-project-create" {
                label class="maitu-sr-only" for="maitu-project-intent" { "项目目标" }
                textarea id="maitu-project-intent" name="intent" required maxlength="16000" rows="3" placeholder="一句话目标" {}
                button class="maitu-button maitu-button--primary" type="submit" { "创建项目" }
            }
        }
        section aria-labelledby="maitu-projects-title" {
            div class="maitu-section-head" { h2 id="maitu-projects-title" { "继续推进" } span { (projects.len()) " 个项目" } }
            @if projects.is_empty() {
                div class="maitu-empty" { strong { "第一个项目从上面的目标开始" } p { "创建后，可以在图上添加多个任务，并行调用自己的模型 API。" } }
            } @else {
                div class="maitu-project-grid" {
                    @for project in projects {
                        @let (total, running) = counts.get(&project.id).copied().unwrap_or_default();
                        a class="maitu-project-card" href=(format!("/maitu/projects/{}",project.id)) {
                            div class="maitu-card-top" {
                                @if running > 0 {
                                    span { i class="maitu-dot maitu-dot--running" aria-hidden="true" {} (running) " 个执行中" }
                                } @else if total > 0 {
                                    span { i class="maitu-dot" aria-hidden="true" {} (total) " 个任务" }
                                } @else {
                                    span { i class="maitu-dot" aria-hidden="true" {} "未添加任务" }
                                }
                            }
                            h3 { (&project.title) }
                            p { (&project.intent) }
                            div class="maitu-card-bottom" {
                                span { (project.updated_at.format("%Y-%m-%d")) }
                                span class="maitu-card-open" { "打开" }
                            }
                        }
                    }
                }
            }
        }
    };
    Ok(shell("项目", "dashboard", None, content))
}

pub async fn project_page(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let project = workflows::project(&state.pool, id).await?;
    let content = html! {
        header class="maitu-page-head maitu-page-head--project" {
            div { a class="maitu-back" href="/" { "← 全部项目" } h1 { (&project.title) } p { (&project.intent) } }
            div class="maitu-head-actions" {
                span id="maitu-capacity" class="maitu-capacity" { "读取执行状态…" }
                button id="maitu-generate-plan" type="button" class="maitu-button" { "生成计划" }
                button id="maitu-start-ready" type="button" class="maitu-button" { "启动待执行" }
                button id="maitu-new-task" type="button" class="maitu-button maitu-button--primary" { "+ 添加任务" }
            }
        }
        section id="maitu-attention" class="maitu-attention" aria-labelledby="maitu-attention-title" hidden {
            div class="maitu-section-head" { h2 id="maitu-attention-title" { "需要你处理" } span id="maitu-attention-count" {} }
            div id="maitu-attention-list" {}
        }
        section id="maitu-code-project" class="maitu-code-project" {
            div { strong { "项目代码" } p id="maitu-code-state" class="maitu-note" { "可导入代码副本，让节点真实修改并运行检查。" } }
            div class="maitu-head-actions" {
                button id="maitu-import-code" type="button" class="maitu-button maitu-button--small" { "导入代码文件夹" }
                a id="maitu-export-code" class="maitu-link-button" href=(format!("/api/maitu/projects/{id}/code/export")) hidden { "导出已采用代码" }
            }
        }
        div class="maitu-view-toggle" role="tablist" aria-label="项目视图" {
            button id="maitu-view-tasks" type="button" class="is-active" role="tab" aria-selected="true" aria-controls="maitu-workspace-tasks" { "任务图" }
            button id="maitu-view-goals" type="button" role="tab" aria-selected="false" aria-controls="maitu-workspace-goals" { "目标分支" }
        }
        div id="maitu-workspace-tasks" class="maitu-workspace" {
            section class="maitu-map-panel" aria-labelledby="maitu-map-title" {
                div class="maitu-section-head" { h2 id="maitu-map-title" { "任务图" } span id="maitu-project-status" { "加载中" } }
                div class="maitu-map-bar" {
                    div class="maitu-map-toolbar" {
                        button id="maitu-zoom-out" type="button" class="maitu-button maitu-button--small" aria-label="缩小" { "−" }
                        span id="maitu-zoom-level" { "100%" }
                        button id="maitu-zoom-in" type="button" class="maitu-button maitu-button--small" aria-label="放大" { "+" }
                        button id="maitu-zoom-reset" type="button" class="maitu-button maitu-button--small" { "重置" }
                        button id="maitu-focus-selected" type="button" class="maitu-button maitu-button--small" { "聚焦所选" }
                    }
                    div class="maitu-legend" { span { i class="maitu-dot maitu-dot--running" {} "执行中" } span { i class="maitu-dot maitu-dot--produced" {} "已产出" } span { i class="maitu-dot maitu-dot--queued" {} "等待执行" } span { i class="maitu-dot maitu-dot--failed" {} "需要处理" } }
                }
                p class="maitu-map-hint" { "左右滑动查看任务，点击节点查看记录与成果。" }
                div class="maitu-map-scroll" tabindex="0" aria-label="可横向滚动的项目任务图" {
                    div id="maitu-map" class="maitu-map" {}
                }
                section class="maitu-sources" {
                    div class="maitu-section-head" { h2 { "项目资料" } button id="maitu-add-source" type="button" class="maitu-button maitu-button--small" { "＋ 添加资料" } }
                    div id="maitu-source-list" {}
                }
            }
            aside id="maitu-detail" class="maitu-detail" aria-label="任务详情" {
                div class="maitu-empty" { strong { "选择一个任务" } p { "查看要求、历次尝试、输入来源和成果；也可以从节点上启动或重试。" } }
            }
        }
        section id="maitu-workspace-goals" class="maitu-goals" hidden aria-label="目标分支工作台" {
            div class="maitu-section-head" {
                h2 { "目标分支" }
                div class="maitu-head-actions" {
                    span id="maitu-goals-summary" { "读取中…" }
                    button id="maitu-goal-proposal-new" type="button" class="maitu-button" { "提出目标提案" }
                }
            }
            div id="maitu-goals-proposals" {}
            div class="maitu-goals-layout" {
                nav id="maitu-goals-branches" class="maitu-goals-branches" aria-label="目标分支列表" {}
                div id="maitu-goals-workspace" class="maitu-goals-workspace" {
                    div class="maitu-empty" { strong { "选择一个目标分支" } p { "查看契约、会话记录、证据与合入门禁；也可以从这里推进下一步。" } }
                }
            }
        }
        dialog id="maitu-task-dialog" class="maitu-dialog" {
            form id="maitu-task-create" {
                div class="maitu-section-head" { h2 { "添加任务" } button type="button" class="maitu-close" data-close-dialog="maitu-task-dialog" aria-label="关闭添加任务" { "×" } }
                label { "名称" input name="title" required maxlength="120" placeholder="做什么"; }
                label { "要求" textarea name="instruction" required maxlength="16000" rows="2" placeholder="读取什么、产出什么" {} }
                label { "成果文件名" input name="outputFilename" required maxlength="80" value="result.md"; }
                details class="maitu-advanced" {
                    summary { "高级" }
                    label { "任务类型" select name="taskKind" {
                        option value="file" { "读取资料并产出文件" }
                        option value="code" { "修改代码并运行检查" }
                    } }
                    label { "怎样算完成" textarea name="acceptanceCriteria" rows="2" maxlength="8000" placeholder="可选" {} }
                    fieldset { legend { "使用哪些资料" } div id="maitu-task-sources" class="maitu-checks" {} }
                    fieldset { legend { "依赖哪些前序任务" } div id="maitu-task-dependencies" class="maitu-checks" {} }
                    label { "使用连接" select name="connectionKey" id="maitu-task-connection" {
                        option value="" { "自动选择可用连接" }
                    } }
                }
                button type="submit" class="maitu-button maitu-button--primary" { "加入任务图" }
            }
        }
        dialog id="maitu-code-import-dialog" class="maitu-dialog" {
            form id="maitu-code-import-form" {
                div class="maitu-section-head" { h2 { "导入代码副本" } button type="button" class="maitu-close" data-close-dialog="maitu-code-import-dialog" aria-label="关闭代码导入" { "×" } }
                label { "代码文件夹" input id="maitu-code-files" type="file" webkitdirectory directory multiple required; }
                p id="maitu-code-import-count" class="maitu-note" { "" }
                details class="maitu-advanced" {
                    summary { "检查设置" }
                    label { "检查方式" select name="checkProgram" {
                        option value="node" { "Node" }
                        option value="python3" { "Python" }
                        option value="cargo" { "Rust" }
                    } }
                    label { "检查名称" input name="checkLabel" value="项目测试" required maxlength="120"; }
                    label { "检查参数（每行一个）" textarea name="checkArgs" rows="2" required { "--test" } }
                }
                button type="submit" class="maitu-button maitu-button--primary" { "保存代码基线" }
            }
        }
        dialog id="maitu-plan-generate-dialog" class="maitu-dialog" {
            form id="maitu-plan-generate-form" {
                div class="maitu-section-head" { h2 { "生成推进计划" } button type="button" class="maitu-close" data-close-dialog="maitu-plan-generate-dialog" aria-label="关闭计划要求" { "×" } }
                label { "补充要求（可选）" textarea name="instruction" rows="2" maxlength="8000" placeholder="不填则按项目目标生成" {} }
                details class="maitu-advanced" {
                    summary { "参考资料" }
                    div id="maitu-plan-sources" class="maitu-checks" {}
                }
                button type="submit" class="maitu-button maitu-button--primary" { "生成计划" }
            }
        }
        dialog id="maitu-plan-review-dialog" class="maitu-dialog maitu-dialog--wide" {
            form id="maitu-plan-review-form" {
                div class="maitu-section-head" { h2 { "调整推进计划" } button type="button" class="maitu-close" data-close-dialog="maitu-plan-review-dialog" aria-label="关闭计划编辑" { "×" } }
                label { "推进思路" textarea name="summary" rows="2" required maxlength="5000" {} }
                div id="maitu-plan-questions" {}
                div id="maitu-plan-edit-tasks" {}
                button type="submit" class="maitu-button maitu-button--primary" { "加入任务图" }
            }
        }
        dialog id="maitu-source-dialog" class="maitu-dialog" {
            div class="maitu-section-head" { h2 { "添加资料" } button type="button" class="maitu-close" data-close-dialog="maitu-source-dialog" aria-label="关闭添加资料" { "×" } }
            label { "上传文本文件" input id="maitu-source-files" type="file" multiple; }
            form id="maitu-source-create" {
                label { "或粘贴内容" input name="filename" required value="资料.txt" maxlength="80"; }
                textarea name="content" required rows="5" maxlength="250000" {}
                button type="submit" class="maitu-button maitu-button--primary" { "保存到项目" }
            }
        }
        dialog id="maitu-adopt-dialog" class="maitu-dialog" {
            form id="maitu-adopt-form" {
                div class="maitu-section-head" { h2 { "采用这次成果" } button type="button" class="maitu-close" data-close-dialog="maitu-adopt-dialog" aria-label="关闭采用" { "×" } }
                label { "理由（可选）" textarea name="reason" rows="2" maxlength="2000" placeholder="默认记录时间与版本" {} }
                button type="submit" class="maitu-button maitu-button--primary" { "确认采用" }
            }
        }
        dialog id="maitu-retry-dialog" class="maitu-dialog" {
            form id="maitu-retry-form" {
                div class="maitu-section-head" { h2 { "补充要求后再执行" } button type="button" class="maitu-close" data-close-dialog="maitu-retry-dialog" aria-label="关闭补充要求" { "×" } }
                label { "本次补充" textarea name="additionalInstruction" required maxlength="8000" rows="3" {} }
                button type="submit" class="maitu-button maitu-button--primary" { "开始新尝试" }
            }
        }
        dialog id="maitu-output-dialog" class="maitu-dialog maitu-dialog--wide" {
            div class="maitu-section-head" { h2 id="maitu-output-title" {} button type="button" class="maitu-close" data-close-dialog="maitu-output-dialog" aria-label="关闭内容" { "×" } }
            pre id="maitu-output-content" {}
        }
        dialog id="maitu-goal-dialog" class="maitu-dialog maitu-dialog--wide" {}
    };
    Ok(shell(&project.title, "project", Some(id), content))
}

pub async fn settings() -> Markup {
    let content = html! {
        header class="maitu-page-head" { div { h1 { "模型连接" } p { "独立任务按连接并行；一个连接限流时其他连接继续工作。" } } }
        section class="maitu-settings-panel" {
            div class="maitu-section-head" { h2 { "已保存的连接" } span id="maitu-connections-state" { "读取配置…" } }
            div id="maitu-connection-list" {}
        }
        section class="maitu-settings-panel" {
            form id="maitu-connection-form" {
                div class="maitu-section-head" { h2 id="maitu-connection-form-title" { "添加连接" } button type="button" id="maitu-connection-cancel" class="maitu-button maitu-button--small" hidden { "取消编辑" } }
                input type="hidden" name="originalKey" value="";
                div class="maitu-form-row" {
                    label { "连接编号" input name="key" required maxlength="64" pattern="[a-z][a-z0-9-]*" placeholder="例如 deepseek-main"; }
                    label { "连接名称" input name="label" required maxlength="80" placeholder="例如 DeepSeek 主力账户"; }
                }
                label { "API 地址" input name="baseUrl" type="url" required value="https://api.deepseek.com"; }
                datalist id="maitu-models" { option value="deepseek-flash"; option value="deepseek-v4-pro"; }
                div class="maitu-form-row" {
                    label { "模型名称" input name="model" required value="deepseek-flash" list="maitu-models"; }
                    label { "思考模式" select name="thinkingEnabled" {
                        option value="true" selected { "开启思考（默认）" }
                        option value="false" { "关闭思考" }
                    } }
                }
                label { "API 密钥" input name="apiKey" type="password" autocomplete="new-password" placeholder="首次配置时填写；编辑时留空保留当前密钥"; }
                p class="maitu-note" { "密钥只存本机，不进数据库备份。" }
                div class="maitu-settings-grid" {
                    label { "同时执行的任务数" input name="concurrency" type="number" min="1" max="64" value="3" required; }
                    label { "上下文预算（Token）" input name="contextTokens" type="number" min="1" max="1048576" value="1048576" required; }
                    label { "最大输出长度（Token）" input name="maxTokens" type="number" min="1" max="393216" value="65536" required; }
                }
                label class="maitu-check-line" { input type="checkbox" name="enabled" checked { } "启用此连接（停用后其排队任务等待其他连接）" }
                p id="maitu-model-limits" class="maitu-note" { "" }
                button type="submit" class="maitu-button maitu-button--primary" { "保存连接" }
                a class="maitu-text-link" href="/" { "返回项目" }
            }
        }
    };
    shell("模型连接", "settings", None, content)
}

pub async fn connections(State(state): State<Arc<AppState>>) -> Json<Value> {
    let list = state.providers.list().await;
    Json(json!(
        list.iter().map(ProviderConfig::view).collect::<Vec<_>>()
    ))
}

pub async fn save_connection(
    State(state): State<Arc<AppState>>,
    input: Result<Json<ProviderConfig>, JsonRejection>,
) -> AppResult<Json<Value>> {
    let Json(input) = input.map_err(|rejection| AppError::Operation {
        status: rejection.status(),
        code: "invalid_provider_input",
        message: match rejection.status().as_u16() {
            413 => "提交的配置过大，请检查填写内容后重试。",
            415 => "网页提交格式不正确，请刷新连接页后重新保存。",
            _ => "配置格式不正确。请刷新连接页并重新选择思考模式；执行数、上下文预算和输出长度须为整数。",
        }.into(),
    })?;
    Ok(Json(json!(state.providers.save(input).await?.view())))
}

pub async fn delete_connection(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> AppResult<Json<Value>> {
    state.providers.delete(&key).await?;
    Ok(Json(json!({"deleted": key})))
}

/// Legacy single-connection alias kept for older local scripts: reads the first
/// saved connection.
pub async fn provider_config(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!(state.providers.primary().await.view()))
}
pub async fn project_snapshot(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(
        workflows::snapshot(&state.pool, &state.providers, id).await?
    )))
}
pub async fn create_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateTaskRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(
        workflows::create_task(&state.pool, &state.providers, id, input).await?
    )))
}
pub async fn generate_plan(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<GeneratePlanRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(
        plans::generate(&state.pool, &state.providers, id, input).await?
    )))
}
pub async fn adopt_plan(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<AdoptPlanRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({"taskIds":plans::adopt(&state.pool, id, input).await?}),
    ))
}
pub async fn add_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<AddSourceRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(
        workflows::add_source(&state.pool, id, input).await?
    )))
}
pub async fn source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(workflows::source(&state.pool, id).await?)))
}
pub async fn task_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(workflows::detail(&state.pool, id).await?)))
}
pub async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(workflows::cancel(&state.pool, id).await?)))
}

pub async fn start_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<StartRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(
        workflows::start_with_instruction(
            &state.pool,
            id,
            input.request_id,
            &input.additional_instruction
        )
        .await?
    )))
}
pub async fn accept_output(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<AcceptRequest>,
) -> AppResult<Json<Value>> {
    let task = workflows::task(&state.pool, id).await?;
    let result = if task.task_kind == "code" {
        code::adopt(&state, id, input.attempt_id, &input.reason).await?
    } else {
        workflows::accept(&state.pool, id, input.attempt_id, &input.reason).await?
    };
    Ok(Json(json!(result)))
}

pub async fn import_code(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<code::ImportRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(code::import(&state, id, input).await?)))
}

pub async fn export_code(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/x-tar"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"maitu-project.tar\"",
            ),
        ],
        code::export(&state, id).await?,
    )
        .into_response())
}

pub async fn code_diff(
    State(state): State<Arc<AppState>>,
    Path((task, attempt)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        json!({"content":code::working_diff(&state,task,attempt).await?}),
    ))
}
