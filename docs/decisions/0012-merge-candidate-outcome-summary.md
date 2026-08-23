# 0012：成果摘要属于拟合并候选，不设独立 Contribution

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

现有 `goal_contributions` 把代码变化、文件、结论、决定和条件建模成多个不可变回流对象，并允许审核时选择其中一部分。目标枝干已经改为整体审核和标准 `git merge --no-ff`；项目文件由 Git/Git LFS 保存，Evidence 负责结构化证明，因此 Contribution 不再承担独立合并或文件版本职责。

继续保留 Contribution 会让用户和 Agent 在 Git 内容、Session 总结、Contribution 与拟合并说明之间重复登记同一结果，并可能重新引入“挑选部分成果”的合并语义。

## 备选方案

- 保留多个独立 Contribution，并只取消它们的物理合并作用。
- 每条枝干结束时创建一个独立 Contribution。
- 删除独立 Contribution，把简短成果摘要冻结在拟合并候选中。

## 决定

目标模型不再包含独立 `Contribution`。BranchProposal 只描述期望结果；工作中的长篇结论和正式输出写入项目 Git/Git LFS，轻量进度写入 Session 事件。

工作 Agent 提出拟合并时自动生成简短的候选成果摘要。摘要作为该次冻结候选的一部分，可以包含多条结果及其 Git commit、仓库相对路径和 Evidence 引用，但每一项都没有独立生命周期或选择性合并语义，用户不需要填写表单。

审核退回后原候选保持冻结，新 Session 继续同一枝干；再次拟合并会产生新的完整候选和摘要。部分接受只形成反馈，不选择摘要子集，也不产生物理整合。枝干在拟合并前停止时，只保存 Session/GoalBranch 终止摘要。

父更新通知由真实 merge commit、被冻结的候选成果摘要和 Evidence 推导，不依赖 Contribution 对象。

## 影响

- 用户审核的是一条完整目标枝干，而不是一组可勾选成果。
- 详细内容以 Git 文件和 diff 为权威，简短摘要只服务于图、审核和上下文披露。
- 当前 `goal_contributions`、Contribution API、Evidence 关联表、上下文来源图和选择性审核字段均为待迁移的旧实现。迁移必须保留旧记录可读，不改写已发布迁移；新模型启用后停止产生新的 Contribution。
- MergeCandidate/ReviewGate 需要新增可冻结、可哈希的成果摘要及 Git/Evidence 引用。

## 相关资料

- [一次拟合并只对应一个 MergeGate](0014-single-merge-gate-aggregate.md)
- [目标枝干只通过标准 Git 合并回流](0002-standard-git-branch-integration.md)
- [持久文件只进入项目 Git 或 Git LFS](0011-retained-files-live-in-project-git.md)
- [产品设计：候选成果摘要与 Evidence](../product/product-design.md#45-候选成果摘要与-evidence)
