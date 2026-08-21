use std::collections::{HashMap, HashSet};

use maud::{DOCTYPE, Markup, html};
use uuid::Uuid;

use crate::{
    domain::{
        branch_status_label, can_append_to_branch, contribution_kind_label, node_kind_label,
        outcome_label, state_label,
    },
    models::{
        ActionRun, ProjectBranch, ProjectContribution, ProjectNode, ProjectSnapshot, ProjectSummary,
    },
};

use super::handlers::ProjectPageQuery;

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
            a class="button button--primary" href="/new" { span aria-hidden="true" { "+" } " 新项目" }
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
                    a class="button button--primary" href="/new" { "创建第一个项目" }
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

pub fn project(snapshot: &ProjectSnapshot, query: &ProjectPageQuery) -> Markup {
    let tab = query.tab.as_deref().unwrap_or("graph");
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
                (project_tab(snapshot.project.id, "graph", "脉络", tab, snapshot.nodes.len()))
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

        (current_action_panel(snapshot))

        @match tab {
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
            a class=(if active == "projects" { "is-active" } else { "" }) href="/" { span { "⌘" } b { "项目" } }
            a class=(if active == "attention" { "is-active" } else { "" }) href="/?view=attention" { span { "◌" } b { "待处理" } }
            a class="mobile-nav__new" href="/new" { span { "+" } b { "新项目" } }
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
