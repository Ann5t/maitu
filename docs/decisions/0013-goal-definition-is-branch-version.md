# 0013：目标说明是 GoalBranch 的内部版本

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

此前把 `GoalBranch` 与 `GoalContract` 作为两个并列概念：前者保存枝干身份和执行历史，后者保存目标、验收、未知项与停止条件。用户实际面对的是同一个目标从讨论、批准到执行的连续过程；暴露两个并列对象会让人误以为开始目标后还要再创建或确认一份“合同”。

目标内容确实可能在执行期间变化，但这只要求保留说明版本，不要求建立第二个顶层对象。GoalBranch 的身份、Git branch 和 Session 历史应当在说明修订时保持稳定。

## 备选方案

- GoalBranch 与 GoalContract 继续作为两个并列的用户可见对象。
- 直接覆盖 GoalBranch 上的目标字段，不保留历史版本。
- GoalBranch 是唯一用户可见目标，内部持有不可变目标说明版本。

## 决定

用户只面对一个目标枝干。`GoalBranch` 保存稳定身份、父子关系、Git branch、Session 历史、状态和当前目标说明版本引用。

BranchProposal 在批准前保存目标说明草稿。用户批准开始目标时，系统一次性创建 GoalBranch、Git branch/worktree、首个 Session，并把获批草稿冻结为目标说明 v1；不再要求第二次 Contract 确认。

目标说明包含目标、约束、偏好、未知问题、验证或探索方式、用户判断时机和停止条件。执行期间的修改产生新的不可变说明版本；只有用户能接受并切换当前版本。旧版本继续绑定当时运行的 Session 和审核候选，不能被覆盖。

`GoalContract` 不再是目标产品中的独立名称。数据库可以使用单独表保存版本，但它属于 GoalBranch 聚合内部，在界面、图和普通 API 中以“目标说明”呈现。

## 影响

- 用户从草稿到执行只操作一个目标，不会遇到 Proposal 批准后再次确认 Contract 的重复流程。
- 目标变化不会更换 Git branch 或丢失已经发生的 Session 历史。
- 拟合并候选必须冻结准确的目标说明版本，之后的说明修订不能悄悄改变旧候选标准。
- 当前 `goal_contract_versions`、Contract 修订 API、事件名和前端术语是待迁移的兼容实现。旧数据继续可读，已发布迁移不改写。

## 相关资料

- [目标枝干与 Git branch 一一对应](0003-goal-branch-git-identity.md)
- [在菱形 Merge Gate 中由 AI 处理整合](0008-ai-assisted-merge-gate.md)
- [产品设计：GoalBranch](../product/product-design.md#42-goalbranch)
