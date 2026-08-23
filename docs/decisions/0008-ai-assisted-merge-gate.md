# 0008：在菱形 Merge Gate 中由 AI 处理整合

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

普通 Git 文件和 Git LFS 文件都可能被多个兄弟枝干修改。全局文件锁会阻止有意的并行探索，也不能解决所有语义冲突；只看 `git merge` 的退出状态又无法证明合并后的项目仍然正确。

当前实现根据用户选中的 Contribution 对提交执行 `cherry-pick --no-commit`，冲突时直接暂停。它既不符合完整枝干 merge 的已接受决定，也没有可写的 AI 整合候选阶段。

## 备选方案

- 为所有可能冲突的文件建立跨枝干独占锁。
- Git 冲突一律交给用户手工处理。
- 在隔离候选中由 Integration Agent 解决，独立 AI复核，用户最终批准。

## 决定

普通 Git 与 Git LFS 默认不使用 Fudian 自定义跨枝干文件锁。系统可以提前提示相同路径被多个活动枝干修改，但正常允许并行，最终统一进入枝干回流处的菱形 Merge Gate。

Merge Gate 固定冻结子 HEAD、父预期 HEAD、目标契约、环境和证据。系统从父 HEAD 创建隔离整合 worktree，执行 `git merge --no-ff`。Integration Agent 可以写这一候选 worktree，但不能直接移动父或子 ref；完成冲突处理和验证后形成具有两个 parent 的标准 merge commit。

另一个只读的独立 Review AI 必须检查最终 diff、冲突选择、契约和测试。只有用户接受最终候选，系统才以 compare-and-swap 更新父 branch；父 HEAD 已变化时，旧候选失效并重新形成。

文本冲突使用 Git 三方材料解决。Git LFS 只负责内容存储：有对应格式插件时可以尝试语义合并；没有时只能明确选择父版本、子版本、两份并存或人工重制，不能声称已经自动融合。若冲突处理扩大为独立目标，撤回候选并回到原子枝干建立新 Session 或子目标。

## 影响

- 没有 Git 冲突的合并也必须构建、测试和审查，以发现语义不兼容。
- Merge Gate 是图上的整合门和受控 Agent Run，不是新的 GoalBranch，也不是父枝干普通工作 Agent恢复。
- 特定不可合并文件若以后产生真实重复浪费，可以单独评估标准 Git LFS locking；这不是第一版默认依赖。
- 当前选择性 cherry-pick 和冲突即暂停的实现必须迁移；旧 Integration 审计记录继续保留。

## 相关资料

- 后续术语决定：[目标说明是 GoalBranch 的内部版本](0013-goal-definition-is-branch-version.md)。本记录中的“目标契约”对应候选冻结时的当前目标说明版本。
- [标准 Git 整枝合并](0002-standard-git-branch-integration.md)
- [项目文件使用 Git 与标准 Git LFS](0007-standard-git-lfs-for-project-files.md)
- [当前整合实现](../../src/application/workspaces.rs)
- [Git LFS Locking API](https://github.com/git-lfs/git-lfs/blob/main/docs/api/locking.md)
