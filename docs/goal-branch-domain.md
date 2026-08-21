# 目标枝干领域与状态机 v0.1

- 状态：里程碑 1 的实现契约
- 上位语义：[`product-design.md`](product-design.md)
- 适用实现：`feat/goal-branch-core-v0.1`

## 1. 边界与命名

本文把产品设计中的稳定语义收敛为数据库、Rust 领域层和 HTTP 应用服务可以共同执行的约束。它不决定项目图的具体布局，也不决定暂停、审核和合并事件最终显示成节点、门还是边标记。

新模型与恢复出的旧图模型并存。为避免把旧 `project_branches` 误解成 GoalBranch，本文中的新表和事件统一使用 `goal_` 前缀；Rust 类型使用完整领域名称。

## 2. 聚合与权威记录

### 2.1 Project

沿用 `projects.id` 作为项目身份和所有新聚合的外键。项目状态仍是项目生命周期的权威记录；AI 只能提出根枝干候选完成，只有用户决定后才能把项目置为 `completed`。

### 2.2 BranchProposal

`BranchProposal` 是正式枝干出现前的可版本化讨论对象，不是 Session 节点，也没有可写 worktree。

身份记录包含：

- `id`、`project_id`；
- 可选的 `parent_goal_branch_id` 与 `parent_session_id`；
- `status`：`draft | awaiting_approval | approved | cancelled`；
- 当前修订号、批准后生成的 `approved_goal_branch_id`；
- 创建者、时间戳。

每次修改产生不可变 `BranchProposalRevision`，至少包含：

- 为什么需要该枝干；
- 目标契约草稿；
- 已知约束、明确未知；
- 期望回流的 Contribution；
- 探索方法与停止方式；
- 请求用户判断的时机；
- 继承上下文点与工具需求；
- 对 AI 推断的显式标记。

批准永远绑定一个准确的修订号。批准后的 Proposal 和修订不可修改。

### 2.3 GoalContractVersion

目标契约版本是不可变值，包含：

- `desired_outcome`：想达到的结果；
- `hard_constraints`：不能突破的边界；
- `subjective_preferences`：需要用户品味判断的偏好；
- `unknowns`：当前诚实未知的问题；
- `non_goals`：明确不做；
- `validation_plan`：机器验证、人工判断或探索方法；
- `judgment_triggers`：何时必须找用户；
- `stop_conditions`：完成、预算用尽或安全停止条件；
- `expected_contributions`：准备回流的成果类型。

每条 GoalBranch 恰好有一个当前 `active` 版本。新版本只能由提案产生并经用户确认；执行 Agent 不得原地修改、降低或替换当前契约。

### 2.4 GoalBranch

`GoalBranch` 表示一个目标的完整枝干，包含：

- 项目、父 GoalBranch 与创建它的 Proposal；
- 从父 Session 冻结的继承点；
- 当前目标契约版本；
- Git 分支/worktree 的逻辑身份与基线提交；
- 环境清单引用；
- Session 序列、子枝干和当前状态。

状态为：

| 状态 | 含义 |
| --- | --- |
| `active` | 可以开始或继续一个可写 Session |
| `waiting` | 因审查、判断、依赖或异常而暂停 |
| `review_pending` | 候选现场已冻结，等待独立 AI/用户审核 |
| `integrated` | 子枝干目标被用户接受，选中贡献已回流 |
| `completed` | 根枝干被用户确认完成 |
| `stopped` | 用户放弃、部分接受或安全终止；不冒充完成 |
| `archived` | 仅存储/展示状态，不表示成败 |

根枝干的 `parent_goal_branch_id` 为空。子枝干必须来自已批准的 Proposal，且其父 Session 与父枝干一致。

### 2.5 AgentSessionNode

Session 是枝干主图上的主要工作节点。它保存分配的一轮工作和可恢复执行身份；进程重启不会创建新 Session。

状态为：

| 状态 | 是否可写 | 含义 |
| --- | ---: | --- |
| `running` | 是 | 当前唯一工作 Agent 可以修改本枝干 worktree |
| `waiting_branch_review` | 否 | 已提出子枝干，等待用户审查 Proposal |
| `waiting_dependency` | 否 | 已批准的子枝干或其他外部依赖尚未回流 |
| `waiting_judgment` | 否 | 等待用户直觉、品味或科研判断 |
| `exception_paused` | 否 | 工具、条件、预算或安全边界阻止继续 |
| `manual_paused` | 否 | 用户主动暂停 |
| `awaiting_merge_review` | 否 | 整条枝干被声明达成，候选现场已冻结 |
| `review_rejected` | 否 | 候选被退回；必须创建下一 Session 才能继续 |
| `accepted` | 否 | 候选被用户接受 |
| `stopped` | 否 | 本轮被明确终止 |

