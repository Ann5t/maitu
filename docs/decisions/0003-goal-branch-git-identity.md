# 0003：目标枝干与 Git branch 一一对应

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

目标枝干、Agent Session、Git branch、commit 和 worktree 如果没有稳定映射，界面会产生一套无法由真实文件历史核验的“项目图”；如果把每个 Session 都做成 branch，又会把一条目标的连续工作错误拆散。

## 备选方案

- GoalBranch 只存在于数据库，按需共享任意 Git branch。
- 每个 Agent Session 创建独立 Git branch。
- 一个 GoalBranch 固定一个 Git branch，多个 Session 在其上连续接力。

## 决定

每个获批的正式 GoalBranch 固定拥有一个 Git branch。根 GoalBranch 使用项目默认分支；子 GoalBranch 从创建时父 Session 的准确安全 commit 分出。BranchProposal 在批准前不创建 branch 或可写 worktree。

同一 GoalBranch 的所有 Session 依次使用这一条 branch。后一个 Session 从前一个 Session 的最终安全 commit 开始；一次进程崩溃和恢复仍属于原 Session。审核退回后，新 Session 默认从被冻结候选继续修改，需要放弃内容时使用新提交或 revert，不重写已经审查过的历史。

Git branch 是持久身份，worktree 是可释放和重建的执行现场。Session 交接前必须把有意义变化落到干净的检查点 commit。一个 Session 可以对应零到多个 commit，二者不合并为同一类节点。

## 影响

- 数据库的 GoalBranch 父子关系表达目标语义；Git ref 和 commit 表达文件事实，二者通过稳定映射连接。
- 暂停或归档可以回收 worktree 空间，但不得删除仍用于审计或恢复的 Git ref。
- 当前 worktree 生命周期与 Session 交接实现需要按此规则审计；旧数据库记录和 Git 历史继续保留。
- 父子枝干并行、同步和最终冲突处理需要单独决策。

## 相关资料

- [产品设计：GoalBranch](../product/product-design.md#42-goalbranch)
- [标准 Git 整枝合并决策](0002-standard-git-branch-integration.md)
- [Git worktree 官方文档](https://git-scm.com/docs/git-worktree)
