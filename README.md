# 脉图 Maitu

脉图是一个以项目图组织个人 AI 工作的工作台。目标是同时推进互不依赖的任务，让每次尝试、成果、检查和决定留在可操作、可追溯的项目图中。

首版采用 Rust 后台、网页界面、PostgreSQL 和 Docker Compose，先在本机使用，再根据需要部署到持续运行的服务器。

## 当前状态

开发从浮点的 Rust 实现和完整主线历史开始，保留目标枝干、工作轮次、成果、Git worktree 和持久队列基础。仓库已导入的代码仍使用 `fudian` 包名、二进制名和部分界面名称；产品入口以 Maitu 为准，命名迁移按任务进行。

真实模型适配器、完整 Agent 工具循环和新的项目图尚待接通。已有示例与静态投影只能证明界面和领域基础，不能当成日用编码工具。当前工作和验证结果见 [实施进度](docs/development/maitu-progress.md)。

## 开始开发

打开本仓库根目录即可查看真实 Git 历史。Agent 先阅读 [AGENTS.md](AGENTS.md)，归类任务并建立分支，再修改文件。完成后以 PR 审阅，用户批准后合入 `main`。

本机启动与检查遵循 [贡献指南](CONTRIBUTING.md)和[测试指南](docs/development/testing.md)。Docker 运行入口和实际验证结果在当前实施进度中维护。

## 首版交付顺序

1. 从 Docker 启动现有网页版，形成可重复的运行基础。
2. 一个节点调用真实模型并执行获准工具，产生可打开的成果和检查记录。
3. 多个任务实际并行，依赖满足后启动相关后续任务。
4. 从图上操作任务，保留历次尝试、采用决定和重开页面后的进度。

详细输入、输出和验收见 [首版范围](docs/product/maitu-scope.md)。

## 文档与来源

- [五个旧项目的来源](docs/product/source-projects.md)
- [产品与工作流设计建议](docs/product/maitu-design.md)
- [文档导航](docs/README.md)
- [现有实现架构](docs/architecture/README.md)
- [接口参考](docs/reference/README.md)
- [部署指南](docs/operations/README.md)

继承的浮点文档保留其原始状态和日期。Maitu 的当前范围与实际验证结果由本仓库入口指向的文档定义；旧仓库继续作为原始来源。