数据库必须用部分唯一索引保证每条 GoalBranch 最多一个 `running` Session。应用层也要在事务内锁定 GoalBranch；worktree/Runner 还需使用同一枝干的写租约，数据库约束不是完整沙箱。

### 2.6 Contribution、ReviewGate 与 Integration

`GoalContribution` 是 Session 产出的不可变回流候选，种类为 `artifact | finding | evidence | decision | condition | code_change | other`。修改内容会产生新记录并用 `supersedes_id` 关联旧版本。

`ReviewGate` 冻结一次拟合并候选，包含：

- 当前 Session、目标契约版本和候选 Contribution；
- Git 基线/头提交/脏状态摘要；
- EnvironmentManifest 指纹；
- 构建、测试、浏览器验证与证据摘要；
- 已知风险、未解决项和工作 Agent 自查。

Gate 状态为 `pending_ai_review | pending_human_review | accepted | partially_accepted | rejected | abandoned | withdrawn`。每个决定写入不可变 `ReviewDecision`：角色为 `review_ai | human`，结论为 `recommend_accept | recommend_reject | accept | partial_accept | reject | abandon | withdraw`。

独立审核 AI 只能给建议；用户决定才会产生 `GoalIntegration`。完整接受使子枝干为 `integrated`、根枝干为 `completed`。部分接受只回流用户选中的 Contribution，并把枝干置为 `stopped`，避免把未达成目标写成完成。

### 2.7 Event、AttentionItem 与 CommandReceipt

所有重要动作追加不可变 `GoalEvent`。事件至少有项目内顺序号、聚合类型/ID、事件类型、Actor、命令 ID、结构化载荷和发生时间。事件是审计事实，不是可以重建全部业务状态的唯一 Event Store；当前状态表与事件在同一事务写入。

所有暂停、拟分支和待审核状态必须创建持久 `AttentionItem`。它包含原因、最后安全检查点、已尝试办法、风险、用户需要做什么、建议动作和去重键。站外通知是后续适配器，站内记录不可省略。

所有状态命令都带 `client_request_id`。`CommandReceipt` 以 `(project_id, client_request_id)` 唯一，保存命令种类、规范化输入哈希和结果引用。同一个 ID 与相同输入重放时返回第一次结果；同一个 ID 配不同命令或输入时返回 `idempotency_conflict`。

## 3. 全局不可变量

1. Proposal 未批准时不存在由它产生的 GoalBranch、正式 worktree 或执行 Session。
2. 一条 GoalBranch 只表达一个目标；一个 Session 只属于一条 GoalBranch。
3. 每条 GoalBranch 同时最多一个 `running` Session；并行工作必须创建子 GoalBranch。
4. Session 只有在 `running` 时可以写 worktree、生成普通 Contribution 或发起状态出口。
5. `awaiting_merge_review` 的候选现场、Contribution 集、Git 头和环境指纹不可变。
6. 未被用户接受的 Gate 不得产生 Integration，也不得改变父枝干的上下文或 worktree。
7. 退回不会复用冻结 Session；后续修改发生在同一枝干的新 Session。
8. 子枝干接受不自动证明父目标完成；父枝干恢复后仍需整合验证。
9. 根枝干只有用户 `accept` 后才能令项目完成；AI 决定无此权限。
10. 目标契约、权限和硬边界不可被上下文摘要折叠或被执行 Agent 静默修改。
11. 所有状态变化与事件在同一数据库事务提交；失败时两者都不出现。
12. UI 只消费可替换的图投影，不从图形形状反推领域状态。

## 4. 通用命令规则

以下转换表中的幂等符号 `C` 都表示统一命令语义：必需 `client_request_id`，规范化输入哈希一致则重放原结果，不一致则冲突。除纯读取外不存在无幂等键的写接口。

每个命令还必须验证：Actor 对项目有权限；目标记录属于同一项目；引用的版本是当前版本；请求时间不用于判断正确性；事务锁定被修改的聚合；错误使用稳定机器码。

## 5. Proposal 转换

