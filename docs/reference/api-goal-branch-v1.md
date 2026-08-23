# 目标枝干 HTTP API v1

> 状态：已实现参考。

本接口是 [`goal-branch-domain.md`](../architecture/goal-branch-domain.md) 的第一条真实纵向实现。旧 `/api/projects/:id/graph` 继续读取旧探索图；新接口不会把旧分支或节点自动解释成 GoalBranch。

## 查询

`GET /api/v1/projects/:project_id/goal-graph`

返回 `modelVersion: "goal-branch-v2"`，以及 Project、Proposal 修订、契约版本/修订请求/来源、GoalBranch、Session、Contribution、Evidence、ReviewGate/Decision、Integration、AttentionItem 与有序 Event。URL 保持 v1 是为了兼容现有客户端；`modelVersion` 明确表达增量快照结构。它是领域快照，不规定前端必须怎样画图。

## 命令

`POST /api/v1/projects/:project_id/goal-commands`

```json
{
  "clientRequestId": "uuid",
  "action": "proposal.create",
  "payload": {}
}
```

每个写请求必须有 UUID 幂等键。完全相同的重放返回原结果并带 `replayed: true`；同一键更换 action 或 payload 返回 HTTP 409 / `idempotency_conflict`。状态冲突返回 409，输入错误返回 422，不存在返回 404。

当前命令：

| action | 用途 |
| --- | --- |
| `proposal.create` | 创建根目标 Proposal 草稿 |
| `proposal.revise` | 基于准确修订号产生新修订 |
| `proposal.submit` | 达到最低充分明确度后提交用户审核 |
| `proposal.cancel` | 用户取消尚未批准的 Proposal |
| `proposal.approve` | 用户批准并原子创建枝干、契约 v1 与首个 Session |
| `session.propose_child` | 工作 Agent 提出子目标并暂停父 Session |
| `session.add_contribution` | running Session 登记不可变 Contribution |
| `session.add_evidence` | running Session 登记带哈希和来源的不可变 Evidence |
| `contract.propose_revision` | 基于活动契约生成不可变候选、字段差异和来源 |
| `contract.accept_revision` | 用户接受候选，切换活动契约并安全暂停 running Session |
| `contract.reject_revision` | 用户拒绝候选，保留审计且不改变活动契约 |
| `session.request_judgment` | 请求用户品味/科研判断并创建待处理项 |
| `session.pause_exception` | 带完整诊断信息异常暂停 |
| `session.pause_manual` | 用户手动暂停 |
| `session.resume` | 解决待处理后显式恢复同一 Session |
| `session.start_next` | 拟合并被退回后在同一枝干创建下一 Session |
| `session.stop` | 用户明确停止 Session/枝干，保留未达成结论 |
| `merge.propose` | 冻结 Contribution、Git、环境与证据候选 |
| `merge.withdraw` | 发现反例后撤回冻结候选，要求下一 Session |
| `review.ai_record` | 仅兼容没有物理 workspace 的旧 Gate；真实候选拒绝表单代录 |
| `review.human_decide` | 用户接受、部分接受、退回或放弃 |
| `goal_branch.archive` | 用户归档终态枝干并保留归档前结论 |

真实候选由 `merge.propose` 自动产生 `review.goal_candidate.v1` ActionRun。Review Worker 通过 `/api/v1/scheduler/claim` 获得 ActionLease，只读复验后向 `/api/v1/scheduler/action-runs/:id/complete` 提交绑定候选摘要、HEAD、tree、workspace snapshot、环境、契约检查、反例与隔离证明的报告。Worker 身份来自调度器注册信息，不能与工作 Agent 相同；错误摘要、错误观察值和旧 fencing token 均拒绝。

`review.human_decide` 完整接受时必须选择冻结候选中的全部 Contribution；部分接受必须选择非空真子集；退回和放弃不得选择 Contribution。对子枝干，接受只创建 `pending` Integration 和 `integration.goal_branch.v1` ActionRun，源枝干仍是 `review_pending`。Integration Worker 先调用 `.../integrations/:integration_id/prepare` 形成只读父候选，再用绑定父契约回归的报告调用 `.../finalize`。只有 Git CAS、父 worktree/snapshot、上下文和数据库同时确认后，子枝干才成为 `integrated`/`stopped`；根枝干不创建虚假 Integration，用户接受后直接完成。完整协议见 [`review-integration-v1.md`](../archive/branch-proposals/review-integration-v1.md)。

契约中的 `exploration` 支持 `delivery | exploration | hybrid`。探索/混合模式还必须提供 `budgets`、`candidateOutputs` 和 `uncertaintyReduction`；否则在 Proposal 批准或契约修订时返回 422 / `insufficient_exploration_contract`。完整语义见 [`core-domain-v2.md`](../architecture/core-domain-v2.md)。

## Session 上下文

上下文采用独立的渐进式披露接口，完整语义见 [`context-memory-v1.md`](../architecture/context-memory-v1.md)：

| 方法 | 路径 | 语义 |
| --- | --- | --- |
| `GET` | `/api/v1/projects/:project_id/sessions/:session_id/context` | 不可折叠信封、实时状态、默认目录与遗漏数 |
| `GET` | `.../context/entries?query=&sourceKind=&offset=&limit=` | 检索和分页完整来源目录 |
| `POST` | `.../context/read` | 审计式读取 `summary`、`snippet` 或 `full` |
| `POST` | `.../context/rebuild` | 从权威来源只追加派生数据新代 |

按需读取和重建必须带 UUID 幂等键及准确 `snapshotId`。快照过期、目录外来源、摘要不一致或同一请求 ID 更换参数均返回 409/422，不会猜测性读取。读取审计不保存原文。

## HTML 表单适配

`POST /projects/:project_id/goal-commands` 接收：

- `client_request_id`：可选 UUID；没有时服务器生成；
- `action`：与 JSON API 相同；
- `payload`：JSON 字符串。

表单和 JSON API 调用同一个事务应用服务。这个低层表单入口用于无 JavaScript 回退与后续服务端页面；最终工作台会在按需披露的交互层之上生成这些命令，不要求用户手写 JSON。

## 当前安全说明

恢复出的当前版本还没有身份认证，因此这些接口只能在本机或受信任私人网络使用。服务端仍按动作固定 Actor 权限：工作/审核 AI 只能建议，根目标完成和最终接受只走 human 决定；未来认证层负责把真实身份绑定到这些 Actor，不能改变领域权限。
