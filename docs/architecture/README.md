# 实现架构

> 状态：已实现快照。这里描述当前代码和测试证明的边界，不替代仍在讨论的产品设计。

- [总体架构](architecture.md)：部署单元、数据、一致性、安全和测试边界
- [目标枝干领域](goal-branch-domain.md)：聚合、状态机和全局不变量
- [目标契约与生命周期](core-domain-v2.md)：探索、Evidence、候选和终态
- [想法与项目](idea-project-domain-v1.md)：Idea、ProjectProposal 和来源
- [上下文与记忆](context-memory-v1.md)：继承、目录、渐进式披露和审计

具体接口字段放在[接口参考](../reference/README.md)。已完成 BranchProposal 的目标契约和验收证据保存在[历史归档](../archive/README.md)，避免把过去的实施计划误当作现行架构。

当前实现及本目录仍使用 `GoalContractVersion` 名称；目标产品已经决定将其视为 GoalBranch 内部的“目标说明版本”，迁移边界见[决策 0013](../decisions/0013-goal-definition-is-branch-version.md)。

当前实现把 ReviewGate、ReviewDecision、Integration 和对应 ActionRun 分开持久化；目标产品把它们视为同一 MergeGate 的内部记录，见[决策 0014](../decisions/0014-single-merge-gate-aggregate.md)。

当前实现使用独立 `goal_attention_items` 保存待处理状态；目标产品改为直接汇总 Session、目标草稿、MergeGate 和工具请求自身的等待状态，见[决策 0015](../decisions/0015-attention-is-derived-view.md)。

当前实现仍有独立的 Proposal 提交、审批路由和视图；目标交互改为在来源对话中持续修订并接受准确版本，见[决策 0016](../decisions/0016-inline-versioned-proposal-conversation.md)。