| 命令 | 当前 → 新状态 | 关键输入 | 前置条件 | 原子输出 | 事件 | 幂等 |
| --- | --- | --- | --- | --- | --- | --- |
| `proposal.create` | 不存在 → `draft` | 项目、可选父枝干/Session、首个修订 | 父引用一致；若为子目标，父 Session 必须 `running` | Proposal + revision 1 | `proposal.created` | C |
| `proposal.revise` | `draft`/`awaiting_approval` → `draft` | 基准修订号、完整新修订、理由 | 基准号仍为当前；批准/取消后禁止修改 | revision N+1，更新当前修订 | `proposal.revised` | C |
| `proposal.submit` | `draft` → `awaiting_approval` | 当前修订号、摘要 | 达到最低充分明确度；重大未知已列出 | 待处理项 | `proposal.submitted` | C |
| `session.propose_child` | 父 Session `running` → `waiting_branch_review` | 完整 Proposal 修订、暂停说明 | 没有其他未决子 Proposal；当前 Session 可写 | `awaiting_approval` Proposal + 待处理项；父枝干 `waiting` | `session.child_branch_proposed`、`proposal.submitted` | C |
| `proposal.approve` | `awaiting_approval` → `approved` | 准确修订号、用户决定、分支名称 | Actor 为用户；引用未漂移；父 Session 若存在须仍等待该 Proposal | active contract v1 + GoalBranch + 首个 `running` Session；子目标父 Session 转 `waiting_dependency` | `proposal.approved`、`goal_branch.created`、`session.started` | C |
| `proposal.cancel` | `draft`/`awaiting_approval` → `cancelled` | 原因 | 尚未批准 | 关闭待处理项；父 Session 保持暂停，等待显式恢复 | `proposal.cancelled` | C |

## 6. Session 暂停、恢复与继续

| 命令 | 当前 → 新状态 | 关键输入 | 前置条件 | 原子输出 | 事件 | 幂等 |
| --- | --- | --- | --- | --- | --- | --- |
| `session.request_judgment` | `running` → `waiting_judgment` | 要判断的问题、候选、证据、建议 | 问题会实质改变方向/品味判断 | 父枝干 `waiting` + 待处理项 | `session.judgment_requested` | C |
| `session.pause_exception` | `running` → `exception_paused` | 原因、检查点、尝试、风险、所需动作、恢复条件 | 无法在权限/安全边界内继续 | 父枝干 `waiting` + 待处理项 | `session.exception_paused` | C |
| `session.pause_manual` | 任一非终态、非审核态 → `manual_paused` | 用户原因 | Actor 为用户 | 父枝干 `waiting` + 待处理项 | `session.manual_paused` | C |
| `session.resume` | 任一可恢复暂停态 → `running` | 解决说明、可选判断结果 | 相关待处理已解决；依赖已接受/取消；无另一 running Session；枝干非终态 | 关闭待处理；父枝干 `active` | `session.resumed` | C |
| `session.start_next` | 无 running Session → 新 `running` Session | 分配说明、上一 Session、当前契约/环境 | 枝干 `active`；上一 Session 为 `review_rejected` 或显式结束；无未决 Gate | 新的序号递增 Session | `session.started` | C |
| `session.stop` | 非终态、非审核态 → `stopped` | 用户原因 | Actor 为用户，或已授权的安全停止 | 枝干 `stopped`；关闭/替换待处理项 | `session.stopped`、`goal_branch.stopped` | C |

`waiting_branch_review` 只有在对应 Proposal 被取消后才能恢复；批准后转成 `waiting_dependency`。依赖子枝干被接受、部分接受、停止或取消后，父 Session 仍由显式 `session.resume` 恢复，系统不会悄悄启动 Agent。

## 7. 拟合并与审核转换

| 命令 | 当前 → 新状态 | 关键输入 | 前置条件 | 原子输出 | 事件 | 幂等 |
| --- | --- | --- | --- | --- | --- | --- |
| `merge.propose` | Session `running` → `awaiting_merge_review` | Contribution IDs、冻结 Git/环境/测试/风险快照 | Agent 声明整个目标达成；当前契约一致；无未决必需子目标；候选均属本 Session/枝干 | `pending_ai_review` Gate；枝干 `review_pending`；审核待处理项 | `merge.proposed` | C |
| `review.ai_record` | `pending_ai_review` → `pending_human_review` | 结论、逐条契约检查、反例、复验结果 | reviewer 与工作 Agent 身份不同；候选哈希未变 | AI ReviewDecision；用户待处理项 | `review.ai_completed` | C |
| `review.human_accept` | `pending_human_review` → `accepted` | 选中全部/指定 Contribution、说明 | Actor 为用户；AI 审核已记录；冻结哈希未变 | Session `accepted`；子枝干 `integrated` 或根枝干/项目 `completed`；Integration；仅此时更新父上下文 | `review.accepted`、`contributions.integrated`、枝干/项目终态事件 | C |
| `review.human_partial` | `pending_human_review` → `partially_accepted` | 非空 Contribution 子集、未接受原因 | Actor 为用户；子集均在候选中 | Session `accepted`；枝干 `stopped`；选中项 Integration | `review.partially_accepted`、`goal_branch.stopped` | C |
| `review.human_reject` | `pending_human_review` → `rejected` | 退回理由、下一轮要求 | Actor 为用户 | Session `review_rejected`；枝干 `active` 但无 writer；创建继续待处理项；父枝干无变化 | `review.rejected` | C |
| `review.human_abandon` | `pending_human_review` → `abandoned` | 放弃理由 | Actor 为用户 | Session/枝干 `stopped`；不产生 Integration | `review.abandoned`、`goal_branch.stopped` | C |
| `merge.withdraw` | `pending_ai_review`/`pending_human_review` → `withdrawn` | 新发现问题与证据 | 工作 Agent 或审核 AI 发现候选不再可信 | Session `review_rejected`；枝干 `active`；新 Session 才能修改 | `merge.withdrawn` | C |

