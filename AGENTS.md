# 脉图开发规则

本仓库是脉图 Maitu 的开发根目录。先阅读 [README](README.md)、[整体完成路线](docs/product/completion-roadmap.md)、[首轮范围](docs/product/maitu-scope.md)和[实施进度](docs/development/maitu-progress.md)。运行基础来自浮点；旧设计中的计划不能当成已实现功能。

最终目标是本仓库成为用户日常使用的个人 AI 工作台，再收拢五个旧库的功能、资料和可保存历史。首轮资料任务的 Goal 完成不表示整个产品完成。[旧库迁移方案](docs/operations/legacy-repository-migration.md)定义逐库恢复验收与删除准备，当前不删除旧库。优先围绕真实完整使用流程补能力，不用新增页面或生成文件数量代替验收。

## 先归类再分支

开始任务时向用户说明工作类别、相关目录、目标和拟用分支。只读研究可以留在当前分支；方向明确后，在首次修改文件前创建任务分支，文档和研究笔记也适用。不得在 `main` 上直接修改。

| 类别 | 主要目录 | 分支前缀 |
| --- | --- | --- |
| 模型与执行 | `src/application/`、`src/` | `execution/` |
| 并行与依赖 | `src/application/scheduler.rs`、`src/scheduler.rs`、`migrations/` | `scheduling/` |
| 项目图与操作 | `src/web/`、`assets/` | `graph/` |
| 历史与上下文 | `src/context_memory.rs`、`src/application/context_memory.rs` | `context/` |
| 工作区与工具 | `src/workspace.rs`、`src/bin/`、`plugins/` | `tools/` |
| 构建与部署 | `Dockerfile`、`compose*.yaml`、`scripts/` | `infra/` |
| 仓库与文档 | `docs/`、根目录说明、Agent 指令 | `repo/` |

分支名称说明具体工作，例如 `execution/first-model-task`。先检查现有工作树；不覆盖用户或其他任务的改动。相关文件按稳定职责修改，不夹带大范围整理。

## 提交与审阅

一个提交解决一个可描述的问题，使用 `docs:`、`feat:`、`fix:`、`test:`、`chore:` 等类型加清楚的描述。保留实际提交、分支和合并关系，不编造历史。

完成任务时提交并推送任务分支，提供 PR、行为变化、验证结果和实际限制。用户明确同意合并后才合入 `main`；未批准时保持可审阅状态。仓库初始化所导入的基线保留在 `main`。

## 以真实使用验收

优先让一个节点产生实际成果，再扩大并行、项目图和恢复能力。调用模型、工具执行、检查通过和用户接受分别记录。演示数据和计时模拟必须标明，不能当作真实 API 或工具执行。

重要架构变化先记录背景、选择和影响；优先复用已有代码。三个并行任务是首轮验收规模，不是硬编码的并发上限。

遵循 [贡献指南](CONTRIBUTING.md)和[测试指南](docs/development/testing.md)，执行与改动相关的检查。基础环境或旧代码失败时记录失败原因与可复现步骤，不报告为通过。重试、取消、依赖传播和崩溃恢复应有能发现真实错误的检查。

## 数据与仓库体积

凭据、真实 `.env`、数据库、模型文件、构建缓存和运行日志不进入 Git。大型原件只有实际需要归档时才放同仓库 Release，并保存校验值和恢复步骤。

本地预览使用 Maitu 专用数据卷和环回端口，防止与旧浮点实例混用。清理前检查范围和可恢复来源。远端发布、删除旧仓库和删除原始资料必须得到相应授权。
