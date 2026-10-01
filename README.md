# 脉图 Maitu

脉图是一个以项目图组织个人 AI 工作的工作台。目标是同时推进互不依赖的任务，让每次尝试、成果、检查和决定留在可操作、可追溯的项目图中。

首版采用 Rust 后台、网页界面、PostgreSQL 和 Docker Compose，先在本机使用，再根据需要部署到持续运行的服务器。

## 当前状态

开发从浮点的 Rust 实现和完整主线历史开始，保留目标枝干、工作轮次、成果、Git worktree 和持久队列基础。仓库已导入的代码仍使用 `fudian` 包名、二进制名和部分界面名称；产品入口以 Maitu 为准，命名迁移按任务进行。

当前交付分支已连通目标生成计划、编辑并采用任务图、并行资料与编码任务、真实工具操作和检查、代码差异与版本采用。可以导入文本代码副本，让独立节点同时修改文件，后续节点引用确定成果继续整合；补充要求会创建新尝试并保留历史。真实 DeepSeek、实际进程与重启验收见[实施进度](docs/development/maitu-progress.md)。多 API 调度、完整的日常历史体验和五库收拢仍待完成。

用户确定的最终归宿是本仓库：先让脉图能承担自己的实际工作，再收拢五个旧项目的功能、资料和全部可保存历史，完成恢复验收后再决定删除旧库。[日常使用与旧仓库收拢路线](docs/product/completion-roadmap.md)定义整体完成条件；首轮资料任务完成只是其中一个阶段。

## 开始开发

打开本仓库根目录即可查看真实 Git 历史。Agent 先阅读 [AGENTS.md](AGENTS.md)，归类任务并建立分支，再修改文件。完成后以 PR 审阅，用户批准后合入 `main`。

本机运行入口见 [本机运行指南](docs/operations/maitu-local.md)。在仓库根目录的 PowerShell 中执行：

```powershell
./scripts/start-local.ps1
```

默认地址为 `http://127.0.0.1:3033`。开发与检查遵循 [贡献指南](CONTRIBUTING.md)和[测试指南](docs/development/testing.md)，实际验证结果在实施进度中维护。

## 首轮资料任务

1. 从 Docker 启动现有网页版，形成可重复的运行基础。
2. 接通 DeepSeek，以读取资料并产出文件完成首轮；连接设置位于 `/maitu/settings`。
3. 多个任务实际并行，依赖满足后启动相关后续任务。
4. 从图上操作任务，保留历次尝试、采用决定和重开页面后的进度，然后再扩展代码与检查工具。

资料任务已完成首轮真实账户验收，随后[从想法到编码成果](docs/development/code-workflow-goal.md)也已在本机跑通。代码导入、检查环境和成果导出的使用边界见[本机运行指南](docs/operations/maitu-local.md)；整体后续工作见[完成路线](docs/product/completion-roadmap.md)。

## 文档与来源

- [五个旧项目的来源](docs/product/source-projects.md)
- [旧仓库迁入与删除准备](docs/operations/legacy-repository-migration.md)
- [产品与工作流设计建议](docs/product/maitu-design.md)
- [文档导航](docs/README.md)
- [现有实现架构](docs/architecture/README.md)
- [接口参考](docs/reference/README.md)
- [部署指南](docs/operations/README.md)

继承的浮点文档保留其原始状态和日期。Maitu 的整体目标、每轮范围与实际验证结果由本仓库入口指向的文档分别定义；旧仓库继续作为原始来源，尚未达到删除条件。