接受时创建的 Integration 只记录被选择的 Contribution、源/目标枝干、源 Gate、目标继承版本和实际 Git 集成状态。v0.1 可以先做到“领域接受与上下文可见”，不能伪称尚未执行的 Git merge 已完成。

## 8. 契约修订、停止与归档

- `contract.propose_revision` 可以从自然语言灵感生成一个不可变 `proposed` 版本，但不影响正在执行的版本。
- `contract.accept_revision` 只允许用户执行。若有 running Session，先把其安全暂停；新契约成为 active 后，显式恢复或开始下一 Session。
- `contract.reject_revision` 保留提案和理由，不改变 active 版本。
- `goal_branch.archive` 只允许终态枝干进入 `archived`；它不改变完成/停止的历史结论。
- 任何契约转换同样使用 C 幂等规则，并分别产生 `contract.revision_proposed | contract.revision_accepted | contract.revision_rejected | goal_branch.archived` 事件。

## 9. 错误语义

| 机器码 | 含义 |
| --- | --- |
| `invalid_state_transition` | 当前状态不允许该命令 |
| `stale_revision` | Proposal/契约/候选版本已变化 |
| `idempotency_conflict` | 同一请求 ID 携带不同语义 |
| `branch_writer_exists` | 目标枝干已有 running Session |
| `candidate_frozen` | 试图修改等待审核的冻结现场 |
| `unapproved_proposal` | 试图绕过 Proposal 批准创建枝干 |
| `cross_project_reference` | 引用记录不属于同一项目 |
| `unresolved_child_goal` | 必需子目标尚未接受、停止或豁免 |
| `human_authority_required` | AI 试图执行用户专属决定 |
| `independent_reviewer_required` | 审核者与工作 Agent 不是独立身份 |

HTTP 层把校验错误映射到 400、权限错误映射到 403、不存在映射到 404、版本/状态/幂等冲突映射到 409；内部错误不向客户端泄露 SQL、路径或秘密。

## 10. 旧模型共存策略

1. 不重命名、不删除、不回填 `project_branches`、`project_nodes`、`project_node_edges`、`project_contributions` 和 `branch_merges`。
2. 新表使用 `goal_branch_proposals`、`goal_branches`、`goal_sessions`、`goal_contributions`、`goal_review_gates` 等独立命名，仅复用 `projects` 身份。
3. 旧项目页面和旧 API 继续从旧表读取；新纵向流程使用版本化 `/api/v1/projects/:id/goal-*` 路径。
4. 现有项目可以创建一个无父 GoalBranch 的根 Proposal；旧主枝干不会被自动解释或复制成新 GoalBranch。
5. 新图快照返回明确的 `modelVersion: "goal-branch-v1"`。过渡期 UI 可以并列显示“旧探索图”和“目标枝干图”，不得把二者 ID 混用。
6. Artifact/Evidence 的旧表继续可读；新 Contribution 通过显式引用关联旧产物，不原地改变旧记录。
7. 数据迁移采用只增不减的 `0002_goal_branch_core.sql`。必须分别在空库和只含 `0001` 的隔离库验证；真实库只做只读基线核验。
8. 将来是否导入旧图必须由单独 Goal 决定，并保留来源映射、演练、回滚与用户审核，不属于 v0.1。

## 11. 图投影边界

领域查询返回枝干、Session、事件、Gate、Attention 与它们的稳定关系。投影器可以把事件渲染为菱形门、边标记、时间线卡片或可折叠细节，但必须满足：

- 枝干始终代表目标，主要工作节点始终代表 Session；
- Proposal 不伪装成第零号 Session；
- 拟合并与真正接受可被清楚区分；
- 暂停类型、待处理责任人和冻结状态可见；
- 改变布局不需要迁移领域数据。
