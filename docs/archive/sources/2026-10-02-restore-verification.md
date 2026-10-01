# 独立恢复验收记录（2026-10-02）

对应 Release [`legacy-sources-2026-10-02`](https://github.com/Ann5t/maitu/releases/tag/legacy-sources-2026-10-02)。验收过程与[迁移与删除准备](../../operations/legacy-repository-migration.md)一致：全新空目录 `restore-verify/`，从 Release 下载全部 9 件资产，**未借用旧仓库或本机镜像**。机器可读结果见迁移执行目录的 `restore-report.json` 与 `final-check.json`。

## 校验值核对

`sha256sum -c SHA256SUMS.txt`：9/9 件全部 OK（5 个 bundle、metadata-archives.tar.gz、manifest.json、RESTORE.md）。

## 逐库恢复结果

| 仓库 | bundle 校验 | 恢复克隆 | fsck --full --strict | refs 逐条一致 | 每条 ref 文件树可读 | 恢复提交数 |
| --- | --- | --- | --- | --- | --- | --- |
| fudian | complete history | ✓ | 通过 | ✓（2/2） | ✓ | 92 = 92 |
| project1 | complete history | ✓ | 通过 | ✓（52/52，含 19 条 PR refs） | ✓ | 40 = 40 |
| -Encore | complete history | ✓ | 通过 | ✓（5/5） | ✓ | 101 = 101 |
| personal-ai-hub | complete history | ✓ | 通过 | ✓（10/10） | ✓ | 200 = 200 |
| the-hive | complete history | ✓ | 通过 | ✓（1/1） | ✓ | 42 = 42 |

历史内容抽查：fudian 默认分支 README、-Encore 开发分支提交说明、the-hive 生产加固提交均可读取。

## 附属资料核对

`metadata-archives.tar.gz` 解压：project1 全部 19 条 PR 的正文/评论/审阅 JSON（19+19+19 件，全部可解析，pr-18 正文 1,575 字符可读）；5 份逐库盘点报告在列。LFS/子模块/Wiki/Release 的“本次未发现”结论与盘点报告一致。

## 迁入能力的使用验证

浮点主线（fudian 默认分支 79 条提交）作为脉图运行基线已随 PR #1 进入 `main` 并持续演进；本轮新增的多连接调度、取消、采用理由、过期输入与比较能力均为该基线上的真实实现，并已通过本机真实验收（见[实施进度](../../development/maitu-progress.md)）。project1/-Encore/personal-ai-hub/the-hive 的对应能力去向见[来源与去向清单](2026-10-02-migration-manifest.md)；原实现未被声称可直接启动。

## 末尾增量核对（2026-10-02，恢复验收之后执行）

重新认证读取五库 GitHub 的全部分支、标签、Issue/PR 与 Release：与迁移快照逐条比对，五库 refs **零新增、零缺失、零变更**；project1 的 19 条 PR 与其余四库的 0 条 Issue/PR、0 个 Release 保持一致。迁移期间原库无新增内容，无需补迁。

## 结论

五库满足[删除条件](../../operations/legacy-repository-migration.md)：产品日用验收（阶段 0–4）+ 全部可保留内容迁移 + 从 Release 的独立恢复验收 + 末尾增量核对。**删除 GitHub 仓库与清理本地目录由用户另行决定并执行。**
