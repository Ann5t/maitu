# 五个旧仓库的来源盘点

这里保留迁移前的事实快照，用于核对来源和避免遗漏；当前不定义脉图功能已经完成，也不授权删除旧仓库。现行流程见[迁移与删除准备](../../operations/legacy-repository-migration.md)。

## 2026 年 10 月 1 日快照

[机器可读盘点](2026-10-01-inventory.json)保存五库的全部镜像 refs、分支头和文件树、可达提交数、默认分支外的提交数、Git LFS 指针与分支子模块检查，以及 GitHub 附属资料的盘点状态。

来源为用户电脑已有凭据访问的 GitHub API 与 `git clone --mirror`。每个镜像均为非浅克隆，并通过 `git fsck --full --strict`。原库没有被修改。本机镜像只用于盘点，未作为 Maitu 中已发布的迁移档案，也未通过空目录恢复验收。

Git 盘点覆盖当时服务端可见 refs 及其可达对象，不包含不可访问或已被清理的对象。Git LFS 扫描核对可达 blob 中的标准指针；子模块检查核对保存 refs 对应文件树中的 gitlink。镜像不包含 PR 讨论、Wiki 和原电脑项目的未提交文件、数据库、配置及运行成果；这些资料仍按现行迁移流程另行核对。

盘点 JSON 只保存来源元数据，不包含真实密钥、源码正文或用户运行数据。五个仓库的 `deletionReady` 均为 `false`；日后发布归档和执行恢复时追加新的事实快照，保留本次历史记录。

## 2026 年 10 月 2 日迁移快照与恢复验收

迁移已完成：五库完整 bundle 与附属资料归档发布于 Release [`legacy-sources-2026-10-02`](https://github.com/Ann5t/maitu/releases/tag/legacy-sources-2026-10-02)，来源与去向见[迁移清单](2026-10-02-migration-manifest.md)，空目录独立恢复验收与末尾增量核对见[恢复验收记录](2026-10-02-restore-verification.md)。两次快照 refs 逐条一致。五库具备删除条件，删除决定由用户作出。
