use std::collections::{HashMap, HashSet};

use maud::{DOCTYPE, Markup, html};
use uuid::Uuid;

use crate::{
    application::workbench::WorkbenchActivity,
    domain::{
        branch_status_label, can_append_to_branch, contribution_kind_label, node_kind_label,
        outcome_label, state_label,
    },
    goal_models::{
        GoalContractVersionRecord, GoalGraphSnapshot, GoalProposalRecord,
        GoalProposalRevisionRecord, GoalReviewGateRecord, GoalSessionRecord,
    },
    idea_models::{
        IdeaLinkView, IdeaRevisionRecord, IdeaSnapshot, IdeaSummary, ProjectProposalRecord,
        ProjectProposalRevisionRecord,
    },
    models::{
        ActionRun, ProjectBranch, ProjectContribution, ProjectNode, ProjectSnapshot, ProjectSummary,
    },
};

use super::{
    goal_projection::{GoalGraphProjection, GoalLane, PROJECTION_VERSION},
    handlers::{IdeaPageQuery, ProjectPageQuery},
};

pub fn dashboard(projects: &[ProjectSummary], requested_view: Option<&str>) -> Markup {
    let view = match requested_view {
        Some("attention") => "attention",
        Some("artifacts") => "artifacts",
        _ => "projects",
    };
    let visible_projects = projects
        .iter()
        .filter(|project| match view {
            "attention" => project.attention_count > 0,
            "artifacts" => project.artifact_count > 0,
            _ => true,
        })
        .collect::<Vec<_>>();
    let (title, subtitle, section_title, empty_title, empty_copy) = match view {
        "attention" => (
            "待处理",
            "只显示需要确认、正在执行或受阻的项目。",
            "需要你或系统继续处理",
            "现在没有悬而未决的动作",
            "新的确认、执行或阻塞事项出现时，会集中显示在这里。",
        ),
        "artifacts" => (
            "产物",
            "从留下正式文件的项目继续工作。",
            "已有正式产物的项目",
            "还没有项目产物",
            "确认成果契约并生成第一份项目说明后，产物会出现在这里。",
        ),
        _ => (
            "项目",
            "持续保存目标、现实行动、产物与证据。",
            "最近项目",
            "从一句自然语言开始",
            "不必先选分类。说清楚你现在想推进什么，浮点会先帮你形成可修订的成果契约。",
        ),
    };
    let active_count = projects
        .iter()
        .filter(|project| matches!(project.state.as_str(), "shaping" | "active" | "waiting"))
        .count();
    let content = html! {
        header class="page-head" {
            div {
                span class="eyebrow" { "WORKSPACE" }
                h1 { (title) }
                p { (subtitle) }
            }
            a class="button button--primary" href="/ideas/new" { span aria-hidden="true" { "+" } " 记录想法" }
        }

        section class="dashboard-strip" aria-label="工作区概览" {
            div class="dashboard-strip__primary" {
                span class="pulse-mark" aria-hidden="true" {}
                div {
                    strong { (active_count) " 个项目正在推进" }
                    small { "下一步优先产生现实变化，而不是继续堆积建议。" }
                }
            }
            div class="dashboard-stat" { strong { (projects.len()) } span { "全部项目" } }
            div class="dashboard-stat" {
                strong { (projects.iter().filter(|item| item.state == "waiting").count()) }
                span { "等待反馈" }
            }
        }

        section class="section-block" {
            div class="section-heading" {
                div { span class="eyebrow" { "RECENT" } h2 { (section_title) } }
                span class="section-count" { (visible_projects.len()) }
            }
            @if visible_projects.is_empty() {
                div class="empty-state" {
                    span class="empty-state__icon" { "✦" }
                    h3 { (empty_title) }
                    p { (empty_copy) }
                    a class="button button--primary" href="/ideas/new" { "从想法开始" }
                }
            } @else {
                div class="project-grid" {
                    @for project in visible_projects {
                        (project_card(project, view))
                    }
                }
            }
        }
    };
    layout(title, view, content)
}

pub fn ideas(ideas: &[IdeaSummary], links: &[IdeaLinkView], query: &IdeaPageQuery) -> Markup {
    let view = if query.view.as_deref() == Some("map") {
        "map"
    } else {
        "stream"
    };
    let content = html! {
        header class="page-head idea-space-head" {
            div {
                span class="eyebrow" { "UPSTREAM SPACE" }
                h1 { "想法" }
                p { "先保留未经整理的念头；真正立项前再形成可审查的 ProjectProposal。" }
            }
            a class="button button--primary" href="/ideas/new" { span aria-hidden="true" { "+" } " 记录想法" }
        }
        @if let Some(message) = query.notice.as_deref() {
            div class="flash flash--success" role="status" { (message) }
        }
        @if let Some(message) = query.error.as_deref() {
            div class="flash flash--error" role="alert" { (message) }
        }
        section class="idea-projection-bar" aria-label="想法投影" {
            div {
                strong { (ideas.len()) " 个想法" }
                span { (links.len()) " 条关系；投影可以切换，内容与来源不会变。" }
            }
            nav aria-label="切换想法视图" {
                a class=(if view == "stream" { "is-active" } else { "" }) href="/ideas?view=stream" { "时间流" }
                a class=(if view == "map" { "is-active" } else { "" }) href="/ideas?view=map" { "关系图" }
            }
        }
        @if ideas.is_empty() {
            div class="empty-state idea-empty" {
                span class="empty-state__icon" { "∿" }
                h3 { "先记下一句话" }
                p { "它不必已经像项目，也不要求你填一张巨大的表。" }
                a class="button button--primary" href="/ideas/new" { "记录第一个想法" }
            }
        } @else if view == "map" {
            (idea_map(ideas, links))
        } @else {
            section class="idea-stream" aria-label="想法时间流" {
                @for idea in ideas {
                    (idea_card(idea, false))
                }
            }
        }
    };
    layout("想法", "ideas", content)
}

