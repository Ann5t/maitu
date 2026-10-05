//! 想法空间页面：列表、创建与想法详情（修订 / 关联 / 立项提案）。
//! 数据全部由前端从 /api/v1/ideas 读取，本模块只渲染外壳与空态。

use axum::extract::Path;
use maud::{Markup, html};
use uuid::Uuid;

use super::maitu::shell;

pub async fn ideas_page() -> Markup {
    let content = html! {
        header class="maitu-page-head" {
            div { h1 { "想法" } p { "记下尚未成形的方向，准备好时把其中一条变成项目。" } }
            button id="maitu-idea-new" type="button" class="maitu-button maitu-button--primary" { "+ 记录想法" }
        }
        section aria-labelledby="maitu-ideas-title" {
            div class="maitu-section-head" { h2 id="maitu-ideas-title" { "全部想法" } span id="maitu-ideas-count" { "读取中…" } }
            div id="maitu-idea-list" class="maitu-idea-grid" {}
        }
        dialog id="maitu-idea-create-dialog" class="maitu-dialog" {
            form id="maitu-idea-create" {
                div class="maitu-section-head" { h2 { "记录想法" } button type="button" class="maitu-close" data-close-dialog="maitu-idea-create-dialog" aria-label="关闭记录想法" { "×" } }
                label { "标题" input name="title" required maxlength="200" placeholder="一句话说清这个想法"; }
                label { "内容" textarea name="body" required rows="5" maxlength="20000" placeholder="背景、动机、可能的做法……想到什么写什么。" {} }
                div class="maitu-form-row" {
                    label { "来源类型" select name="sourceKind" { option value="text" { "随手记录" } option value="link" { "链接" } } }
                    label { "来源地址（可选）" input name="sourceRef" maxlength="500" placeholder="https://…"; }
                }
                button type="submit" class="maitu-button maitu-button--primary" { "保存想法" }
            }
        }
    };
    shell("想法", "ideas", None, content)
}

pub async fn idea_page(Path(idea_id): Path<Uuid>) -> Markup {
    let content = html! {
        header class="maitu-page-head maitu-page-head--idea" {
            div {
                a class="maitu-back" href="/maitu/ideas" { "← 全部想法" }
                h1 id="maitu-idea-title" { "读取中…" }
                p id="maitu-idea-meta" class="maitu-idea-meta" {}
            }
            div id="maitu-idea-actions" class="maitu-head-actions" {}
        }
        div class="maitu-idea-layout" {
            section class="maitu-panel" aria-labelledby="maitu-idea-body-title" {
                div class="maitu-section-head" { h2 id="maitu-idea-body-title" { "当前内容" } span id="maitu-idea-state" {} }
                article id="maitu-idea-body" class="maitu-idea-body" {}
                div id="maitu-idea-sources" {}
            }
            section class="maitu-panel" aria-labelledby="maitu-idea-history-title" {
                div class="maitu-section-head" { h2 id="maitu-idea-history-title" { "修订与关联" } span id="maitu-idea-history-count" {} }
                div id="maitu-idea-history" {}
            }
        }
        dialog id="maitu-idea-dialog" class="maitu-dialog" {}
    };
    shell("想法", "idea", Some(idea_id), content)
}
