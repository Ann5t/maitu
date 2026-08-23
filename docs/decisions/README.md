# 架构决策记录

架构决策记录（ADR）保存重要选择的背景和理由。编号单调递增，文件使用 `NNNN-short-title.md`。

- [0000：模板](0000-template.md)
- [0001：文档信息架构](0001-documentation-information-architecture.md)
- [0002：目标枝干只通过标准 Git 合并回流](0002-standard-git-branch-integration.md)
- [0003：目标枝干与 Git branch 一一对应](0003-goal-branch-git-identity.md)
- [0004：只有叶子目标枝干运行工作 Agent](0004-leaf-only-agent-execution.md)
- [0005：获批子枝干立即合入父枝干](0005-merge-approved-child-immediately.md)

状态使用 `Proposed`、`Accepted`、`Rejected`、`Superseded`。已接受记录保持追加式；改变方向时创建新记录并链接被替代记录。
