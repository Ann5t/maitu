# 0002：目标枝干只通过标准 Git 合并回流

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

现有实现把用户选中的 `Contribution` 映射为若干提交，再以 `cherry-pick --no-commit` 在父枝干上构造新的整合提交。这让 Fudian 在 Git 之外又定义了一套文件成果选择语义，也与用户已经确定的“枝干只有 merge”不一致。

## 备选方案

- 保留选择性 Contribution 和 cherry-pick。
- 将枝干压缩为一个提交后 squash merge。
- 生成自定义补丁或对象指针并由 Fudian 应用。
- 使用完整子枝干和标准 Git merge。

## 决定

拥有 Git 工作区的子目标枝干只有在整体可接受时，才能以 `git merge --no-ff` 完整合入直接父枝干。审批阶段不 cherry-pick 提交、不拼接补丁，也不实现另一套文件合并协议。

部分接受只保存用户已经认可的内容和剩余反馈，不改变父枝干。工作继续留在原目标枝干，或在正式合并前先拆分成边界清楚的子目标；最终被接受的枝干仍以完整 merge 回流。

Git 是文件、提交、diff、冲突和合并结果的权威来源。Fudian 保存目标、Session、审核等领域状态，以及 Git ref 和对象 ID 的映射与冻结证据。Contribution 用于描述成果和来源，不再作为物理 Git 集成单位。

## 影响

- 每个合并结果用 merge commit 保留目标枝干边界，即使 Git 原本可以 fast-forward。
- 枝干提出拟合并前必须清理掉不准备回流的文件变化。
- 冲突在隔离整合 worktree 中按 Git 语义暴露；Fudian 负责暂停、整合 Session、验证和审核。
- 现有 `selected_commits`、`partial` 物理集成和 cherry-pick 实现成为迁移对象；旧审计记录继续保留，已发布迁移不改写。

## 相关资料

- 后续决定：[成果摘要属于拟合并候选，不设独立 Contribution](0012-merge-candidate-outcome-summary.md)，替代了本记录中继续保留独立 Contribution 描述对象的部分。
- [产品设计：拟合并与退回](../product/product-design.md#53-拟合并与退回)
- [当前选择性整合实现](../../src/application/workspaces.rs)
- [Git merge 官方文档](https://git-scm.com/docs/git-merge)
