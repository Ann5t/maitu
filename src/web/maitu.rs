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
        provider::ProviderConfig,
        workflows::{self, AcceptRequest, AddSourceRequest, CreateTaskRequest, StartRequest},
    },
};

use super::AppState;

fn shell(title: &str, mode: &str, project_id: Option<Uuid>, content: Markup) -> Markup {
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
                script src="/assets/maitu.js?v=provider-save-2" defer {}
            }
            body data-maitu-mode=(mode) data-project-id=(project_id.map(|id| id.to_string()).unwrap_or_default()) {
                div class="maitu-shell" {
                    aside class="maitu-nav" {
                        a class="maitu-brand" href="/" { span class="maitu-brand-mark" aria-hidden="true" { "脉" } strong { "脉图" } }
                        p class="maitu-nav-caption" { "让工作从一个节点继续" }
                        nav aria-label="主要导航" {
                            a href="/" class=(if mode == "dashboard" || mode == "project" { "is-active" } else { "" }) { "◎ 项目" }
                            a href="/maitu/settings" class=(if mode == "settings" { "is-active" } else { "" }) { "⚙ 模型连接" }
                        }
                        div class="maitu-nav-bottom" {
                            p { "资料 → 任务 → 成果" }
                            button type="button" data-theme-set="light" aria-label="浅色主题" { "浅色" }
                            button type="button" data-theme-set="dark" aria-label="深色主题" { "深色" }
                            a href="/legacy" { "早期工作台" }
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
    let provider = state.providers.get().await.view();
    let content = html! {
        header class="maitu-page-head" {
            div { p class="maitu-eyebrow" { "你的个人工作区" } h1 { "项目" } p { "把资料变成任务，让独立的工作同时推进。" } }
            a class="maitu-link-button" href="/maitu/settings" {
                @if provider.configured { "DeepSeek 已配置 · 最多 " (provider.concurrency) " 项并行" } @else { "连接 DeepSeek" }
            }
        }
        section class="maitu-intake" aria-labelledby="maitu-intake-title" {
            div { h2 id="maitu-intake-title" { "今天想推进什么？" } p { "先给项目一个目标，再添加资料和具体任务。" } }
            form id="maitu-project-create" {
                label class="maitu-sr-only" for="maitu-project-intent" { "项目目标" }
                textarea id="maitu-project-intent" name="intent" required maxlength="16000" rows="3" placeholder="例如：整理我的旧项目，找出最值得继续推进的一条路线……" {}
                button class="maitu-button maitu-button--primary" type="submit" { "创建项目 →" }
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
                            div class="maitu-card-top" { span class="maitu-project-symbol" { "◎" } span { @if running > 0 { (running) " 个任务执行中" } @else { (total) " 个任务" } } }
                            h3 { (&project.title) }
                            p { (&project.intent) }
                            div class="maitu-card-bottom" { span { (project.updated_at.format("%Y-%m-%d")) } strong { "打开任务图 ↗" } }
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
                button id="maitu-start-ready" type="button" class="maitu-button" { "启动待执行任务" }
                button id="maitu-new-task" type="button" class="maitu-button maitu-button--primary" { "+ 添加任务" }
            }
        }
        div class="maitu-workspace" {
            section class="maitu-map-panel" aria-labelledby="maitu-map-title" {
                div class="maitu-section-head" { h2 id="maitu-map-title" { "任务图" } span id="maitu-project-status" { "加载中" } }
                div class="maitu-legend" { span { i class="maitu-dot maitu-dot--running" {} "执行中" } span { i class="maitu-dot maitu-dot--produced" {} "已产出" } span { i class="maitu-dot maitu-dot--failed" {} "需要处理" } }
                p class="maitu-map-hint" { "左右滑动查看任务，点击节点查看记录与成果。" }
                div class="maitu-map-scroll" tabindex="0" aria-label="可横向滚动的项目任务图" {
                    div id="maitu-map" class="maitu-map" {}
                }
                section class="maitu-sources" {
                    div class="maitu-section-head" { h2 { "项目资料" } button id="maitu-add-source" type="button" class="maitu-button maitu-button--small" { "+ 添加资料" } }
                    div id="maitu-source-list" {}
                    p class="maitu-note" { "支持 UTF-8 文本文件。每次启动固定当时的资料，之后添加的内容留给下次执行。" }
                }
            }
            aside id="maitu-detail" class="maitu-detail" aria-label="任务详情" {
                div class="maitu-empty" { strong { "选择一个任务" } p { "查看要求、历次尝试、输入来源和成果；也可以从节点上启动或重试。" } }
            }
        }
        dialog id="maitu-task-dialog" class="maitu-dialog" {
            form id="maitu-task-create" {
                div class="maitu-section-head" { h2 { "添加任务" } button type="button" class="maitu-close" data-close-dialog="maitu-task-dialog" aria-label="关闭添加任务" { "×" } }
                label { "任务名称" input name="title" required maxlength="120" placeholder="例如：整理需求、检查风险、拟定推进计划"; }
                label { "具体要求" textarea name="instruction" required maxlength="16000" rows="4" placeholder="说明希望它读取什么、分析什么，以及文件中应包含什么。" {} }
                label { "成果文件名" input name="outputFilename" required maxlength="80" value="result.md"; }
                fieldset { legend { "使用哪些资料" } p class="maitu-note" { "未选择时，使用启动时项目的全部资料。" } div id="maitu-task-sources" class="maitu-checks" {} }
                fieldset { legend { "依赖哪些前序任务" } p class="maitu-note" { "等待前序成果被采用后执行，引用的成果版本会保留在记录中。" } div id="maitu-task-dependencies" class="maitu-checks" {} }
                button type="submit" class="maitu-button maitu-button--primary" { "加入任务图" }
            }
        }
        dialog id="maitu-source-dialog" class="maitu-dialog" {
            div class="maitu-section-head" { h2 { "添加资料" } button type="button" class="maitu-close" data-close-dialog="maitu-source-dialog" aria-label="关闭添加资料" { "×" } }
            label { "上传文本文件" input id="maitu-source-files" type="file" multiple; }
            p class="maitu-note" { "单个文件最多 256 KiB。也可以直接粘贴资料：" }
            form id="maitu-source-create" {
                label { "资料文件名" input name="filename" required value="资料.txt" maxlength="80"; }
                label { "资料内容" textarea name="content" required rows="7" maxlength="250000" {} }
                button type="submit" class="maitu-button maitu-button--primary" { "保存到项目" }
            }
        }
        dialog id="maitu-output-dialog" class="maitu-dialog maitu-dialog--wide" {
            div class="maitu-section-head" { h2 id="maitu-output-title" {} button type="button" class="maitu-close" data-close-dialog="maitu-output-dialog" aria-label="关闭内容" { "×" } }
            pre id="maitu-output-content" {}
        }
    };
    Ok(shell(&project.title, "project", Some(id), content))
}

pub async fn settings() -> Markup {
    let content = html! {
        header class="maitu-page-head" { div { p class="maitu-eyebrow" { "执行资源" } h1 { "模型连接" } p { "先连接 DeepSeek，再按可用额度设置并发。" } } }
        section class="maitu-settings-panel" {
            form id="maitu-provider-form" {
                div class="maitu-section-head" { h2 { "DeepSeek" } span id="maitu-provider-state" { "读取配置…" } }
                label { "API 地址" input name="baseUrl" type="url" required value="https://api.deepseek.com"; }
                label { "模型名称" input name="model" required value="deepseek-flash" list="maitu-models"; }
                datalist id="maitu-models" { option value="deepseek-flash"; option value="deepseek-v4-pro"; }
                label { "思考模式" select name="thinkingEnabled" {
                    option value="true" selected { "开启思考（默认）" }
                    option value="false" { "关闭思考" }
                } }
                label { "API 密钥" input name="apiKey" type="password" autocomplete="new-password" placeholder="首次配置时填写；留空可保留当前密钥"; }
                p class="maitu-note" { "密钥保存在本机专用配置卷，不显示在任务记录中。" }
                div class="maitu-settings-grid" {
                    label { "同时执行的任务数" input name="concurrency" type="number" min="1" max="64" value="3" required; }
                    label { "上下文预算（Token）" input name="contextTokens" type="number" min="1" max="1048576" value="1048576" required; }
                    label { "最大输出长度（Token）" input name="maxTokens" type="number" min="1" max="393216" value="65536" required; }
                }
                p id="maitu-model-limits" class="maitu-note" { "DeepSeek：上下文容量 1,048,576 Token，最大输出 393,216 Token。" }
                p class="maitu-note" { "上下文预算计入任务要求、资料与预留输出。输入按文本估算，超出预算会提示调整；实际 Token 用量以模型返回为准。输出上限实际传给模型，不代表每次都会写满。" }
                p class="maitu-note" {
                    span id="maitu-generation-defaults" { "思考模式：参考默认输出上限 65,536 Token。此额度包含思考与最终正文。" }
                    a href="https://api-docs.deepseek.com/zh-cn/quick_start/pricing/" target="_blank" rel="noopener noreferrer" { "查看官方模型说明" }
                }
                p class="maitu-note" { "保存后用于新启动的请求。连接是否可用，以实际任务的执行结果为准；限流时可降低并发。" }
                button type="submit" class="maitu-button maitu-button--primary" { "保存连接" }
                a class="maitu-text-link" href="/" { "返回项目" }
            }
        }
    };
    shell("模型连接", "settings", None, content)
}

pub async fn provider_config(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!(state.providers.get().await.view()))
}
pub async fn save_provider(
    State(state): State<Arc<AppState>>,
    input: Result<Json<ProviderConfig>, JsonRejection>,
) -> AppResult<Json<Value>> {
    let Json(input) = input.map_err(|rejection| AppError::Operation {
        status: rejection.status(),
        code: "invalid_provider_input",
        message: match rejection.status().as_u16() {
            413 => "提交的配置过大，请检查填写内容后重试。",
            415 => "网页提交格式不正确，请刷新模型连接页后重新保存。",
            _ => "配置格式不正确。请刷新连接页并重新选择思考模式；执行数、上下文预算和输出长度须为整数。",
        }.into(),
    })?;
    Ok(Json(json!(state.providers.save(input).await?)))
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
        workflows::create_task(&state.pool, id, input).await?
    )))
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
pub async fn start_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<StartRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(
        workflows::start(&state.pool, id, input.request_id).await?
    )))
}
pub async fn accept_output(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(input): Json<AcceptRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(json!(
        workflows::accept(&state.pool, id, input.attempt_id).await?
    )))
}
