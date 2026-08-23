# 0009：终态目标枝干保留 Git ref 并释放 worktree

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

目标枝干结束后继续保留物理 worktree 会重复占用普通文件和 Git LFS 检出内容；直接删除 Git ref 又会让未合并的停止枝干提交失去稳定可达路径，并削弱现场恢复和审计。

## 备选方案

- 永久保留所有 branch 和 worktree。
- 合并或停止后同时删除 branch 与 worktree。
- 保留只读 Git ref，释放可重建的物理 worktree。

## 决定

GoalBranch 进入终态后永久保留其领域记录和 Git ref。Fudian 将 ref 标记为不可继续写入，保持它指向枝干最终 HEAD；确认没有活动写租约、ToolLease 或整合候选后，释放物理 worktree。

Git 与 Git LFS 对象的保留和垃圾回收必须考虑这些终态 refs。需要查看现场时，从记录的 commit、Git ref 和 EnvironmentManifest 重建临时只读 worktree；需要继续工作时创建新的 GoalBranch，并记录旧枝干为来源，不重新激活终态枝干。

普通远端默认只同步项目默认分支。内部目标枝干只保存在 Fudian 托管仓库；用户可以显式选择导出活动枝干或完整审计 refs。

## 影响

- 终态目标保持可追溯，同时释放重复检出文件占用的空间。
- 停止但未合并的枝干提交仍然可达，不依赖 Git 的 reflog 保留期。
- worktree 路径不能被当作持久身份；所有恢复必须从 ref、commit 和环境记录验证。
- 当前实现需要补齐终态 ref 只读策略、worktree 回收和可重建验证；现有历史数据不删除。

## 相关资料

- [目标枝干与 Git branch 一一对应](0003-goal-branch-git-identity.md)
- [项目文件使用 Git 与标准 Git LFS](0007-standard-git-lfs-for-project-files.md)
