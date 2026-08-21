# 目标枝干 HTTP API v1

本接口是 [`goal-branch-domain.md`](goal-branch-domain.md) 的第一条真实纵向实现。旧 `/api/projects/:id/graph` 继续读取旧探索图；新接口不会把旧分支或节点自动解释成 GoalBranch。

## 查询

`GET /api/v1/projects/:project_id/goal-graph`

返回 `modelVersion: "goal-branch-v1"`，以及 Project、Proposal 修订、契约版本、GoalBranch、Session、Contribution、ReviewGate/Decision、Integration、AttentionItem 与有序 Event。它是领域快照，不规定前端必须怎样画图。

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
| `session.request_judgment` | 请求用户品味/科研判断并创建待处理项 |
| `session.pause_exception` | 带完整诊断信息异常暂停 |
| `session.pause_manual` | 用户手动暂停 |
| `session.resume` | 解决待处理后显式恢复同一 Session |
| `session.start_next` | 拟合并被退回后在同一枝干创建下一 Session |
| `merge.propose` | 冻结 Contribution、Git、环境与证据候选 |
| `review.ai_record` | 独立审核 AI 记录建议和复验 |
| `review.human_decide` | 用户接受、部分接受、退回或放弃 |

`review.human_decide` 完整接受时必须选择冻结候选中的全部 Contribution；部分接受必须选择非空真子集；退回和放弃不得选择 Contribution。领域接受只把选中贡献放入父上下文，Git 状态明确记录为 `not_attempted`，不会把尚未执行的物理 merge 冒充完成。

## HTML 表单适配

`POST /projects/:project_id/goal-commands` 接收：

- `client_request_id`：可选 UUID；没有时服务器生成；
- `action`：与 JSON API 相同；
- `payload`：JSON 字符串。

表单和 JSON API 调用同一个事务应用服务。这个低层表单入口用于无 JavaScript 回退与后续服务端页面；最终工作台会在按需披露的交互层之上生成这些命令，不要求用户手写 JSON。

## 当前安全说明

恢复出的当前版本还没有身份认证，因此这些接口只能在本机或受信任私人网络使用。服务端仍按动作固定 Actor 权限：工作/审核 AI 只能建议，根目标完成和最终接受只走 human 决定；未来认证层负责把真实身份绑定到这些 Actor，不能改变领域权限。