fn idea_map(ideas: &[IdeaSummary], links: &[IdeaLinkView]) -> Markup {
    html! {
        section class="idea-map" aria-label="想法关系投影" {
            div class="idea-map__nodes" {
                @for (index, idea) in ideas.iter().enumerate() {
                    article class=(format!("idea-map-node idea-map-node--{}", index % 4)) {
                        (idea_card(idea, true))
                    }
                }
            }
            aside class="idea-map__relations" {
                div class="section-heading" { div { span class="eyebrow" { "RELATIONS" } h2 { "关系索引" } } span class="section-count" { (links.len()) } }
                @if links.is_empty() {
                    p class="muted-copy" { "还没有关联；打开任一想法即可补充支持、矛盾或依赖关系。" }
                } @else {
                    ol {
                        @for link in links {
                            li {
                                a href=(format!("/ideas/{}", link.source_idea_id)) { (&link.source_title) }
                                span { (idea_relation_label(&link.relation)) }
                                a href=(format!("/ideas/{}", link.target_idea_id)) { (&link.target_title) }
                                small { (&link.rationale) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn idea_card(idea: &IdeaSummary, compact: bool) -> Markup {
    html! {
        a class=(if compact { "idea-card idea-card--compact" } else { "idea-card" })
            href=(format!("/ideas/{}", idea.id)) {
            header {
                span class=(format!("idea-state idea-state--{}", idea_state_tone(&idea.state))) {
                    (idea_state_label(&idea.state))
                }
                span { "v" (idea.current_revision) }
                time { (format_date(idea.updated_at)) }
            }
            h2 { (&idea.title) }
            p { (&idea.body) }
            footer {
                span { (idea.link_count) " 关系" }
                span { (idea.proposal_count) " 提案" }
                @if idea.promoted_project_id.is_some() { b { "已形成项目 →" } } @else { b { "继续发展 →" } }
            }
        }
    }
}

pub fn new_idea(error: Option<&str>) -> Markup {
    let content = html! {
        div class="focused-page" {
            a class="back-link" href="/ideas" { "← 返回想法" }
            section class="intake-card idea-intake-card" {
                div class="intake-card__mark" { "∞" }
                span class="eyebrow" { "CAPTURE AN IDEA" }
                h1 { "先把脑中的东西留下来" }
                p { "一句话就能开始。标题可留空，系统会从第一句话生成；以后每次变化都会保留版本。" }
                @if let Some(message) = error {
                    div class="flash flash--error" role="alert" { (message) }
                }
                form class="intake-form" method="post" action="/ideas/commands" data-idea-capture="" {
                    input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                    input type="hidden" name="action" value="idea.create";
                    input type="hidden" name="source_kind" value="text";
                    label for="idea-title" { "标题（可留空）" }
                    input id="idea-title" name="title" maxlength="240" placeholder="系统会从第一句话生成";
                    label for="idea-body" { "现在想到什么？" }
                    textarea id="idea-body" name="body" rows="8" autofocus
                        placeholder="例如：如果每个科研目标都能展开成可审查的独立枝干，我就能知道是技术不可行，还是还有遗漏。" {}
                    label for="idea-file" { "也可以直接从文件、图片或语音开始（可选）" }
                    input id="idea-file" name="file" type="file"
                        accept="image/*,audio/*,.pdf,.txt,.md,.doc,.docx,.ppt,.pptx,.xls,.xlsx";
                    div class="intake-hints" {
                        span { "✦ 不要求立即立项" }
                        span { "✦ 未知可以一直保留" }
                        span { "✦ 文件、图片和语音可直接开始" }
                    }
                    button class="button button--primary button--large" type="submit" { "保留这个想法" span aria-hidden="true" { "→" } }
                }
            }
        }
    };
    layout("记录想法", "ideas", content)
}

pub fn idea(snapshot: &IdeaSnapshot, all_ideas: &[IdeaSummary], query: &IdeaPageQuery) -> Markup {
    let current = snapshot
        .revisions
        .iter()
        .find(|revision| revision.revision == snapshot.idea.current_revision)
        .expect("Idea current revision is protected by a database constraint");
    let content = html! {
        header class="idea-detail-head" {
            a class="back-link" href="/ideas" { "← 想法" }
            div {
                span class=(format!("idea-state idea-state--{}", idea_state_tone(&snapshot.idea.state))) { (idea_state_label(&snapshot.idea.state)) }
                span { "版本 " (snapshot.idea.current_revision) }
                span { (source_kind_label(&current.source_kind)) }
            }
            h1 { (&current.title) }
            p { (&current.body) }
            @if let Some(project_id) = snapshot.proposals.iter().find_map(|proposal| proposal.approved_project_id) {
                a class="button button--primary" href=(format!("/projects/{project_id}")) { "打开已形成的项目 →" }
            }
        }
        @if let Some(message) = query.notice.as_deref() {
            div class="flash flash--success" role="status" { (message) }
        }
        @if let Some(message) = query.error.as_deref() {
            div class="flash flash--error" role="alert" { (message) }
        }
        div class="idea-detail-layout" {
            main {
                section class="idea-detail-section" {
                    div class="section-heading" { div { span class="eyebrow" { "SOURCE MATERIALS" } h2 { "文件、图片与语音来源" } } span class="section-count" { (snapshot.sources.len()) } }
                    @if !snapshot.sources.is_empty() {
                        div class="idea-source-grid" {
                            @for source in &snapshot.sources {
                                article class=(format!("idea-source-card idea-source-card--{}", source.kind)) {
                                    @if source.kind == "image" {
                                        img src=(format!("/api/v1/ideas/{}/sources/{}/content", snapshot.idea.id, source.id))
                                            alt=(source.display_name.as_str()) loading="lazy";
                                    } @else if source.kind == "audio" {
                                        div class="idea-source-card__audio" { span aria-hidden="true" { "◖))" } audio controls preload="metadata"
                                            src=(format!("/api/v1/ideas/{}/sources/{}/content", snapshot.idea.id, source.id)) {} }
                                    } @else {
                                        div class="idea-source-card__file" aria-hidden="true" { "FILE" }
                                    }
                                    div {
                                        strong { (&source.display_name) }
                                        small { (&source.trusted_media_type) " · " (source.size_bytes) " B" }
                                        @if !source.note.is_empty() { p { (&source.note) } }
                                        a href=(format!("/api/v1/ideas/{}/sources/{}/content", snapshot.idea.id, source.id)) target="_blank" { "打开原文件 →" }
                                    }
                                }
                            }
                        }
                    }
                    form class="idea-source-upload" data-idea-source-upload=""
                        data-idea-id=(snapshot.idea.id) data-idea-revision=(snapshot.idea.current_revision) {
                        label { "追加来源"
                            input name="file" type="file" required
                                accept="image/*,audio/*,.pdf,.txt,.md,.doc,.docx,.ppt,.pptx,.xls,.xlsx";
                        }
                        label { "这份材料说明什么？（可选）" input name="note" maxlength="4000"; }
                        button class="button button--secondary" type="submit" { "验证并附加" }
                        output data-idea-source-status="" aria-live="polite" {}
                    }
                }
                section class="idea-detail-section" {
                    div class="section-heading" { div { span class="eyebrow" { "PROJECT PROPOSALS" } h2 { "从想法形成项目" } } span class="section-count" { (snapshot.proposals.len()) } }
                    @if snapshot.proposals.is_empty() {
                        article class="idea-explainer" {
                            strong { "立项不是复制粘贴" }
                            p { "先明确现在为什么值得做、第一条根目标如何验证，以及哪些想法内容这次暂不采用。提交后仍需你批准，批准才会原子创建项目和根 BranchProposal 草案。" }
                        }
                        (project_proposal_form(snapshot.idea.id, current, all_ideas, &[], None))
                    } @else {
                        @for proposal in &snapshot.proposals {
                            (project_proposal_card(snapshot, current, all_ideas, proposal))
                        }
                    }
                }
                section class="idea-detail-section" {
                    div class="section-heading" { div { span class="eyebrow" { "RELATIONSHIPS" } h2 { "与其他想法的关系" } } span class="section-count" { (snapshot.links.len()) } }
                    @for link in &snapshot.links {
                        article class="idea-link-card" {
                            span { (idea_relation_label(&link.relation)) }
                            @if link.source_idea_id == snapshot.idea.id {
                                a href=(format!("/ideas/{}", link.target_idea_id)) { (&link.target_title) }
                            } @else {
                                a href=(format!("/ideas/{}", link.source_idea_id)) { (&link.source_title) }
                            }
                            p { (&link.rationale) }
                        }
                    }
                    @if all_ideas.iter().any(|idea| idea.id != snapshot.idea.id) {
                        form class="idea-inline-form" method="post" action="/ideas/commands" {
                            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                            input type="hidden" name="subject_id" value=(snapshot.idea.id);
                            input type="hidden" name="return_idea_id" value=(snapshot.idea.id);
                            input type="hidden" name="action" value="idea.link";
                            input type="hidden" name="expected_revision" value=(snapshot.idea.current_revision);
                            label { "关联到"
                                select name="target_idea_ref" required {
                                    option value="" { "选择另一个想法" }
                                    @for idea in all_ideas.iter().filter(|idea| idea.id != snapshot.idea.id) {
                                        option value=(format!("{}@{}", idea.id, idea.current_revision)) { (&idea.title) " · v" (idea.current_revision) }
                                    }
                                }
                            }
                            label { "关系"
                                select name="relation" required {
                                    option value="related" { "有关联" }
                                    option value="supports" { "支持" }
                                    option value="contradicts" { "矛盾" }
                                    option value="depends_on" { "依赖" }
                                    option value="duplicates" { "重复" }
                                }
                            }
                            label class="idea-inline-form__wide" { "为什么这样关联？" input name="rationale" required maxlength="4000"; }
                            button class="button button--secondary" type="submit" { "保存关系" }
                        }
                    }
                }
            }
            aside class="idea-history-panel" {
                details open {
                    summary { "继续发展这个想法" }
                    form class="idea-revision-form" method="post" action="/ideas/commands" {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="subject_id" value=(snapshot.idea.id);
                        input type="hidden" name="return_idea_id" value=(snapshot.idea.id);
                        input type="hidden" name="action" value="idea.revise";
                        input type="hidden" name="expected_revision" value=(snapshot.idea.current_revision);
                        input type="hidden" name="source_kind" value=(current.source_kind.as_str());
                        @if let Some(source_ref) = &current.source_ref { input type="hidden" name="source_ref" value=(source_ref); }
                        label { "标题" input name="title" required value=(&current.title); }
                        label { "内容" textarea name="body" rows="6" required { (&current.body) } }
                        label { "这次为什么改变？" textarea name="revision_reason" rows="2" required {} }
                        button class="button button--primary" type="submit" { "保存为 v" (snapshot.idea.current_revision + 1) }
                    }
                }
                div class="section-heading" { div { span class="eyebrow" { "VERSIONS" } h2 { "不可覆盖的版本" } } span class="section-count" { (snapshot.revisions.len()) } }
                ol class="idea-version-list" {
                    @for revision in &snapshot.revisions {
                        li {
                            strong { "v" (revision.revision) " · " (&revision.title) }
                            time { (format_date_time(revision.created_at)) }
                            @if let Some(reason) = &revision.revision_reason { p { (reason) } }
                        }
                    }
                }
            }
        }
    };
    layout(&current.title, "ideas", content)
}

fn project_proposal_card(
    snapshot: &IdeaSnapshot,
    current_idea: &IdeaRevisionRecord,
    all_ideas: &[IdeaSummary],
    proposal: &ProjectProposalRecord,
) -> Markup {
    let revision = snapshot
        .proposal_revisions
        .iter()
        .find(|revision| {
            revision.proposal_id == proposal.id && revision.revision == proposal.current_revision
        })
        .expect("ProjectProposal current revision is protected by a database constraint");
    let desired_outcome = revision
        .root_goal
        .0
        .get("contract")
        .and_then(|value| value.get("desiredOutcome"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("根目标内容缺失");
    let selected_source_ids = snapshot
        .proposal_sources
        .iter()
        .filter(|source| {
            source.proposal_id == proposal.id
                && source.proposal_revision == proposal.current_revision
                && source.idea_id != snapshot.idea.id
        })
        .map(|source| source.idea_id)
        .collect::<Vec<_>>();
    html! {
        article class="project-proposal-card" data-project-proposal-id=(proposal.id) {
            header {
                span class=(format!("proposal-state proposal-state--{}", proposal_state_tone(&proposal.status))) { (proposal_state_label(&proposal.status)) }
                span { "ProjectProposal v" (proposal.current_revision) }
            }
            h3 { (&revision.title) }
            p { (&revision.why_now) }
            div class="project-proposal-card__goal" {
                span { "拟定根目标" }
                strong { (desired_outcome) }
            }
            @if !revision.omitted_notes.0.is_empty() {
                details {
                    summary { "这次暂不采用的内容（" (revision.omitted_notes.0.len()) "）" }
                    ul { @for note in &revision.omitted_notes.0 { li { (note) } } }
                }
            }
            div class="project-proposal-card__actions" {
                @if proposal.status == "draft" {
                    form method="post" action="/ideas/commands" {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="subject_id" value=(proposal.id);
                        input type="hidden" name="return_idea_id" value=(snapshot.idea.id);
                        input type="hidden" name="action" value="project_proposal.submit";
                        input type="hidden" name="expected_revision" value=(proposal.current_revision);
                        button class="button button--primary" type="submit" { "提交立项审核" }
                    }
                    details class="project-proposal-revise" {
                        summary { "继续修订提案" }
                        (project_proposal_form(snapshot.idea.id, current_idea, all_ideas, &selected_source_ids, Some((proposal, revision))))
                    }
                } @else if proposal.status == "awaiting_approval" {
                    form method="post" action="/ideas/commands" {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="subject_id" value=(proposal.id);
                        input type="hidden" name="return_idea_id" value=(snapshot.idea.id);
                        input type="hidden" name="action" value="project_proposal.approve";
                        input type="hidden" name="expected_revision" value=(proposal.current_revision);
                        button class="button button--primary" type="submit" { "批准并创建项目" }
                    }
                    details class="project-proposal-revise" {
                        summary { "修改后再审" }
                        (project_proposal_form(snapshot.idea.id, current_idea, all_ideas, &selected_source_ids, Some((proposal, revision))))
                    }
                    details class="proposal-reject" {
                        summary { "退回这项提案" }
                        form method="post" action="/ideas/commands" {
                            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                            input type="hidden" name="subject_id" value=(proposal.id);
                            input type="hidden" name="return_idea_id" value=(snapshot.idea.id);
                            input type="hidden" name="action" value="project_proposal.reject";
                            label { "退回理由" textarea name="rationale" rows="2" required {} }
                            button class="button button--secondary" type="submit" { "确认退回" }
                        }
                    }
                } @else if proposal.status == "approved" {
                    @if let Some(project_id) = proposal.approved_project_id {
                        a class="button button--primary" href=(format!("/projects/{project_id}")) { "进入项目空间 →" }
                    }
                } @else {
                    p class="muted-copy" { (proposal.decision_rationale.as_deref().unwrap_or("该提案已经收尾，想法本身仍被保留。")) }
                }
            }
        }
    }
}

fn project_proposal_form(
    idea_id: Uuid,
    idea_revision: &IdeaRevisionRecord,
    all_ideas: &[IdeaSummary],
    selected_source_ids: &[Uuid],
    existing: Option<(&ProjectProposalRecord, &ProjectProposalRevisionRecord)>,
) -> Markup {
    let proposal = existing.map(|item| item.0);
    let revision = existing.map(|item| item.1);
    let root = revision.map(|item| &item.root_goal.0);
    let contract = root.and_then(|value| value.get("contract"));
    let root_lines = |key: &str| {
        root.and_then(|value| value.get(key))
            .map(json_value_lines)
            .unwrap_or_default()
    };
    let contract_lines = |key: &str| {
        contract
            .and_then(|value| value.get(key))
            .map(json_value_lines)
            .unwrap_or_default()
    };
    let desired_outcome = contract
        .and_then(|value| value.get("desiredOutcome"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&idea_revision.body);
    html! {
        form class="project-proposal-form" method="post" action="/ideas/commands" {
            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
            input type="hidden" name="subject_id" value=(proposal.map(|item| item.id).unwrap_or(idea_id));
            input type="hidden" name="return_idea_id" value=(idea_id);
            input type="hidden" name="action" value=(if proposal.is_some() { "project_proposal.revise" } else { "project_proposal.create" });
            input type="hidden" name="source_idea_id" value=(idea_id);
            input type="hidden" name="source_idea_revision" value=(idea_revision.revision);
            @if let Some(proposal) = proposal {
                input type="hidden" name="expected_revision" value=(proposal.current_revision);
            }
            div class="goal-contract-form__intro" {
                strong { "先审查最少信息" }
                span { "目标、为何现在做、如何验证、何时停止；不确定的内容放进未知。" }
            }
            label { "项目名称"
                input name="title" required maxlength="240"
                    value=(revision.map(|item| item.title.as_str()).unwrap_or(&idea_revision.title));
            }
            label { "项目意图"
                textarea name="project_intent" rows="3" required { (revision.map(|item| item.project_intent.as_str()).unwrap_or(&idea_revision.body)) }
            }
            label { "为什么现在值得立项？"
                textarea name="why_now" rows="2" required { (revision.map(|item| item.why_now.as_str()).unwrap_or("这个想法已经值得通过独立目标枝干验证")) }
            }
            label { "第一条根目标想得到什么结果？"
                textarea name="desired_outcome" rows="3" required { (desired_outcome) }
            }
            div class="goal-form-columns" {
                label { "怎样验证（每行一项）"
                    textarea name="validation_plan" rows="3" required { (contract_lines("validationPlan")) }
                }
                label { "何时完成或停止（每行一项）"
                    textarea name="stop_conditions" rows="3" required { (contract_lines("stopConditions")) }
                }
            }
            label { "仍然不知道什么？"
                textarea name="unknowns" rows="2" placeholder="不知道可以诚实保留" { (contract_lines("unknowns")) }
            }
            label { "何时回来请你凭感觉判断？"
                textarea name="judgment_triggers" rows="2" { (contract_lines("judgmentTriggers")) }
            }
            details class="goal-form-advanced" {
                summary { "按需披露更多契约与来源取舍" }
                div class="goal-form-columns" {
                    label { "硬约束" textarea name="hard_constraints" rows="2" { (contract_lines("hardConstraints")) } }
                    label { "主观偏好" textarea name="subjective_preferences" rows="2" { (contract_lines("subjectivePreferences")) } }
                    label { "明确不做" textarea name="non_goals" rows="2" { (contract_lines("nonGoals")) } }
                    label { "期望贡献" textarea name="expected_contributions" rows="2" { (contract_lines("expectedContributions")) } }
                    label { "探索计划" textarea name="exploration_plan" rows="2" { (root_lines("explorationPlan")) } }
                    label { "工具需求" textarea name="tool_requirements" rows="2" { (root_lines("toolRequirements")) } }
                    label { "这次保留的想法内容" textarea name="retained_notes" rows="2" { (revision.map(|item| item.retained_notes.0.join("\n")).unwrap_or_default()) } }
                    label { "这次暂不采用的内容" textarea name="omitted_notes" rows="2" { (revision.map(|item| item.omitted_notes.0.join("\n")).unwrap_or_default()) } }
                    label { "AI 推断（非用户事实）" textarea name="inferences" rows="2" { (root_lines("inferences")) } }
                }
                @if all_ideas.iter().any(|idea| idea.id != idea_id) {
                    fieldset class="proposal-source-selector" data-proposal-source-selector="" {
                        legend { "同时引用其他想法（按需）" }
                        input type="hidden" name="additional_sources" value="" data-proposal-sources-value="";
                        @for idea in all_ideas.iter().filter(|idea| idea.id != idea_id) {
                            label {
                                input type="checkbox" value=(format!("{}@{}", idea.id, idea.current_revision))
                                    data-proposal-source="" checked[selected_source_ids.contains(&idea.id)];
                                span { strong { (&idea.title) } small { "v" (idea.current_revision) " · 作为支持来源" } }
                            }
                        }
                    }
                }
                @if proposal.is_some() {
                    label { "本次修订理由" textarea name="revision_reason" rows="2" required {} }
                }
            }
            button class="button button--primary" type="submit" {
                @if proposal.is_some() { "保存新版 ProjectProposal" } @else { "建立 ProjectProposal 草案" }
            }
        }
    }
}

fn project_card(project: &ProjectSummary, view: &str) -> Markup {
    html! {
        a class="project-card" href=(format!("/projects/{}", project.id)) {
            div class="project-card__top" {
                span class=(format!("project-dot project-dot--{}", state_tone(&project.state))) {}
                span class=(format!("status-tag status-tag--{}", state_tone(&project.state))) {
                    (state_label(&project.state))
                }
                time { (format_date(project.updated_at)) }
            }
            h3 { (&project.title) }
            p class="project-card__intent" { (&project.intent) }
            div class="project-card__focus" {
                span { "当前焦点" }
                strong { (project.current_focus.as_deref().unwrap_or("等待确定下一项行动")) }
            }
            footer {
                span {
                    @if view == "attention" {
                        (project.attention_count) " 项需要处理"
                    } @else if view == "artifacts" {
                        (project.artifact_count) " 项正式产物"
                    } @else if project.contract_status.as_deref() == Some("confirmed") {
                        "✓ 完成标准已确认"
                    } @else {
                        "○ 完成标准待确认"
                    }
                }
                b aria-hidden="true" { "→" }
            }
        }
    }
}

pub fn new_project(error: Option<&str>) -> Markup {
    let content = html! {
        div class="focused-page" {
            a class="back-link" href="/" { "← 返回项目" }
            section class="intake-card" {
                div class="intake-card__mark" { "01" }
                span class="eyebrow" { "NEW PROJECT" }
                h1 { "现在，你想让什么事情真正向前走？" }
                p { "不用先整理成方案，也不用选择类型。保留你的原始表达，系统只会追问会改变推进方向的关键歧义。" }
                @if let Some(message) = error {
                    div class="flash flash--error" role="alert" { (message) }
                }
                form class="intake-form" method="post" action="/projects" {
                    label for="intent" { "项目意图" }
                    textarea id="intent" name="intent" rows="8" required autofocus
                        placeholder="例如：我想把一个反复手工完成的流程做成任何人都能使用的小工具，并找到三位真实用户验证它。" {}
                    div class="intake-hints" {
                        span { "✦ 一句话就能开始" }
                        span { "✦ 不按关键词分类" }
                        span { "✦ 所有状态都可恢复" }
                    }
                    button class="button button--primary button--large" type="submit" {
                        "形成项目起点" span aria-hidden="true" { "→" }
                    }
                }
            }
        }
    };
    layout("创建项目", "projects", content)
}

pub fn project(
    snapshot: &ProjectSnapshot,
    goal_snapshot: &GoalGraphSnapshot,
    activity: &WorkbenchActivity,
    query: &ProjectPageQuery,
) -> Markup {
    let tab = query.tab.as_deref().unwrap_or("goals");
    let content = html! {
        header class="project-head" {
            div class="project-head__row" {
                a class="back-link" href="/" { "← 项目" }
                span class="head-separator" {}
                span class=(format!("project-dot project-dot--{}", state_tone(&snapshot.project.state))) {}
                h1 { (&snapshot.project.title) }
                span class=(format!("status-tag status-tag--{}", state_tone(&snapshot.project.state))) {
                    (state_label(&snapshot.project.state))
                }
            }
            p { (snapshot.project.current_focus.as_deref().unwrap_or("尚未确定当前焦点")) }
            nav class="project-tabs" aria-label="项目页面" {
                (project_tab(snapshot.project.id, "goals", "目标枝干", tab, goal_snapshot.sessions.len()))
                (project_tab(snapshot.project.id, "graph", "探索图", tab, snapshot.nodes.len()))
                (project_tab(snapshot.project.id, "contract", "成果契约", tab, snapshot.contract.as_ref().map(|_| 1).unwrap_or(0)))
                (project_tab(snapshot.project.id, "outputs", "产物与证据", tab, snapshot.artifacts.len() + snapshot.evidence.len()))
                (project_tab(snapshot.project.id, "history", "历史", tab, snapshot.events.len()))
            }
        }

        @if let Some(message) = query.notice.as_deref() {
            div class="flash flash--success" role="status" { (message) }
        }
        @if let Some(message) = query.error.as_deref() {
            div class="flash flash--error" role="alert" { (message) }
        }

        @if tab != "goals" {
            (current_action_panel(snapshot))
        }

        @match tab {
            "goals" => (goal_workbench(goal_snapshot, activity, query.session)),
            "contract" => (contract_view(snapshot)),
            "outputs" => (outputs_view(snapshot)),
            "history" => (history_view(snapshot)),
            _ => (graph_view(snapshot, query.node)),
        }
    };
    layout(&snapshot.project.title, "projects", content)
}

fn project_tab(project_id: Uuid, value: &str, label: &str, selected: &str, count: usize) -> Markup {
    html! {
        a class=(if selected == value { "is-active" } else { "" })
            href=(format!("/projects/{project_id}?tab={value}")) {
            (label)
            @if count > 0 { span { (count) } }
        }
    }
}

fn current_action_panel(snapshot: &ProjectSnapshot) -> Markup {
    let current = snapshot
        .actions
        .iter()
        .find(|action| matches!(action.status.as_str(), "ready" | "running" | "blocked"));
    let Some(action) = current else {
        return html! {};
    };
    html! {
        section class="next-action" {
            div class="next-action__index" { "NOW" }
            div class="next-action__copy" {
                span class="eyebrow" { "唯一当前行动" }
                h2 { (&action.title) }
                @if let Some(signal) = &action.expected_signal { p { (signal) } }
                div class="action-meta" {
                    span { (if action.owner == "human" { "由你完成" } else { "由浮点执行" }) }
                    @if action.requires_approval == 1 { span { "需要确认" } }
                    span { (action_status_label(&action.status)) }
                }
            }
            div class="next-action__control" {
                (action_control(snapshot, action))
            }
        }
    }
}

fn action_control(snapshot: &ProjectSnapshot, action: &ActionRun) -> Markup {
    let target = format!("/projects/{}/actions", snapshot.project.id);
    match action.kind.as_str() {
        "confirm_outcome" => html! {
            form class="inline-action-form" method="post" action=(target) {
                input type="hidden" name="action" value="confirm_outcome";
                label for="completion-evidence" { "什么事实出现时，项目才算真正完成？" }
                textarea id="completion-evidence" name="completion_evidence" rows="3" required
                    placeholder="写一个能在项目外部观察或检查的结果" {}
                button class="button button--primary" type="submit" { "确认完成标准" }
            }
        },
        "draft_project_brief" => html! {
            form method="post" action=(target) {
                input type="hidden" name="action" value="generate_brief";
                button class="button button--primary" type="submit" {
                    "生成启动说明" span aria-hidden="true" { "→" }
                }
            }
        },
        "review_project_brief" => {
            let artifact = snapshot
                .artifacts
                .iter()
                .find(|artifact| artifact.kind == "project_brief" && artifact.status == "review");
            html! {
                div class="review-actions" {
                    @if let Some(artifact) = artifact {
                        a class="button button--secondary" href=(format!("/artifacts/{}", artifact.id)) target="_blank" {
                            "打开启动说明 ↗"
                        }
                    }
                    form method="post" action=(target) {
                        input type="hidden" name="action" value="approve_brief";
                        button class="button button--primary" type="submit" { "通过审阅" }
                    }
                }
            }
        }
        _ => html! {
            a class="button button--primary" href=(format!("/projects/{}?tab=graph#graph-workspace", snapshot.project.id)) {
                "在脉络中推进"
            }
        },
    }
}

fn contract_view(snapshot: &ProjectSnapshot) -> Markup {
    let Some(contract) = &snapshot.contract else {
        return html! { div class="empty-state" { h3 { "成果契约不存在" } } };
    };
    html! {
        section class="contract-layout" {
            div class="contract-main" {
                article class="content-card content-card--hero" {
                    div class="content-card__head" {
                        span class="eyebrow" { "DESIRED OUTCOME" }
                        span class=(format!("status-tag status-tag--{}", if contract.status == "confirmed" { "active" } else { "shaping" })) {
                            (if contract.status == "confirmed" { "已确认" } else { "草稿" })
                        }
                    }
                    h2 { "想得到的结果" }
                    p class="contract-outcome" { (&contract.desired_outcome) }
                }
                div class="contract-columns" {
                    (list_card("什么事实代表完成", "EVIDENCE", &contract.success_evidence.0, "✓"))
                    (list_card("必须遵守的约束", "CONSTRAINTS", &contract.constraints.0, "◇"))
                    (list_card("明确不做", "NON-GOALS", &contract.non_goals.0, "—"))
                }
            }
            aside class="contract-aside" {
                article class="version-card" {
                    span class="eyebrow" { "VERSION" }
                    strong { "v" (contract.version) }
                    p { "目标发生实质变化时创建新版本，不静默覆盖旧目标。" }
                    @if contract.status == "draft" {
                        details {
                            summary class="button button--secondary" { "修订原始意图" }
                            form class="stack-form" method="post" action=(format!("/projects/{}/actions", snapshot.project.id)) {
                                input type="hidden" name="action" value="revise_intent";
                                textarea name="intent" rows="7" required { (&snapshot.project.intent) }
                                button class="button button--primary" type="submit" { "保存修订" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn list_card(title: &str, eyebrow: &str, items: &[String], marker: &str) -> Markup {
    html! {
        article class="content-card list-card" {
            span class="eyebrow" { (eyebrow) }
            h3 { (title) }
            ul {
                @for item in items {
                    li { span aria-hidden="true" { (marker) } p { (item) } }
                }
            }
        }
    }
}

fn outputs_view(snapshot: &ProjectSnapshot) -> Markup {
    html! {
        section class="outputs-layout" {
            div class="outputs-column" {
                div class="section-heading" { div { span class="eyebrow" { "ARTIFACTS" } h2 { "正式产物" } } span class="section-count" { (snapshot.artifacts.len()) } }
                @if snapshot.artifacts.is_empty() {
                    div class="empty-compact" { "还没有登记正式产物。" }
                }
                @for artifact in &snapshot.artifacts {
                    article class="output-card" {
                        div class="output-card__icon" { "↗" }
                        div {
                            span { (&artifact.kind) " · v" (artifact.version) }
                            h3 { (&artifact.title) }
                            code { (&artifact.sha256[..artifact.sha256.len().min(16)]) "…" }
                        }
                        span class=(format!("status-tag status-tag--{}", if artifact.status == "approved" { "active" } else { "waiting" })) { (&artifact.status) }
                        a href=(format!("/artifacts/{}", artifact.id)) target="_blank" aria-label="打开产物" { "打开" }
                    }
                }
            }
            div class="outputs-column" {
                div class="section-heading" { div { span class="eyebrow" { "EVIDENCE" } h2 { "证据与质量门" } } span class="section-count" { (snapshot.evidence.len() + snapshot.quality_gates.len()) } }
                @for evidence in &snapshot.evidence {
                    article class="evidence-card" {
                        span class="evidence-card__kind" { "证据" }
                        p { (&evidence.summary) }
                        small { (evidence.stance.as_str()) @if let Some(confidence) = evidence.confidence { " · " (confidence) "%" } }
                    }
                }
                @for gate in &snapshot.quality_gates {
                    article class=(format!("gate-card gate-card--{}", gate.status)) {
                        div { span { (if gate.required == 1 { "必选质量门" } else { "可选质量门" }) } b { (&gate.status) } }
                        h3 { (&gate.title) }
                        ul { @for criterion in &gate.criteria.0 { li { (criterion) } } }
                    }
                }
            }
        }
    }
}

fn history_view(snapshot: &ProjectSnapshot) -> Markup {
    html! {
        section class="history-panel" {
            div class="section-heading" {
                div { span class="eyebrow" { "IMMUTABLE LEDGER" } h2 { "不可覆盖的项目历史" } }
                span class="section-count" { (snapshot.events.len()) }
            }
            ol class="history-list" {
                @for event in &snapshot.events {
                    li {
                        span class="history-list__rail" {}
                        div class="history-list__icon" { (event_actor_icon(&event.actor_type)) }
                        article {
                            div { strong { (event_label(&event.event_type)) } time { (format_date_time(event.created_at)) } }
                            small { (actor_label(&event.actor_type)) " · " (&event.event_type) }
                        }
                    }
                }
            }
        }
    }
}

fn goal_workbench(
    snapshot: &GoalGraphSnapshot,
    activity: &WorkbenchActivity,
    requested_session: Option<Uuid>,
) -> Markup {
    let projection = GoalGraphProjection::build(snapshot, requested_session);
    let open_proposals = snapshot
        .proposals
        .iter()
        .filter(|proposal| matches!(proposal.status.as_str(), "draft" | "awaiting_approval"))
        .collect::<Vec<_>>();
    let selected = projection
        .selected_session_id
        .and_then(|session_id| snapshot.sessions.iter().find(|item| item.id == session_id));
    let open_attention = snapshot
        .attention_items
        .iter()
        .filter(|item| item.status == "open")
        .count();

    html! {
        section id="goal-workbench" class="goal-workbench"
            data-projection-version=(PROJECTION_VERSION) {
            header class="goal-toolbar" {
                div {
                    span class="eyebrow" { "GOAL BRANCH WORKBENCH · PROJECTION " (PROJECTION_VERSION) }
                    h2 { "目标枝干工作台" }
                    p { "一条枝干承载一个目标；圆点卡片代表一次 Agent Session。图只是可替换投影，审核语义保存在领域记录中。" }
                }
                div class="goal-toolbar__stats" aria-label="目标枝干概览" {
                    span { b { (snapshot.branches.len()) } "目标" }
                    span { b { (snapshot.sessions.len()) } "Session" }
                    span class=(if open_attention > 0 { "has-attention" } else { "" }) {
                        b { (open_attention) } "待判断"
                    }
                }
            }

            @if !open_proposals.is_empty() {
                section class="proposal-queue" aria-label="待决定的 BranchProposal" {
                    div class="proposal-queue__heading" {
                        span class="eyebrow" { "BRANCH PROPOSALS" }
                        strong { (open_proposals.len()) " 项分枝提案等待推进" }
                    }
                    div class="proposal-queue__cards" {
                        @for proposal in open_proposals {
                            (proposal_card(snapshot, proposal))
                        }
                    }
                }
            }

            @if projection.lanes.is_empty() {
                div class="goal-empty-layout" {
                    article class="goal-empty-copy" {
                        span class="goal-empty-copy__mark" { "00" }
                        span class="eyebrow" { "FIRST GOAL" }
                        h3 { "先形成第一条目标枝干" }
                        p { "不需要填写巨量表格。先写清想得到什么、怎样验证、何时停止；暂时说不清的内容诚实放进“未知”。" }
                        ul {
                            li { "能明确的部分写清楚" }
                            li { "不能明确的部分标为未知" }
                            li { "约定何时回来请你凭感觉判断" }
                        }
                    }
                    (proposal_editor(snapshot.project.id, "proposal.create", None, None, None))
                }
            } @else {
                div class="goal-workbench__layout" {
                    section class="goal-map" aria-label="目标枝干与 Agent Session" {
                        div class="goal-map__scroll" data-goal-map-scroll {
                            ol class="goal-lanes" {
                                @for lane in &projection.lanes {
                                    (goal_lane(snapshot, lane, projection.selected_session_id))
                                }
                            }
                        }
                    }
                    aside class="session-worksite" aria-label="选中 Session 的工作现场" {
                        @if let Some(session) = selected {
                            (session_worksite(snapshot, activity, session))
                        } @else {
                            div class="worksite-empty" {
                                span { "◎" }
                                h3 { "选择一个 Session" }
                                p { "这里会显示目标、文件、工具行动、测试证据和审核记录。" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn goal_lane(
    snapshot: &GoalGraphSnapshot,
    lane: &GoalLane,
    selected_session_id: Option<Uuid>,
) -> Markup {
    let open_attention = snapshot
        .attention_items
        .iter()
        .filter(|item| item.goal_branch_id == Some(lane.branch_id) && item.status == "open")
        .count();
    html! {
        li class=(format!("goal-lane goal-lane--{}", goal_status_tone(&lane.status)))
            style=(format!("--goal-depth:{}", lane.depth))
            data-branch-id=(lane.branch_id) {
            div class="goal-lane__identity" {
                span class="goal-lane__fork" aria-hidden="true" { "⌁" }
                div {
                    small {
                        @if lane.parent_branch_id.is_some() { "子目标" } @else { "根目标" }
                    }
                    strong { (&lane.name) }
                }
                span class=(format!("goal-state goal-state--{}", goal_status_tone(&lane.status))) {
                    (goal_status_label(&lane.status))
                }
                @if open_attention > 0 {
                    b class="goal-attention-count" title="待处理" { (open_attention) }
                }
            }
            div class="goal-session-chain" {
                @for session_id in &lane.session_ids {
                    @if let Some(session) = snapshot.sessions.iter().find(|item| item.id == *session_id) {
                        @let contribution_count = snapshot.contributions.iter().filter(|item| item.session_id == session.id).count();
                        @let gate = snapshot.review_gates.iter().rev().find(|item| item.session_id == session.id);
                        a class=(format!("goal-session-node goal-session-node--{}{}{}",
                                goal_status_tone(&session.status),
                                if Some(session.id) == selected_session_id { " is-selected" } else { "" },
                                if lane.head_session_id == session.id { " is-head" } else { "" }))
                            data-session-id=(session.id)
                            data-selected=(if Some(session.id) == selected_session_id { "true" } else { "false" })
                            href=(format!("/projects/{}?tab=goals&session={}#goal-workbench", snapshot.project.id, session.id)) {
                            i class="goal-session-node__dot" aria-hidden="true" {}
                            span class="goal-session-node__copy" {
                                small {
                                    "SESSION " (format!("{:02}", session.session_number))
                                    @if lane.head_session_id == session.id { b { "HEAD" } }
                                }
                                strong { (&session.assignment) }
                                em { (goal_status_label(&session.status)) }
                            }
                            span class="goal-session-node__signals" {
                                @if contribution_count > 0 { b { (contribution_count) " 产出" } }
                                @if let Some(gate) = gate { b { (review_status_label(&gate.status)) } }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn proposal_card(snapshot: &GoalGraphSnapshot, proposal: &GoalProposalRecord) -> Markup {
    let revision = latest_proposal_revision(snapshot, proposal);
    let desired_outcome = revision
        .and_then(|item| item.contract.0.get("desiredOutcome"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("契约内容缺失");
    html! {
        article class="proposal-card" data-proposal-id=(proposal.id) {
            header {
                span class="proposal-card__kind" {
                    @if proposal.parent_session_id.is_some() { "子目标提案" } @else { "根目标提案" }
                }
                span class=(format!("goal-state goal-state--{}", goal_status_tone(&proposal.status))) {
                    (proposal_status_label(&proposal.status))
                }
            }
            h3 { (desired_outcome) }
            @if let Some(revision) = revision {
                p { (&revision.why_needed) }
                div class="proposal-card__meta" {
                    span { "v" (proposal.current_revision) }
                    span { (json_array_len(&revision.contract.0, "unknowns")) " 项未知" }
                    span { (json_array_len(&revision.contract.0, "validationPlan")) " 项验证" }
                }
            }
            div class="proposal-card__actions" {
                @if proposal.status == "draft" {
                    form method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="action" value="proposal.submit";
                        input type="hidden" name="proposal_id" value=(proposal.id);
                        input type="hidden" name="expected_revision" value=(proposal.current_revision);
                        @if let Some(session_id) = proposal.parent_session_id {
                            input type="hidden" name="return_session_id" value=(session_id);
                        }
                        button class="button button--primary button--small" type="submit" { "提交审核" }
                    }
                } @else if proposal.status == "awaiting_approval" {
                    details class="proposal-decision" open {
                        summary { "批准并创建独立枝干" }
                        form class="goal-form" method="post"
                            action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                            input type="hidden" name="action" value="proposal.approve";
                            input type="hidden" name="proposal_id" value=(proposal.id);
                            input type="hidden" name="expected_revision" value=(proposal.current_revision);
                            @if let Some(session_id) = proposal.parent_session_id {
                                input type="hidden" name="return_session_id" value=(session_id);
                            }
                            label { "枝干名称" input name="branch_name" required value=(goal_short_title(desired_outcome, 28)); }
                            label { "第一个 Session 要做什么" textarea name="assignment" rows="2" required { (desired_outcome) } }
                            label { "Agent 身份（可选）" input name="agent_identity" placeholder="worker-main"; }
                            button class="button button--primary button--small" type="submit" { "批准 BranchProposal" }
                        }
                    }
                }
                @if let Some(revision) = revision {
                    details class="proposal-decision" {
                        summary { "修订目标契约" }
                        (proposal_editor(snapshot.project.id, "proposal.revise", Some(proposal), proposal.parent_session_id, Some(revision)))
                    }
                }
                details class="proposal-decision proposal-decision--danger" {
                    summary { "取消这项提案" }
                    form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="action" value="proposal.cancel";
                        input type="hidden" name="proposal_id" value=(proposal.id);
                        @if let Some(session_id) = proposal.parent_session_id {
                            input type="hidden" name="return_session_id" value=(session_id);
                        }
                        label { "原因" textarea name="reason" rows="2" required {} }
                        button class="button button--secondary button--small" type="submit" { "确认取消" }
                    }
                }
            }
        }
    }
}

fn proposal_editor(
    project_id: Uuid,
    action: &str,
    proposal: Option<&GoalProposalRecord>,
    parent_session_id: Option<Uuid>,
    revision: Option<&GoalProposalRevisionRecord>,
) -> Markup {
    let why_needed = revision.map(|item| item.why_needed.as_str()).unwrap_or("");
    let desired_outcome = revision
        .and_then(|item| item.contract.0.get("desiredOutcome"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let contract_lines = |key: &str| {
        revision
            .and_then(|item| item.contract.0.get(key))
            .map(json_value_lines)
            .unwrap_or_default()
    };
    let outer_lines =
        |value: Option<&serde_json::Value>| value.map(json_value_lines).unwrap_or_default();
    let form_id = proposal
        .map(|item| item.id.to_string())
        .or_else(|| parent_session_id.map(|item| item.to_string()))
        .unwrap_or_else(|| "root".into());
    html! {
        form class="goal-contract-form" method="post"
            action=(format!("/projects/{project_id}/goal-commands"))
            data-goal-contract-form=(form_id) {
            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
            input type="hidden" name="action" value=(action);
            @if let Some(proposal) = proposal {
                input type="hidden" name="proposal_id" value=(proposal.id);
                input type="hidden" name="expected_revision" value=(proposal.current_revision);
            }
            @if let Some(session_id) = parent_session_id {
                input type="hidden" name="parent_session_id" value=(session_id);
                input type="hidden" name="return_session_id" value=(session_id);
            }
            div class="goal-contract-form__intro" {
                strong { "最少先填 4 项" }
                span { "目标、原因、验证、停止；没想清的写入“未知”。" }
            }
            label { "为什么要单独做这件事？"
                textarea name="why_needed" rows="2" required { (why_needed) }
            }
            label { "想得到的结果"
                textarea name="desired_outcome" rows="3" required { (desired_outcome) }
            }
            div class="goal-form-columns" {
                label { "怎样验证（每行一项）"
                    textarea name="validation_plan" rows="3" required { (contract_lines("validationPlan")) }
                }
                label { "何时完成或停止（每行一项）"
                    textarea name="stop_conditions" rows="3" required { (contract_lines("stopConditions")) }
                }
            }
            label { "目前还不知道什么？（可留空）"
                textarea name="unknowns" rows="2" placeholder="不能明确的部分诚实写在这里" { (contract_lines("unknowns")) }
            }
            details class="goal-form-advanced" {
                summary { "按需补充约束、偏好、工具和探索计划" }
                div class="goal-form-columns" {
                    label { "硬约束" textarea name="hard_constraints" rows="2" { (contract_lines("hardConstraints")) } }
                    label { "主观偏好" textarea name="subjective_preferences" rows="2" { (contract_lines("subjectivePreferences")) } }
                    label { "明确不做" textarea name="non_goals" rows="2" { (contract_lines("nonGoals")) } }
                    label { "何时请你判断" textarea name="judgment_triggers" rows="2" { (contract_lines("judgmentTriggers")) } }
                    label { "期望回流的贡献" textarea name="expected_contributions" rows="2" { (revision.map(|item| outer_lines(Some(&item.expected_contributions.0))).unwrap_or_default()) } }
                    label { "探索计划" textarea name="exploration_plan" rows="2" { (revision.map(|item| outer_lines(Some(&item.exploration_plan.0))).unwrap_or_default()) } }
                    label { "工具需求" textarea name="tool_requirements" rows="2" { (revision.map(|item| outer_lines(Some(&item.tool_requirements.0))).unwrap_or_default()) } }
                    label { "AI 推断（非用户确认）" textarea name="inferences" rows="2" { (revision.map(|item| outer_lines(Some(&item.inferences.0))).unwrap_or_default()) } }
                }
                @if proposal.is_some() {
                    label { "本次修订理由" textarea name="revision_reason" rows="2" required { (revision.and_then(|item| item.revision_reason.as_deref()).unwrap_or("")) } }
                }
            }
            button class="button button--primary" type="submit" {
                @if action == "proposal.create" { "建立 BranchProposal 草案" }
                @else if action == "session.propose_child" { "提议拆出子目标" }
                @else { "保存新版契约" }
            }
        }
    }
}

fn session_worksite(
    snapshot: &GoalGraphSnapshot,
    activity: &WorkbenchActivity,
    session: &GoalSessionRecord,
) -> Markup {
    let branch = snapshot
        .branches
        .iter()
        .find(|item| item.id == session.goal_branch_id);
    let contract = snapshot
        .contracts
        .iter()
        .find(|item| item.id == session.contract_version_id);
    let inputs = activity
        .inputs
        .iter()
        .filter(|item| item.session_id == session.id)
        .collect::<Vec<_>>();
    let tool_calls = activity
        .tool_calls
        .iter()
        .filter(|item| item.session_id == session.id)
        .collect::<Vec<_>>();
    let environment = activity
        .environments
        .iter()
        .find(|item| item.session_id == session.id);
    let contributions = snapshot
        .contributions
        .iter()
        .filter(|item| item.session_id == session.id)
        .collect::<Vec<_>>();
    let gates = snapshot
        .review_gates
        .iter()
        .filter(|item| item.session_id == session.id)
        .collect::<Vec<_>>();
    let attention = snapshot
        .attention_items
        .iter()
        .filter(|item| item.session_id == Some(session.id))
        .collect::<Vec<_>>();

    html! {
        header class="worksite-head" {
            div {
                span class="eyebrow" { "AGENT SESSION WORKSITE" }
                h3 { "Session " (format!("{:02}", session.session_number)) }
            }
            span class=(format!("goal-state goal-state--{}", goal_status_tone(&session.status))) {
                (goal_status_label(&session.status))
            }
        }
        p class="worksite-assignment" { (&session.assignment) }
        div class="worksite-meta" {
            span { b { "目标" } (branch.map(|item| item.name.as_str()).unwrap_or("未知")) }
            span { b { "Agent" } (session.agent_identity.as_deref().unwrap_or("未指定")) }
            span { b { "契约" } "v" (contract.map(|item| item.version).unwrap_or_default()) }
            span { b { "环境" }
                @if let Some(environment) = environment {
                    code title=(environment.environment_fingerprint.as_str()) {
                        (&environment.environment_fingerprint[..environment.environment_fingerprint.len().min(15)]) "…"
                    }
                } @else { "未绑定" }
            }
        }

        @if let Some(contract) = contract {
            details class="worksite-contract" open {
                summary { "目标契约 · 这个 Session 为什么存在" }
                strong { (&contract.desired_outcome) }
                div class="worksite-contract__lists" {
                    (compact_list("验证", &contract.validation_plan.0))
                    (compact_list("停止", &contract.stop_conditions.0))
                    (compact_list("未知", &contract.unknowns.0))
                }
            }
        }

        @for item in attention.iter().filter(|item| item.status == "open") {
            article class="attention-card" role="status" {
                header { span { "暂停 · " (attention_kind_label(&item.kind)) } b { "需要你" } }
                h4 { (&item.title) }
                p { (&item.reason) }
                @if let Some(checkpoint) = &item.safe_checkpoint { small { b { "安全点：" } (checkpoint) } }
                @if let Some(attempted) = &item.attempted { small { b { "已尝试：" } (attempted) } }
                @if let Some(risk) = &item.risk { small { b { "风险：" } (risk) } }
                @if let Some(action) = &item.user_action { small { b { "请你：" } (action) } }
                @if let Some(recommendation) = &item.recommendation { small { b { "AI 建议：" } (recommendation) } }
            }
        }

        div class="worksite-sections" {
            section class="worksite-section" data-worksite-files {
                header { span class="eyebrow" { "FILES / ARTIFACTS" } b { (inputs.len()) } }
                h4 { "文件与产物" }
                @if inputs.is_empty() {
                    p class="worksite-empty-copy" { "尚无输入文件。手机和电脑上传都会先进入受限暂存区。" }
                }
                div class="worksite-records" {
                    @for input in &inputs {
                        article class="worksite-record" {
                            span class="worksite-record__icon" { "◇" }
                            div {
                                strong { (&input.display_name) }
                                small { (input.actual_size) " B · " (input_status_label(&input.status))
                                    @if let Some(mode) = &input.import_mode { " · " (import_mode_label(mode)) }
                                }
                            }
                            @if matches!(input.status.as_str(), "available" | "imported") {
                                a href=(format!("/api/v1/projects/{}/sessions/{}/inputs/{}/content", snapshot.project.id, session.id, input.id)) { "下载" }
                            }
                        }
                    }
                }
                @if session.status == "running" {
                    form class="session-upload" data-input-upload
                        data-project-id=(snapshot.project.id) data-session-id=(session.id) {
                        label { span { "选择文件" } input type="file" name="file" required; }
                        label { span { "Session 内路径（可选）" } input name="inbox_relative_path" placeholder="inputs/notes.md"; }
                        button class="button button--secondary button--small" type="submit" { "验证并导入" }
                        output data-upload-status aria-live="polite" {}
                    }
                }
            }

            section class="worksite-section" data-worksite-tools {
                header { span class="eyebrow" { "TOOLS / BROWSER / TESTS" } b { (tool_calls.len()) } }
                h4 { "工具行动与测试证据" }
                @if tool_calls.is_empty() && gates.iter().all(|gate| json_value_len(&gate.test_evidence.0) == 0) {
                    p class="worksite-empty-copy" { "尚无工具或测试记录。未来 Playwright 浏览器会以插件 ToolCall 出现在这里。" }
                }
                div class="worksite-records" {
                    @for call in tool_calls {
                        article class="tool-record" {
                            div { strong { (&call.tool_name) } span class=(format!("goal-state goal-state--{}", goal_status_tone(&call.status))) { (tool_status_label(&call.status)) } }
                            p { (&call.plugin_id) "@" (&call.plugin_version) }
                            small { (format_date_time(call.completed_at)) }
                        }
                    }
                    @for gate in &gates {
                        @for evidence in json_value_strings(&gate.test_evidence.0) {
                            article class="test-evidence-record" { span { "✓" } p { (evidence) } }
                        }
                    }
                }
            }
        }

        section class="worksite-section worksite-section--wide" data-worksite-contributions {
            header { span class="eyebrow" { "CONTRIBUTIONS" } b { (contributions.len()) } }
            h4 { "本 Session 留下的可回流产出" }
            @if contributions.is_empty() {
                p class="worksite-empty-copy" { "过程日志不会自动冒充产出；Agent 必须显式记录 Contribution。" }
            }
            div class="contribution-stack" {
                @for item in &contributions {
                    article {
                        span { (goal_contribution_kind_label(&item.kind)) }
                        strong { (&item.title) }
                        p { (&item.body) }
                    }
                }
            }
        }

        @for gate in &gates {
            (review_gate_panel(snapshot, session, gate))
        }

        (session_action_panel(snapshot, session, contract, &contributions))
    }
}

fn compact_list(title: &str, items: &[String]) -> Markup {
    html! {
        div {
            b { (title) }
            @if items.is_empty() { span { "未设定" } }
            @for item in items { span { (item) } }
        }
    }
}

fn review_gate_panel(
    snapshot: &GoalGraphSnapshot,
    session: &GoalSessionRecord,
    gate: &GoalReviewGateRecord,
) -> Markup {
    let decisions = snapshot
        .review_decisions
        .iter()
        .filter(|item| item.review_gate_id == gate.id)
        .collect::<Vec<_>>();
    let candidate_ids = candidate_contribution_ids(gate);
    let candidate_id_list = candidate_ids
        .iter()
        .map(Uuid::to_string)
        .collect::<Vec<_>>()
        .join(",");
    html! {
        section class=(format!("review-panel review-panel--{}", goal_status_tone(&gate.status)))
            data-review-gate-id=(gate.id) {
            header {
                div { span class="eyebrow" { "PROPOSED MERGE" } h4 { "拟合并审核" } }
                span class=(format!("goal-state goal-state--{}", goal_status_tone(&gate.status))) { (review_status_label(&gate.status)) }
            }
            p class="review-hash" { "候选快照 " code { (&gate.candidate_hash[..gate.candidate_hash.len().min(24)]) "…" } }
            div class="review-evidence-grid" {
                div { b { "Agent 自查" } p { (gate.self_check.0.get("summary").and_then(serde_json::Value::as_str).unwrap_or("未记录")) } }
                div { b { "测试" } @for item in json_value_strings(&gate.test_evidence.0) { span { (item) } } }
                div { b { "风险" } @if json_value_len(&gate.risks.0) == 0 { span { "未记录" } } @for item in json_value_strings(&gate.risks.0) { span { (item) } } }
            }
            @for decision in decisions {
                article class="review-decision-record" {
                    header { b { (review_actor_label(&decision.actor_role)) } span { (review_decision_label(&decision.decision)) } }
                    p { (&decision.rationale) }
                }
            }
            @if gate.status == "pending_ai_review" {
                details class="review-action" open {
                    summary { "独立审核 AI 记录建议" }
                    form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="action" value="review.ai_record";
                        input type="hidden" name="review_gate_id" value=(gate.id);
                        input type="hidden" name="return_session_id" value=(session.id);
                        label { "审核身份" input name="reviewer_identity" required value="reviewer-v0.1"; }
                        label { "建议"
                            select name="review_decision" { option value="recommend_accept" { "建议接受" } option value="recommend_reject" { "建议退回" } }
                        }
                        label { "理由" textarea name="rationale" rows="3" required {} }
                        label { "复验证据（每行一项）" textarea name="test_evidence" rows="2" {} }
                        button class="button button--primary button--small" type="submit" { "保存独立审核" }
                    }
                }
            } @else if gate.status == "pending_human_review" {
                div class="human-review-actions" {
                    form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="action" value="review.human_decide";
                        input type="hidden" name="review_gate_id" value=(gate.id);
                        input type="hidden" name="return_session_id" value=(session.id);
                        input type="hidden" name="review_decision" value="accept";
                        input type="hidden" name="selected_contribution_ids" value=(candidate_id_list.as_str());
                        label { "接受理由" textarea name="rationale" rows="2" required {} }
                        button class="button button--primary" type="submit" { "接受整条枝干产出" }
                    }
                    form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="action" value="review.human_decide";
                        input type="hidden" name="review_gate_id" value=(gate.id);
                        input type="hidden" name="return_session_id" value=(session.id);
                        input type="hidden" name="review_decision" value="reject";
                        label { "退回理由" textarea name="rationale" rows="2" required {} }
                        button class="button button--secondary" type="submit" { "退回到下一 Session" }
                    }
                }
                @if candidate_ids.len() > 1 {
                    details class="review-action" {
                        summary { "部分接受（高级）" }
                        form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                            input type="hidden" name="action" value="review.human_decide";
                            input type="hidden" name="review_gate_id" value=(gate.id);
                            input type="hidden" name="return_session_id" value=(session.id);
                            input type="hidden" name="review_decision" value="partial_accept";
                            label { "保留的 Contribution ID（至少一个，但不能全部）" textarea name="selected_contribution_ids" rows="3" required { (candidate_id_list.as_str()) } }
                            label { "部分接受理由" textarea name="rationale" rows="2" required {} }
                            button class="button button--secondary button--small" type="submit" { "部分接受" }
                        }
                    }
                }
            }
        }
    }
}

fn session_action_panel(
    snapshot: &GoalGraphSnapshot,
    session: &GoalSessionRecord,
    contract: Option<&GoalContractVersionRecord>,
    current_contributions: &[&crate::goal_models::GoalContributionRecord],
) -> Markup {
    let branch_contributions = snapshot
        .contributions
        .iter()
        .filter(|item| item.goal_branch_id == session.goal_branch_id)
        .collect::<Vec<_>>();
    let contribution_ids = branch_contributions
        .iter()
        .map(|item| item.id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let blocking_proposal = snapshot.proposals.iter().any(|proposal| {
        proposal.parent_session_id == Some(session.id)
            && matches!(proposal.status.as_str(), "draft" | "awaiting_approval")
    });
    let active_child = snapshot.branches.iter().any(|item| {
        item.inherited_from_session_id == Some(session.id)
            && matches!(
                item.status.as_str(),
                "active" | "waiting" | "review_pending"
            )
    });
    let can_resume = matches!(
        session.status.as_str(),
        "waiting_branch_review"
            | "waiting_dependency"
            | "waiting_judgment"
            | "exception_paused"
            | "manual_paused"
    ) && !blocking_proposal
        && !active_child;

    html! {
        section class="session-actions" data-session-actions {
            header {
                div { span class="eyebrow" { "SESSION CONTROL" } h4 { "推进、拆分、暂停或收尾" } }
                small { "一条枝干同时只有这一个可写现场" }
            }
            @if session.status == "running" {
                div class="session-action-grid" {
                    details class="session-action" open[current_contributions.is_empty()] {
                        summary { "+ 记录 Contribution" }
                        form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                            input type="hidden" name="action" value="session.add_contribution";
                            input type="hidden" name="session_id" value=(session.id);
                            label { "类型"
                                select name="contribution_kind" {
                                    option value="code_change" { "代码变更" }
                                    option value="finding" { "发现 / 结论" }
                                    option value="evidence" { "证据 / 测试" }
                                    option value="artifact" { "文件 / 产物" }
                                    option value="decision" { "决定" }
                                    option value="condition" { "客观条件" }
                                    option value="other" { "其他" }
                                }
                            }
                            label { "标题" input name="title" required; }
                            label { "内容" textarea name="body" rows="3" required {} }
                            button class="button button--primary button--small" type="submit" { "保存可回流产出" }
                        }
                    }
                    details class="session-action" {
                        summary { "⑂ 拆出子目标" }
                        (proposal_editor(snapshot.project.id, "session.propose_child", None, Some(session.id), None))
                    }
                    details class="session-action" {
                        summary { "? 请你做方向 / 品味判断" }
                        form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                            input type="hidden" name="action" value="session.request_judgment";
                            input type="hidden" name="session_id" value=(session.id);
                            label { "需要你判断什么？" textarea name="question" rows="3" required {} }
                            label { "候选（每行一项）" textarea name="candidates" rows="2" {} }
                            label { "现有证据" textarea name="evidence" rows="2" {} }
                            label { "AI 建议" textarea name="recommendation" rows="2" {} }
                            button class="button button--secondary button--small" type="submit" { "保存现场并暂停" }
                        }
                    }
                    details class="session-action" {
                        summary { "! 报告异常并安全暂停" }
                        form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                            input type="hidden" name="action" value="session.pause_exception";
                            input type="hidden" name="session_id" value=(session.id);
                            label { "异常原因" textarea name="reason" rows="2" required {} }
                            label { "已保存到哪个安全点" textarea name="safe_checkpoint" rows="2" required {} }
                            label { "已尝试什么" textarea name="attempted" rows="2" required {} }
                            label { "继续的风险" textarea name="risk" rows="2" required {} }
                            label { "需要你做什么" textarea name="user_action" rows="2" required {} }
                            label { "AI 建议处理" textarea name="recommendation" rows="2" required {} }
                            button class="button button--secondary button--small" type="submit" { "标记异常暂停" }
                        }
                    }
                    details class="session-action" {
                        summary { "Ⅱ 你手动暂停这个 Session" }
                        form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                            input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                            input type="hidden" name="action" value="session.pause_manual";
                            input type="hidden" name="session_id" value=(session.id);
                            label { "暂停原因" textarea name="reason" rows="2" required {} }
                            button class="button button--secondary button--small" type="submit" { "暂停" }
                        }
                    }
                    details class="session-action session-action--merge" open[!current_contributions.is_empty()] {
                        summary { "→ Agent 声明整条目标枝干已达成" }
                        @if branch_contributions.is_empty() {
                            p class="worksite-empty-copy" { "至少要先记录一项 Contribution。" }
                        } @else {
                            form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                                input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                                input type="hidden" name="action" value="merge.propose";
                                input type="hidden" name="session_id" value=(session.id);
                                input type="hidden" name="contract_version_id" value=(contract.map(|item| item.id).unwrap_or(session.contract_version_id));
                                input type="hidden" name="contribution_ids" value=(contribution_ids.as_str());
                                div class="candidate-contributions" {
                                    b { "冻结以下 Contribution" }
                                    @for item in &branch_contributions { span { (&item.title) } }
                                }
                                label { "测试 / 浏览器证据（每行一项）" textarea name="test_evidence" rows="3" required {} }
                                label { "已知风险（每行一项，可留空）" textarea name="risks" rows="2" {} }
                                label { "Agent 逐条契约自查" textarea name="self_check" rows="3" required {} }
                                button class="button button--primary" type="submit" { "冻结现场并进入拟合并审核" }
                            }
                        }
                    }
                }
            } @else if can_resume {
                div class="resume-card" {
                    div { b { "现在可以显式恢复" } p { "填写你的判断或问题处理结果，再交还这条枝干的写权。" } }
                    form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="action" value="session.resume";
                        input type="hidden" name="session_id" value=(session.id);
                        label { "解决说明" textarea name="resolution" rows="3" required {} }
                        button class="button button--primary" type="submit" { "恢复 Session" }
                    }
                }
            } @else if session.status == "review_rejected" {
                div class="resume-card" {
                    div { b { "已退回，在同一目标枝干继续" } p { "上一 Session 保持不变；新 Session 继承契约、上下文与固定工具环境。" } }
                    form class="goal-form" method="post" action=(format!("/projects/{}/goal-commands", snapshot.project.id)) {
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());
                        input type="hidden" name="action" value="session.start_next";
                        input type="hidden" name="goal_branch_id" value=(session.goal_branch_id);
                        input type="hidden" name="previous_session_id" value=(session.id);
                        label { "下一 Session 分配说明" textarea name="assignment" rows="3" required {} }
                        label { "Agent 身份（可选）" input name="agent_identity" value=(session.agent_identity.as_deref().unwrap_or("")); }
                        button class="button button--primary" type="submit" { "创建下一 Session" }
                    }
                }
            } @else {
                div class="session-terminal-note" {
                    b { (goal_status_label(&session.status)) }
                    p {
                        @if blocking_proposal { "先在上方决定子目标 BranchProposal。" }
                        @else if active_child { "子目标仍在推进或审核，父 Session 保持只读。" }
                        @else if session.status == "awaiting_merge_review" { "候选现场已冻结，只能审核，不能继续写入。" }
                        @else { "这个 Session 已结束或当前不可写。" }
                    }
                }
            }
        }
    }
}

fn latest_proposal_revision<'a>(
    snapshot: &'a GoalGraphSnapshot,
    proposal: &GoalProposalRecord,
) -> Option<&'a GoalProposalRevisionRecord> {
    snapshot
        .proposal_revisions
        .iter()
        .find(|item| item.proposal_id == proposal.id && item.revision == proposal.current_revision)
}

fn candidate_contribution_ids(gate: &GoalReviewGateRecord) -> Vec<Uuid> {
    gate.candidate_snapshot
        .0
        .get("contributionIds")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|value| Uuid::parse_str(value).ok())
        .collect()
}

fn json_value_strings(value: &serde_json::Value) -> Vec<&str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect()
}

fn json_value_lines(value: &serde_json::Value) -> String {
    json_value_strings(value).join("\n")
}

fn json_value_len(value: &serde_json::Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or_default()
}

fn json_array_len(value: &serde_json::Value, key: &str) -> usize {
    value.get(key).map(json_value_len).unwrap_or_default()
}

fn goal_status_label(status: &str) -> &str {
    match status {
        "draft" => "草案",
        "awaiting_approval" => "等待你批准",
        "approved" => "已批准",
        "cancelled" => "已取消",
        "active" | "running" => "推进中",
        "waiting" | "waiting_branch_review" | "waiting_dependency" => "等待子目标",
        "waiting_judgment" => "等待你判断",
        "exception_paused" => "异常暂停",
        "manual_paused" => "手动暂停",
        "review_pending" | "awaiting_merge_review" => "拟合并审核",
        "review_rejected" | "rejected" => "已退回",
        "accepted" => "已接受",
        "integrated" => "已回流父目标",
        "completed" => "已完成",
        "stopped" => "已停止",
        "archived" => "已归档",
        _ => status,
    }
}

fn goal_status_tone(status: &str) -> &str {
    match status {
        "active" | "running" | "approved" | "accepted" | "integrated" | "completed"
        | "succeeded" => "active",
        "waiting"
        | "waiting_branch_review"
        | "waiting_dependency"
        | "waiting_judgment"
        | "review_pending"
        | "awaiting_merge_review"
        | "pending_ai_review"
        | "pending_human_review" => "waiting",
        "exception_paused" | "review_rejected" | "rejected" | "failed" => "danger",
        "manual_paused" | "stopped" | "archived" | "cancelled" => "muted",
        _ => "shaping",
    }
}

fn proposal_status_label(status: &str) -> &str {
    goal_status_label(status)
}

fn review_status_label(status: &str) -> &str {
    match status {
        "pending_ai_review" => "等独立 AI",
        "pending_human_review" => "等你决定",
        "accepted" => "已接受",
        "partially_accepted" => "部分接受",
        "rejected" => "已退回",
        "abandoned" => "已放弃",
        "withdrawn" => "已撤回",
        _ => status,
    }
}

fn review_actor_label(actor: &str) -> &str {
    match actor {
        "review_ai" => "独立审核 AI",
        "human" => "你的最终决定",
        _ => actor,
    }
}

fn review_decision_label(decision: &str) -> &str {
    match decision {
        "recommend_accept" => "建议接受",
        "recommend_reject" => "建议退回",
        "accept" => "接受",
        "partial_accept" => "部分接受",
        "reject" => "退回",
        "abandon" => "放弃",
        _ => decision,
    }
}

fn attention_kind_label(kind: &str) -> &str {
    match kind {
        "branch_review" => "拟分枝审核",
        "judgment" => "方向判断",
        "exception" => "异常处理",
        "manual_pause" => "手动暂停",
        "merge_review" => "拟合并审核",
        _ => kind,
    }
}

fn goal_contribution_kind_label(kind: &str) -> &str {
    match kind {
        "code_change" => "代码变更",
        "finding" => "发现",
        "evidence" => "证据",
        "artifact" => "产物",
        "decision" => "决定",
        "condition" => "条件",
        _ => "其他",
    }
}

fn input_status_label(status: &str) -> &str {
    match status {
        "staging" => "上传中",
        "verified" => "已验证",
        "available" => "可导入",
        "imported" => "已导入",
        "rejected" => "已拒绝",
        "quarantined" => "已隔离",
        _ => status,
    }
}

fn import_mode_label(mode: &str) -> &str {
    match mode {
        "worktree_copy" => "worktree 副本",
        "artifact_reference" => "产物引用",
        "read_only_mount" => "只读挂载",
        _ => mode,
    }
}

fn tool_status_label(status: &str) -> &str {
    match status {
        "succeeded" => "成功",
        "failed" => "失败",
        "timed_out" => "超时",
        "cancelled" => "已取消",
        "workspace_conflict" => "工作区冲突",
        "policy_denied" => "策略拒绝",
        _ => status,
    }
}

fn goal_short_title(value: &str, max: usize) -> String {
    let value = value.trim();
    if value.chars().count() <= max {
        return value.to_owned();
    }
    value
        .chars()
        .take(max.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

fn graph_view(snapshot: &ProjectSnapshot, requested_node: Option<Uuid>) -> Markup {
    let layout = GraphLayout::new(&snapshot.branches, &snapshot.nodes, &snapshot.edges);
    let selected_id = requested_node
        .filter(|id| snapshot.nodes.iter().any(|node| node.id == *id))
        .or_else(|| {
            snapshot
                .branches
                .iter()
                .find(|branch| branch.is_main == 1)
                .and_then(|branch| branch.head_node_id)
        })
        .or_else(|| snapshot.nodes.last().map(|node| node.id));
    let selected = selected_id.and_then(|id| snapshot.nodes.iter().find(|node| node.id == id));
    let branch_map: HashMap<Uuid, &ProjectBranch> = snapshot
        .branches
        .iter()
        .map(|branch| (branch.id, branch))
        .collect();
    let node_map: HashMap<Uuid, &ProjectNode> =
        snapshot.nodes.iter().map(|node| (node.id, node)).collect();
    let width = 430 + (layout.max_depth + 1) * 250;
    let height = 145 + snapshot.branches.len().max(1) * 156;

    html! {
        section id="graph-workspace" class="evolution-workspace" {
            header class="evolution-toolbar" {
                div {
                    span class="eyebrow" { "PROJECT EVOLUTION" }
                    h2 { "项目脉络" small { (snapshot.branches.len()) " 条分支 · " (snapshot.nodes.len()) " 个节点" } }
                }
                div class="graph-legend" aria-label="节点结果图例" {
                    span { i class="legend-dot legend-dot--useful" {} "有效" }
                    span { i class="legend-dot legend-dot--refuted" {} "反证" }
                    span { i class="legend-dot legend-dot--blocked" {} "等待" }
                    span { i class="legend-dot legend-dot--open" {} "探索" }
                }
            }
            div class="evolution-layout" {
                div class="graph-scroll" data-graph-scroll aria-label="项目分支和节点" {
                    div class="graph-stage" style=(format!("width:{width}px;height:{height}px")) {
                        @for (lane, branch) in snapshot.branches.iter().enumerate() {
                            div class=(if branch.is_main == 1 { "branch-lane branch-lane--main" } else { "branch-lane" })
                                style=(format!("top:{}px", 62 + lane * 156)) {
                                div class="branch-lane__label" {
                                    i style=(format!("background:{}", safe_color(&branch.color))) {}
                                    strong { (&branch.name) }
                                    small { (branch_status_label(&branch.status)) }
                                }
                            }
                        }
                        svg class="graph-edges" width=(width) height=(height) aria-hidden="true" {
                            @for edge in &snapshot.edges {
                                @if let (Some(from), Some(to)) = (layout.positions.get(&edge.parent_node_id), layout.positions.get(&edge.child_node_id)) {
                                    @let x1 = 224 + from.depth * 250;
                                    @let y1 = 92 + from.lane * 156;
                                    @let x2 = 224 + to.depth * 250;
                                    @let y2 = 92 + to.lane * 156;
                                    @let curve = ((x2.saturating_sub(x1)) as f32 * 0.46).max(48.0) as usize;
                                    @let stroke = node_map.get(&edge.child_node_id)
                                        .and_then(|node| branch_map.get(&node.branch_id))
                                        .map(|branch| safe_color(&branch.color))
                                        .unwrap_or("#91b546");
                                    path class=(format!("graph-edge graph-edge--{}", edge.relation))
                                        d=(format!("M {x1} {y1} C {} {y1}, {} {y2}, {x2} {y2}", x1 + curve, x2.saturating_sub(curve)))
                                        style=(format!("stroke:{stroke}")) {}
                                }
                            }
                        }
                        @for node in &snapshot.nodes {
                            @if let Some(position) = layout.positions.get(&node.id) {
                                @let branch = branch_map.get(&node.branch_id).copied();
                                @let output_count = output_count(snapshot, node.id);
                                @let selected_class = if Some(node.id) == selected_id { " is-selected" } else { "" };
                                @let head_class = if branch.and_then(|item| item.head_node_id) == Some(node.id) { " is-head" } else { "" };
                                a class=(format!("graph-node graph-node--{}{}{}", node.outcome, selected_class, head_class))
                                    data-selected=(if Some(node.id) == selected_id { "true" } else { "false" })
                                    href=(format!("/projects/{}?tab=graph&node={}#graph-workspace", snapshot.project.id, node.id))
                                    style=(format!("left:{}px;top:{}px", 206 + position.depth * 250, 74 + position.lane * 156)) {
                                    span class="graph-node__dot" style=(format!("border-color:{}", branch.map(|item| safe_color(&item.color)).unwrap_or("#91b546"))) {}
                                    span class="graph-node__card" {
                                        span class="graph-node__meta" {
                                            (node_kind_label(&node.kind))
                                            @if output_count > 0 { " · " (output_count) " 项产出" }
                                            @if branch.and_then(|item| item.head_node_id) == Some(node.id) { b { "HEAD" } }
                                        }
                                        strong { (&node.title) }
                                        small { (outcome_label(&node.outcome)) }
                                    }
                                }
                            }
                        }
                    }
                    ol class="graph-mobile-list" {
                        @for node in &snapshot.nodes {
                            @let branch = branch_map.get(&node.branch_id).copied();
                            li {
                                a class=(if Some(node.id) == selected_id { "is-selected" } else { "" })
                                    href=(format!("/projects/{}?tab=graph&node={}#graph-workspace", snapshot.project.id, node.id)) {
                                    i style=(format!("background:{}", branch.map(|item| safe_color(&item.color)).unwrap_or("#91b546"))) {}
                                    span { small { (branch.map(|item| item.name.as_str()).unwrap_or("分支")) " · " (node_kind_label(&node.kind)) } strong { (&node.title) } }
                                }
                            }
                        }
                    }
                }
                aside class="node-inspector" {
                    @if let Some(node) = selected {
                        (node_inspector(snapshot, node, branch_map.get(&node.branch_id).copied()))
                    } @else {
                        div class="empty-compact" { "选择一个节点，查看它留下的产出和结论。" }
                    }
                }
            }
        }
    }
}

fn node_inspector(
    snapshot: &ProjectSnapshot,
    node: &ProjectNode,
    branch: Option<&ProjectBranch>,
) -> Markup {
    let contributions: Vec<&ProjectContribution> = snapshot
        .contributions
        .iter()
        .filter(|item| item.node_id == node.id)
        .collect();
    let merge = snapshot
        .merges
        .iter()
        .find(|merge| merge.result_node_id == node.id);
    let merged_outputs: Vec<&ProjectContribution> = merge
        .map(|merge| {
            snapshot
                .contributions
                .iter()
                .filter(|item| merge.accepted_contribution_ids.0.contains(&item.id))
                .collect()
        })
        .unwrap_or_default();
    let artifacts: Vec<_> = snapshot
        .artifacts
        .iter()
        .filter(|item| item.node_id == Some(node.id))
        .collect();
    let evidence: Vec<_> = snapshot
        .evidence
        .iter()
        .filter(|item| item.node_id == Some(node.id))
        .collect();
    let decisions: Vec<_> = snapshot
        .decisions
        .iter()
        .filter(|item| item.node_id == Some(node.id))
        .collect();
    let output_total = contributions.len()
        + merged_outputs.len()
        + artifacts.len()
        + evidence.len()
        + decisions.len();
    let is_head = branch.and_then(|item| item.head_node_id) == Some(node.id);
    let candidates: Vec<_> = branch
        .map(|branch| {
            let branch_nodes: HashSet<Uuid> = snapshot
                .nodes
                .iter()
                .filter(|item| item.branch_id == branch.id)
                .map(|item| item.id)
                .collect();
            snapshot
                .contributions
                .iter()
                .filter(|item| item.status == "candidate" && branch_nodes.contains(&item.node_id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let accepted_ids = candidates
        .iter()
        .map(|item| item.id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    html! {
        div class="node-inspector__head" {
            span style=(format!("color:{}", branch.map(|item| safe_color(&item.color)).unwrap_or("#91b546"))) {
                (branch.map(|item| item.name.as_str()).unwrap_or("项目"))
            }
            time { (format_date_time(node.created_at)) }
        }
        div class="node-inspector__tags" {
            span { (node_kind_label(&node.kind)) }
            span class=(format!("outcome-tag outcome-tag--{}", node.outcome)) { (outcome_label(&node.outcome)) }
        }
        h3 { (&node.title) }
        p class="node-summary" { (&node.summary) }

        div class="node-outputs" {
            h4 { "这个节点留下了什么" }
            @if output_total == 0 {
                p class="node-outputs__empty" { "还没有正式产出。过程日志不会自动冒充项目进展。" }
            }
            @for item in contributions {
                (contribution_card(item, false))
            }
            @for item in merged_outputs {
                (contribution_card(item, true))
            }
            @for artifact in artifacts {
                article {
                    div { span { "正式文件" } small { (&artifact.status) } }
                    strong { (&artifact.title) }
                    a href=(format!("/artifacts/{}", artifact.id)) target="_blank" { "打开文件 ↗" }
                }
            }
            @for item in evidence {
                article { div { span { "证据" } } p { (&item.summary) } }
            }
            @for item in decisions {
                article { div { span { "决策" } } p { (&item.rationale) } }
            }
        }

        div class="node-actions" {
            details class="action-disclosure" {
                summary { "+ 从这里开一条新分支" }
                form class="graph-form" method="post" action=(format!("/projects/{}/graph", snapshot.project.id)) {
                    input type="hidden" name="action" value="create_branch";
                    input type="hidden" name="node_id" value=(node.id);
                    label for="branch-purpose" { "这条路要弄清或做成什么？" }
                    textarea id="branch-purpose" name="purpose" rows="4" required {}
                    button class="button button--primary button--small" type="submit" { "创建分支" }
                }
            }
            @if is_head && branch.is_some_and(|item| can_append_to_branch(&item.status)) {
                details class="action-disclosure" {
                    summary { "+ 记录这条路的新进展" }
                    (append_progress_form(snapshot.project.id, branch.expect("checked")))
                }
            }
            @if is_head && branch.is_some_and(|item| item.is_main != 1 && can_append_to_branch(&item.status)) {
                details class="action-disclosure" {
                    summary { "↳ 把所得带回主线" }
                    form class="graph-form" method="post" action=(format!("/projects/{}/graph", snapshot.project.id)) {
                        input type="hidden" name="action" value="integrate_branch";
                        input type="hidden" name="branch_id" value=(branch.expect("checked").id);
                        input type="hidden" name="accepted_ids" value=(accepted_ids);
                        fieldset class="merge-selection" {
                            legend { "将带回这些正式产出" }
                            @if candidates.is_empty() {
                                p { "这条分支还没有正式产出，先记录一次真实进展。" }
                            }
                            @for item in &candidates {
                                div { strong { (&item.title) } small { (contribution_kind_label(&item.kind)) } }
                            }
                        }
                        label for="merge-summary" { "主线以后应该记住什么？" }
                        textarea id="merge-summary" name="summary" rows="4" required {}
                        button class="button button--primary button--small" type="submit" disabled[candidates.is_empty()] { "确认带回主线" }
                    }
                }
                details class="action-disclosure" {
                    summary { "◇ 等条件变化后再试" }
                    form class="graph-form" method="post" action=(format!("/projects/{}/graph", snapshot.project.id)) {
                        input type="hidden" name="action" value="park_branch";
                        input type="hidden" name="branch_id" value=(branch.expect("checked").id);
                        label for="park-reason" { "当前为什么不继续？" }
                        textarea id="park-reason" name="reason" rows="3" required {}
                        label for="park-reopen" { "什么变化后值得重试？" }
                        textarea id="park-reopen" name="reopen_when" rows="2" {}
                        button class="button button--secondary button--small" type="submit" { "保存为等待条件" }
                    }
                }
            }
        }
    }
}

fn append_progress_form(project_id: Uuid, branch: &ProjectBranch) -> Markup {
    html! {
        form class="graph-form" method="post" action=(format!("/projects/{project_id}/graph")) {
            input type="hidden" name="action" value="append_progress";
            input type="hidden" name="branch_id" value=(branch.id);
            label for="progress-result" { "实际发生了什么？" }
            textarea id="progress-result" name="result" rows="5" required
                placeholder="可以写完成了什么，也可以写：原以为可行，但测试结果表明……" {}
            details class="advanced-fields" {
                summary { "需要时调整产出类型、判断和重试条件" }
                div class="form-row" {
                    label { "这次主要留下"
                        select name="kind" {
                            option value="finding" { "结论 / 经验" }
                            option value="artifact" { "文件 / 代码 / 内容" }
                            option value="evidence" { "证据 / 反馈" }
                            option value="decision" { "决定" }
                            option value="condition" { "条件变化" }
                            option value="other" { "其他产出" }
                        }
                    }
                    label { "结果判断"
                        select name="outcome" {
                            option value="useful" { "产生有效增量" }
                            option value="open" { "仍在探索" }
                            option value="refuted" { "原判断不成立" }
                            option value="mixed" { "部分成立" }
                            option value="blocked" { "条件不满足" }
                            option value="inconclusive" { "证据仍不足" }
                        }
                    }
                }
                input name="reference_uri" placeholder="文件、代码仓库或网页地址（可选）";
                textarea name="scope" rows="2" placeholder="这个结论在哪些条件下成立？（可选）" {}
                textarea name="reopen_when" rows="2" placeholder="什么变化后值得再试？（可选）" {}
            }
            button class="button button--primary button--small" type="submit" { "形成进展节点" }
        }
    }
}

fn contribution_card(item: &ProjectContribution, merged: bool) -> Markup {
    html! {
        article class=(if merged { "merged-output" } else { "" }) {
            div {
                span {
                    @if merged { "从分支带回 · " }
                    (contribution_kind_label(&item.kind))
                }
                small { (if item.status == "accepted" { "已进入主线认识" } else { "仅在当前分支" }) }
            }
            strong { (&item.title) }
            p { (&item.body) }
            @if let Some(scope) = &item.scope { p { b { "适用范围：" } (scope) } }
            @if let Some(reopen) = &item.reopen_when { p { b { "值得重试：" } (reopen) } }
            @if let Some(uri) = safe_uri(item.reference_uri.as_deref()) {
                a href=(uri) target="_blank" rel="noreferrer" { "打开关联内容 ↗" }
            }
        }
    }
}

fn output_count(snapshot: &ProjectSnapshot, node_id: Uuid) -> usize {
    snapshot
        .contributions
        .iter()
        .filter(|item| item.node_id == node_id)
        .count()
        + snapshot
            .artifacts
            .iter()
            .filter(|item| item.node_id == Some(node_id))
            .count()
        + snapshot
            .evidence
            .iter()
            .filter(|item| item.node_id == Some(node_id))
            .count()
        + snapshot
            .decisions
            .iter()
            .filter(|item| item.node_id == Some(node_id))
            .count()
}

struct GraphLayout {
    positions: HashMap<Uuid, NodePosition>,
    max_depth: usize,
}

#[derive(Clone, Copy)]
struct NodePosition {
    depth: usize,
    lane: usize,
}

impl GraphLayout {
    fn new(
        branches: &[ProjectBranch],
        nodes: &[ProjectNode],
        edges: &[crate::models::ProjectNodeEdge],
    ) -> Self {
        let lanes: HashMap<Uuid, usize> = branches
            .iter()
            .enumerate()
            .map(|(lane, branch)| (branch.id, lane))
            .collect();
        let node_ids: HashSet<Uuid> = nodes.iter().map(|node| node.id).collect();
        let mut parents: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for edge in edges {
            parents
                .entry(edge.child_node_id)
                .or_default()
                .push(edge.parent_node_id);
        }
        let mut depths = HashMap::new();
        for node in nodes {
            let mut visiting = HashSet::new();
            calculate_depth(node.id, &parents, &node_ids, &mut depths, &mut visiting);
        }
        let mut max_depth = 0;
        let positions = nodes
            .iter()
            .map(|node| {
                let depth = depths.get(&node.id).copied().unwrap_or_default();
                max_depth = max_depth.max(depth);
                (
                    node.id,
                    NodePosition {
                        depth,
                        lane: lanes.get(&node.branch_id).copied().unwrap_or_default(),
                    },
                )
            })
            .collect();
        Self {
            positions,
            max_depth,
        }
    }
}

fn calculate_depth(
    node_id: Uuid,
    parents: &HashMap<Uuid, Vec<Uuid>>,
    node_ids: &HashSet<Uuid>,
    depths: &mut HashMap<Uuid, usize>,
    visiting: &mut HashSet<Uuid>,
) -> usize {
    if let Some(depth) = depths.get(&node_id) {
        return *depth;
    }
    if !visiting.insert(node_id) {
        return 0;
    }
    let valid_parents = parents
        .get(&node_id)
        .into_iter()
        .flatten()
        .filter(|parent| node_ids.contains(parent));
    let depth = valid_parents
        .map(|parent| calculate_depth(*parent, parents, node_ids, depths, visiting) + 1)
        .max()
        .unwrap_or_default();
    visiting.remove(&node_id);
    depths.insert(node_id, depth);
    depth
}

fn layout(title: &str, active: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="zh-CN" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, viewport-fit=cover";
                meta name="theme-color" content="#f6f6f2" media="(prefers-color-scheme: light)";
                meta name="theme-color" content="#141612" media="(prefers-color-scheme: dark)";
                title { (title) " · 浮点" }
                meta name="description" content="以现实结果为目标、以证据更新决策的长期项目工作区。";
                link rel="icon" href="/assets/icon.svg";
                link rel="manifest" href="/assets/manifest.webmanifest";
                link rel="stylesheet" href="/assets/app.css";
                script src="/assets/theme.js" {}
                script src="/assets/app.js" defer {}
            }
            body {
                div class="app-shell" {
                    (sidebar(active))
                    main class="app-main" { div class="page-container" { (content) } }
                    (mobile_nav(active))
                }
            }
        }
    }
}

fn sidebar(active: &str) -> Markup {
    html! {
        aside class="sidebar" {
            a class="brand" href="/" aria-label="浮点首页" {
                (brand_mark()) strong { "浮点" }
            }
            nav class="side-nav" aria-label="主导航" {
                a class=(if active == "ideas" { "is-active" } else { "" }) href="/ideas" { span { "∿" } b { "想法" } }
                a class=(if active == "projects" { "is-active" } else { "" }) href="/" { span { "⌘" } b { "项目" } }
                a class=(if active == "attention" { "is-active" } else { "" }) href="/?view=attention" { span { "◌" } b { "待处理" } }
                a class=(if active == "artifacts" { "is-active" } else { "" }) href="/?view=artifacts" { span { "◇" } b { "产物" } }
            }
            div class="sidebar__bottom" {
                div class="rust-badge" { span { "R" } div { strong { "Rust edition" } small { "可恢复单体" } } }
                button class="theme-toggle" type="button" data-theme-toggle="" aria-label="切换浅色或深色模式" {
                    span class="theme-toggle__dark" { "☾" }
                    span class="theme-toggle__light" { "☀" }
                    b { "切换主题" }
                }
            }
        }
    }
}

fn mobile_nav(active: &str) -> Markup {
    html! {
        nav class="mobile-nav" aria-label="移动端导航" {
            a class=(if active == "ideas" { "is-active" } else { "" }) href="/ideas" { span { "∿" } b { "想法" } }
            a class=(if active == "projects" { "is-active" } else { "" }) href="/" { span { "⌘" } b { "项目" } }
            a class=(if active == "attention" { "is-active" } else { "" }) href="/?view=attention" { span { "◌" } b { "待处理" } }
            a class="mobile-nav__new" href="/ideas/new" { span { "+" } b { "记录" } }
            a class=(if active == "artifacts" { "is-active" } else { "" }) href="/?view=artifacts" { span { "◇" } b { "产物" } }
            button type="button" data-theme-toggle="" { span { "◐" } b { "主题" } }
        }
    }
}

fn brand_mark() -> Markup {
    html! {
        span class="brand-mark" aria-hidden="true" { i {} i {} i {} }
    }
}

fn state_tone(state: &str) -> &str {
    match state {
        "active" | "completed" => "active",
        "waiting" | "paused" => "waiting",
        "stopped" | "archived" => "muted",
        _ => "shaping",
    }
}

fn idea_state_label(state: &str) -> &str {
    match state {
        "captured" => "刚记录",
        "developing" => "发展中",
        "proposed" => "已提议立项",
        "promoted" => "已形成项目",
        "archived" => "已归档",
        _ => state,
    }
}

fn idea_state_tone(state: &str) -> &str {
    match state {
        "captured" => "captured",
        "developing" => "developing",
        "proposed" => "proposed",
        "promoted" => "promoted",
        _ => "muted",
    }
}

fn proposal_state_label(state: &str) -> &str {
    match state {
        "draft" => "草案",
        "awaiting_approval" => "等待你批准",
        "approved" => "已批准立项",
        "rejected" => "已退回",
        "cancelled" => "已取消",
        _ => state,
    }
}

fn proposal_state_tone(state: &str) -> &str {
    match state {
        "awaiting_approval" => "waiting",
        "approved" => "active",
        "rejected" | "cancelled" => "muted",
        _ => "draft",
    }
}

fn idea_relation_label(relation: &str) -> &str {
    match relation {
        "supports" => "支持",
        "contradicts" => "矛盾",
        "depends_on" => "依赖",
        "duplicates" => "近似重复",
        _ => "有关联",
    }
}

fn source_kind_label(kind: &str) -> &str {
    match kind {
        "file" => "文件来源",
        "image" => "图片来源",
        "audio" => "语音来源",
        "external" => "外部来源",
        _ => "文字记录",
    }
}

fn action_status_label(status: &str) -> &str {
    match status {
        "ready" => "可以开始",
        "running" => "执行中",
        "blocked" => "受阻",
        "completed" => "已完成",
        "failed" => "失败",
        _ => status,
    }
}

fn event_label(event_type: &str) -> &str {
    match event_type {
        "project.created" => "创建项目起点",
        "project.intent.revised" => "修订项目意图",
        "outcome_contract.confirmed" => "确认成果契约",
        "artifact.project_brief.created" => "生成项目启动说明",
        "artifact.project_brief.approved" => "接受项目启动说明",
        "graph.branch.created" => "展开探索分支",
        "graph.node.created" => "记录真实进展",
        "graph.branch.integrated" => "将分支所得带回主线",
        "graph.branch.waiting" => "保存等待条件",
        _ => event_type,
    }
}

fn actor_label(actor: &str) -> &str {
    match actor {
        "human" => "用户",
        "agent" => "浮点",
        "system" => "系统",
        _ => actor,
    }
}

fn event_actor_icon(actor: &str) -> &str {
    match actor {
        "human" => "你",
        "agent" => "✦",
        _ => "·",
    }
}

fn format_date(value: chrono::DateTime<chrono::Utc>) -> String {
    value.format("%m月%d日").to_string()
}

fn format_date_time(value: chrono::DateTime<chrono::Utc>) -> String {
    value.format("%m月%d日 %H:%M").to_string()
}

fn safe_color(value: &str) -> &str {
    if value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        value
    } else {
        "#91b546"
    }
}

fn safe_uri(value: Option<&str>) -> Option<&str> {
    value.filter(|uri| uri.starts_with("http://") || uri.starts_with("https://"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn layout_escapes_project_titles() {
        let projects = vec![ProjectSummary {
            id: Uuid::new_v4(),
            title: "<script>alert(1)</script>".into(),
            intent: "安全渲染用户输入".into(),
            state: "active".into(),
            current_focus: None,
            updated_at: Utc::now(),
            contract_status: Some("confirmed".into()),
            attention_count: 1,
            artifact_count: 1,
        }];
        let rendered = dashboard(&projects, None).into_string();
        assert!(!rendered.contains("<script>alert(1)</script>"));
        assert!(rendered.contains("&lt;script&gt;"));
    }

    #[test]
    fn colors_and_external_links_are_allowlisted() {
        assert_eq!(safe_color("#c8f36c"), "#c8f36c");
        assert_eq!(safe_color("red;display:none"), "#91b546");
        assert!(safe_uri(Some("javascript:alert(1)")).is_none());
    }
}
