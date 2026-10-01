# 脉图代码与设计来源

Maitu 继续利用过去五个项目的积累。运行基线从浮点导入；其他仓库按能力参考，尚未整仓合并。旧仓库和原始文件保留，不在本次创建中删除。

## 已导入基线

- 仓库：[Ann5t/fudian](https://github.com/Ann5t/fudian)。
- 来源分支：`feat/goal-branch-core-v0.1`。
- 导入提交：`fb5e4d9b5211d77937ca2903bfb2db5a221639dc`。
- 该提交及其 79 条完整主线提交保留在本仓库 Git 中，提交 SHA 未改写；不是浅克隆。
- 导入源码、迁移、工具、部署配置和文档。真实运行能力仍需通过本仓库的构建与使用验证。

## 参考来源

| 仓库 | 静态阅读版本 | 参考方向 |
| --- | --- | --- |
| [project1](https://github.com/Ann5t/project1) | `2553a0a61e2b2d5a4a5e7cbb751be5391bb99fbe` | 真实模型调用、工具注册和工作流图；依赖传播与分批等待需修正 |
| [Encore](https://github.com/Ann5t/-Encore) | `3e9742df633c01dfca2c57a6724b8c01a7aff721` | 输入来源、判断、决定和成果回流 |
| [personal-ai-hub](https://github.com/Ann5t/personal-ai-hub) | `fb275f0e92645ac974eedec7af9476d3a7aa033b` | 判断稿、项目上下文、任务验收和复盘 |
| [the-hive](https://github.com/Ann5t/the-hive) | `1c4c230137c657d9486a91782573e7f5bff1fde4` | 拆分、Worker 池和模型连接；需实现可靠排队和依赖校验 |

这些版本来自 2026 年 9 月 30 日的默认分支静态阅读，不等于全部历史或运行验收。后续迁入代码时记录来源、选择理由和针对性验证。

## 全部分支补充盘点

2026-10-01 通过已有 GitHub 凭据读取五库元数据，并取得完整镜像检查可见 refs 与可达对象。五个镜像均非浅克隆且通过对象完整性检查。详细 refs、文件树与数量见[来源盘点档案](../archive/sources/README.md)。镜像仍为本机核对副本；原库继续保留，未发布迁移归档或完成独立恢复。

浮点有两个分支、92 条可达提交。当前导入的 79 条主线之外，`feat/first-testable-baseline` 还有 13 条独有提交，尚未作为历史来源迁入脉图。不能用已导入默认分支说明浮点整库已经保存。

Encore 的 `feature/fill-stubs`、`copilot/check-codex-desktop-functionality` 与 `copilot/research-software-necessity` 指向同一提交 `f3891418d10706d07ee8b6474a71537ccc63ec6b`，拥有默认分支之外的 64 条提交。本次补充静态读取其[Provider 策略](https://github.com/Ann5t/-Encore/blob/f3891418d10706d07ee8b6474a71537ccc63ec6b/docs/architecture/05-ai.md)、[后台判断实现](https://github.com/Ann5t/-Encore/blob/f3891418d10706d07ee8b6474a71537ccc63ec6b/backend/src/ai/orchestrator.rs)和[来源时间线](https://github.com/Ann5t/-Encore/blob/f3891418d10706d07ee8b6474a71537ccc63ec6b/backend/src/api/provenance.rs)。分支还包含 Provider 管理、对话、记忆和数据导入导出等源码，可作为后续复用候选；本次没有运行或验收这些能力。

Personal AI Hub 的九条工作分支均可从默认分支历史到达，本次全库可达提交数和默认分支提交数都是 200。仍保留原分支名称与 SHA。project1 有 19 条 PR 和已开启但未核对内容的 Wiki；Git 镜像不保存 PR 讨论或 Wiki 正文。

功能迁入、全量来源保存和旧库删除准备分别追踪，完整目标见[完成路线](completion-roadmap.md)，操作与恢复条件见[旧库迁移方案](../operations/legacy-repository-migration.md)。

## 新仓库开发方法

参考用户指定的既有项目整理流程：先明确方向与目录，修改前开分支，保留真实提交和分支关系，验证完成后交付审阅版本，由用户决定合并。具体规则写入 [AGENTS.md](../../AGENTS.md)，并提供 Copilot 与 Claude 的入口。
