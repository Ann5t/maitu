# 架构决策记录

架构决策记录（ADR）保存重要选择的背景和理由。编号单调递增，文件使用 `NNNN-short-title.md`。

- [0000：模板](0000-template.md)
- [0001：文档信息架构](0001-documentation-information-architecture.md)
- [0002：目标枝干只通过标准 Git 合并回流](0002-standard-git-branch-integration.md)
- [0003：目标枝干与 Git branch 一一对应](0003-goal-branch-git-identity.md)
- [0004：只有叶子目标枝干运行工作 Agent](0004-leaf-only-agent-execution.md)
- [0005：获批子枝干立即合入父枝干](0005-merge-approved-child-immediately.md)
- [0006：父枝干更新按需披露给运行中的子枝干](0006-parent-update-disclosure.md)
- [0007：项目文件使用 Git 与标准 Git LFS](0007-standard-git-lfs-for-project-files.md)
- [0008：在菱形 Merge Gate 中由 AI 处理整合](0008-ai-assisted-merge-gate.md)
- [0009：终态目标枝干保留 Git ref 并释放 worktree](0009-retain-terminal-ref-release-worktree.md)
- [0010：每个项目使用独立 Git 仓库，不设总 Git](0010-one-repository-per-project.md)
- [0011：持久文件只进入项目 Git 或 Git LFS](0011-retained-files-live-in-project-git.md)
- [0012：成果摘要属于拟合并候选，不设独立 Contribution](0012-merge-candidate-outcome-summary.md)
- [0013：目标说明是 GoalBranch 的内部版本](0013-goal-definition-is-branch-version.md)
- [0014：一次拟合并只对应一个 MergeGate](0014-single-merge-gate-aggregate.md)
- [0015：待我处理是来源状态的汇总视图](0015-attention-is-derived-view.md)
- [0016：Proposal 是原对话中的版本化提案卡](0016-inline-versioned-proposal-conversation.md)

状态使用 `Proposed`、`Accepted`、`Rejected`、`Superseded`。已接受记录保持追加式；改变方向时创建新记录并链接被替代记录。
