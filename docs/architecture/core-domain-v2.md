# 目标契约、Evidence 与执行生命周期 v2

> 状态：已实现领域快照；产品语义仍可能经用户讨论修订。

- 状态：逻辑领域闭环已通过隔离验证
- 领域快照：`modelVersion: "goal-branch-v2"`
- 增量迁移：`0007_goal_domain_v2.sql`
- 上位语义：[`product-design.md`](../product/product-design.md)
- 详细状态机：[`goal-branch-domain.md`](goal-branch-domain.md)

## 这一步解决什么

本阶段把“目标枝干只是可演示流程”收紧成可审计的逻辑执行核心：目标执行中不能静默改契约；效果暂时无法精确验收时可以正式选择探索模式；事实、产出和审核候选之间有稳定来源关系；撤回、停止和归档保留真实结论，不会冒充完成。

它没有宣称已经完成物理 Git、worktree、Runner、后台 Worker 或认证。数据库里的 Git 字段在相应阶段实现前仍会明确保持 `null` / `not_attempted`。

## 契约模式

`GoalContractVersion` 是不可变版本，除原有结果、约束、偏好、未知、验证、判断时机、停止条件和期望贡献外，新增 `explorationPolicy`：

| 模式 | 用途 | 批准条件 |
| --- | --- | --- |
| `delivery` | 结果与验收已相对明确 | 至少有验证/判断边界和停止条件 |
| `exploration` | 先减少未知，再由用户判断方向 | 另需探索预算、候选产出和不确定性收敛方式 |
| `hybrid` | 一部分明确交付，一部分并行探索 | 同探索模式 |

探索预算不是要求用户填写庞大计划。它可以是一句数量、时间、资源或判断边界，例如“最多做两个可操作候选后暂停”。表单仍只先展示目标、原因、验证和停止四类最少信息；模式和细节在高级区域按需披露。

## 执行中修订契约

1. Agent 或用户调用 `contract.propose_revision`，基于准确的当前版本生成一个不可变候选版本。
2. 系统计算逐字段差异，并为变化字段记录 `GoalContractProvenance`。来源明确区分用户输入、Agent 推断、外部资料、继承契约、Artifact 和 Evidence。
3. 活动契约保持不变，站内产生一个持久 `contract_review` 待处理项。
4. 只有用户能接受或拒绝。拒绝保留候选及理由但不切换活动版本；接受会切换活动版本，并把正在运行的 Session 停在安全暂停点。
5. 用户显式恢复后，Session 才能在新契约下继续。旧版本、差异、决定和事件均不可覆盖。

契约若没有任何真实差异会被拒绝；枝干已有未决修订时不能再叠加另一个候选，也不能进入拟合并审核。

## Evidence 与冻结候选

`GoalEvidence` 是独立于叙述性 `Contribution` 的结构化事实记录，包含：

- 支持、反驳、阻塞或上下文立场；
- 声明与实际观察；
- 测试、浏览器、外部来源、Artifact、ToolCall 或研究类型；
- 可选来源 URI、Artifact / ToolCall 精确引用；
- 验证状态、内容哈希、记录者和时间。

Evidence 创建后不可原地修改。`GoalContributionEvidence` 把结论绑定到证据，`GoalReviewGateEvidence` 把证据冻结进拟合并候选。冻结后继续写 Contribution 或 Evidence 会返回 `candidate_frozen`；发现反例时只能撤回 Gate，保留旧候选并在同一目标枝干创建下一 Session。

## 收尾语义

- `merge.withdraw`：候选不再可信，旧 Gate 标为 `withdrawn`，旧 Session 标为 `review_rejected`；下一轮必须创建新 Session。
- `session.stop`：仅用户可执行；有未决子目标、Proposal、契约修订或 Gate 时拒绝停止。停止保留负面结论，根目标同步令 Project 进入 `stopped`。
- `goal_branch.archive`：仅用户可对 `integrated | completed | stopped` 枝干执行；`archivedFromStatus` 永久保留归档前结论。
- 接受契约、停止、归档、拟合并与审核均带幂等请求 ID，并在同一事务追加事件。

## 数据库约束

迁移 0007 以只增不减方式增加契约修订、决定、来源、Evidence 及其冻结关联。触发器阻止：

- 跨 Project / GoalBranch / Session 的引用拼接；
- 修改不可变决定、来源、Evidence 和冻结关联；
- 删除契约修订审计，或在没有匹配人工决定时把修订标成接受/拒绝；
- 篡改冻结 Gate、跳过独立 AI 审核状态，或伪造枝干归档前结论；
- Proposal 的父 Session 与父枝干不一致；
- 一个枝干同时存在多个未决契约修订；
- 非法修改契约修订身份或从终态退回等待态。

迁移在空库、重复重放以及带旧 DAG fixture 的数据库上通过；旧项目、旧枝干和旧节点计数保持不变。

## 已验证流程

隔离测试已覆盖：

- 35 个 Rust 单元测试与 Clippy `-D warnings`；
- 7 个迁移的顺序执行、重复执行、旧数据增量升级和跨聚合约束；
- 契约候选拒绝、探索型候选接受、接受时暂停和显式恢复；
- Evidence 创建、Contribution 绑定、Gate 冻结和冻结后写入拒绝；
- Gate 撤回、两个并发 `session.start_next` 只有一个获得写权；
- 明确停止、终态归档和根 Project 不冒充完成；
- Chromium 桌面、平板和手机的契约差异决定、Evidence 和拟合并流程。

完整 `scripts/quality-gate.sh` 已从头通过。随后只读复核真实现场仍为 `3:8:23:2`，迁移登记仍只有原有 4 条，两个正式 Artifact 的数据库 SHA-256 与卷内文件逐项一致；原应用/数据库容器 ID 和启动时间均未改变，所有一次性测试容器已清理。测试始终使用一次性数据库、随机回环端口和隔离文件目录。

## 后续边界

- BP-03：上下文目录、来源图、摘要/全文加载与固定窗口验证；
- BP-04：真实 Git branch/worktree、单写 Lease 和隔离 Runner；
- BP-06：ActionRun、持久队列、心跳、fencing 与重启恢复；
- BP-07：独立审核 Worker、物理 Git/环境绑定及父枝干整合复验；
- BP-09：认证、授权、CSRF、HTTPS 和完整恢复演练。
