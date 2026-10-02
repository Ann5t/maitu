# 五个旧仓库来源与去向清单（2026-10-02 迁移快照）

本清单与 Release `legacy-sources-2026-10-02` 配套。机器可读数据在 Release 资产 `manifest.json`（逐库 refs、SHA、字节数、SHA-256）；附属资料盘点报告在 `metadata-archives.tar.gz`。快照方法：GitHub API（认证读取）+ `git clone --mirror` 全量镜像 + `git fsck --full --strict`。2026-10-01 盘点见 [2026-10-01-inventory.json](2026-10-01-inventory.json)，两次快照 refs 逐条一致（逐库核对无增删改）。

## 逐库来源与去向

### fudian（浮点）
- 来源：`https://github.com/Ann5t/fudian`，2 条分支，92 条可达提交；默认分支 `feat/goal-branch-core-v0.1`（79 条提交）即脉图 `main` 的运行基线；另一条基线分支 `feat/first-testable-baseline` 有 13 条默认分支外的提交。
- 去向：默认分支历史作为脉图运行基线**进入现行实现并继续演进**（PR #1 导入 79 条提交）；13 条独有提交中的分支提案、旧部署设计作为历史资料归档（本仓库 `docs/archive/`），原始实现**由新实现接替**；完整历史进 bundle。
- 附属资料：无标签、Release、Issue、PR、Wiki；无 LFS、无子模块。

### project1
- 来源：`https://github.com/Ann5t/project1`，17 条分支（16 条 dependabot + main），52 条 refs（含 19 条 PR 的 head/merge refs），40 条可达提交。
- 去向：模型接口、工具注册与会话执行思路**由新实现接替**（`src/maitu/provider.rs` 连接管理、`src/tooling.rs` 工具注册、`src/maitu/workflows.rs` 队列调度——本轮多连接调度替代其整批等待逻辑）；原实现与全部 PR/分支归档。
- 附属资料：19 条 PR（dependabot 依赖升级，1/3/18 已关闭，其余开放），正文、评论与审阅保存于 `metadata-archives.tar.gz`（`project1-prs/pr-N*.json`）；PR 代码变更可从 bundle 内 `refs/pull/*/head` 恢复。Wiki 仅为设置项，实际不存在。无标签、Release、Issue、LFS、子模块。

### -Encore
- 来源：`https://github.com/Ann5t/-Encore`，5 条分支，101 条可达提交；三个分支（`copilot/check-codex-desktop-functionality`、`copilot/research-software-necessity`、`feature/fill-stubs`）指向同一开发头，含 64 条默认分支外提交。
- 去向：输入/判断/决定/结果回流的思路**由新实现接替**（`src/maitu/plans.rs` 计划生成与采用、`src/application/ideas.rs`、任务图采用与固定引用）；开发分支中的 Provider 管理界面与对话导入导出设计作为历史资料归档，其中连接管理思想在本轮多连接调度（`src/maitu/provider.rs`、`0017_maitu_connections.sql`）中延续。
- 附属资料：无标签、Release、Issue、PR、Wiki；无 LFS、子模块。全部对象中仅存在 `sk-` 形式的 UI 输入占位符，无真实凭据（已检查）。

### personal-ai-hub
- 来源：`https://github.com/Ann5t/personal-ai-hub`，10 条分支（9 条 codex/* + master），200 条可达提交，分支历史全部可从默认分支到达。
- 去向：想法收集、判断稿、验收与复盘的思路**由新实现接替**（脉图想法→目标→任务图→采用与理由记录）；全部原实现归档。
- 附属资料：无标签、Release、Issue、PR、Wiki；无 LFS、子模块。

### the-hive
- 来源：`https://github.com/Ann5t/the-hive`，仅默认分支 `master`，42 条可达提交。
- 去向：目标拆分、任务分配、Worker 与模型选择思路**由新实现接替**（脉图计划生成、任务图依赖调度与本轮多连接调度）；其依赖错误漏派与资源占满问题未沿用，由有界重试与按连接并发替代。原实现归档。
- 附属资料：无标签、Release、Issue、PR、Wiki；无 LFS、子模块。

## 本机数据审计（2026-10-02）

对五个旧仓库可能的本机数据执行双重扫描：按 `.git` 目录读取 origin 远端比对仓库名，按目录名匹配（fudian/-Encore/project1/personal-ai-hub/the-hive），覆盖 `C:\Users\dongy`（含 Documents、Desktop，5 层深度）、`D:\`（5 层深度）与 WSL `Ubuntu-26.04-LTS` 文件系统。结果：**未发现五个仓库的本机工作目录、未提交或忽略文件、数据库与运行成果**。该结论限于上述扫描范围。

## 敏感内容检查（发布前）

- 五库全部可达对象的文本扫描：`-Encore` 中 `sk-` 命中为表单占位符（`placeholder="sk-..."`），其余四库无命中；`password`/`secret` 命中均为界面代码或文档中的安全指引，无真实凭据。
- 元数据 JSON（PR 正文/评论/审阅）扫描无密钥模式。
- 五库均为私有仓库，归档仅进入同为本私有的 `Ann5t/maitu` 仓库 Release，仓库可见性不变。

## 删除准备状态

五库均满足：内容清单+去向记录、bundle 与附属归档、从 Release 的独立恢复验收（见恢复验收记录）、末尾增量核对。**是否删除 GitHub 仓库与本地目录由用户决定，本仓库与自动化流程不执行删除。**
