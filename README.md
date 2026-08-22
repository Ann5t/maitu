# 浮点 · Rust 重写版

这是从现有 Docker 镜像、PostgreSQL 数据和产物卷重新建立的 Rust 全栈版本。它保留 `fudian-nextgen` 已经形成的项目模型，同时把界面重新收束到桌面版 `fudian` 的视觉语言：纸张感背景、酸绿色强调、固定侧栏、紧凑卡片和以项目脉络为中心的工作区。

当前版本不依赖 Node.js、npm 或前端构建链。HTML 由 Rust 服务端渲染，少量原生 JavaScript 只负责主题切换、提交反馈和图谱定位；关闭 JavaScript 后核心表单仍可使用。

从旧 Docker 镜像找回的源码已独立封存并校验，见 [Docker 源码取证归档](recovery/README.md)；它不参与新版本构建。

## 已实现

- 项目列表、项目创建和原始意图修订
- 独立“想法”一级空间、不可变版本/关系、内容寻址文件/图片/语音，以及人工批准的 ProjectProposal 立项来源
- 成果契约确认，以及可核验的完成标准
- 项目启动说明的生成、哈希存储、审阅和批准
- 项目 DAG：开分支、记录带类型的贡献、暂停、选择性合流
- 图谱、契约、产物和历史四个项目视图
- 与现有 `fudian-nextgen` PostgreSQL 表和 Docker 产物卷兼容
- HTML 表单与 JSON API 两套入口，共用同一应用服务和事务逻辑
- 不可变目标契约修订、逐字段来源与人工接受/拒绝，以及有预算和收敛边界的探索型目标
- 结构化 Evidence、候选冻结、撤回、停止和保留原结论的终态归档
- 精确父 Session 上下文快照、不可折叠契约/权限信封、完整来源目录和审计式按需披露
- 请求幂等、输入校验、产物路径防穿越和基础安全响应头
- 明暗主题、响应式布局和移动端项目脉络列表

尚未完成的功能边界见 [迁移状态](docs/migration-status.md)。目前没有身份认证，因此只应放在本机或受信任的反向代理之后。

## 快速预览

先在不占用旧服务端口的位置启动：

```bash
APP_PORT=3001 make start
```

打开 `http://localhost:3001`。这个 Compose 项目名仍是 `fudian-nextgen`，会复用已有的：

- `fudian_nextgen_postgres_data`
- `fudian_nextgen_artifacts`

启动时只执行幂等迁移，不清空数据。不要运行 `docker compose down -v`，它会请求删除数据卷。
应用和 PostgreSQL 的宿主机端口默认都只绑定 `127.0.0.1`；需要跨设备访问时，应先增加认证和受控反向代理，不要直接公开数据库端口。

开发模式：

```bash
APP_PORT=3001 make dev
```

本机直接运行 Rust 服务时，可复制 `.env.example` 为 `.env`；它默认连接宿主机的 `55432` 端口。

## 质量检查

```bash
make check
```

该命令依次执行 rustfmt、Clippy（警告视为错误）和全部测试。完整的隔离端到端核验方法记录在 [架构说明](docs/architecture.md)。

提交前的完整质量门是：

```bash
./scripts/quality-gate.sh
```

它会构建固定 Rust 开发镜像，在一次性 PostgreSQL 中运行迁移及全部 HTTP 闭环，用固定 Playwright/Chromium 验证桌面和 390px 工作台，最后构建非 root、只读根文件系统的生产镜像并在随机本机端口验收。数据库和浏览器现场都是隔离的，不连接 Compose 中的真实数据卷；Cargo、npm 仅复用中央缓存卷。

仓库内的 `.github/workflows/ci.yml` 在 push、pull request 或手动触发时运行同一入口。它只有源码读取权限，不发布镜像、不部署，也不持久化 Git 凭据。

## 备份

在切换正式服务前，先创建包含数据库、产物和当前源码的备份：

```bash
make backup
```

备份写入 `backups/<UTC 时间>/`，并生成 `SHA256SUMS`。也可以指定目录：

```bash
./scripts/backup.sh /path/to/backup-root
```

恢复和回退步骤见 [恢复与切换说明](docs/recovery.md)。

## 代码结构

```text
src/domain.rs                纯领域规则与输入校验
src/idea_domain.rs           想法与 ProjectProposal 的纯领域规则
src/application/projects.rs  项目、契约、产物用例
src/application/ideas.rs     想法版本、关联和原子立项事务
src/application/graph.rs     项目 DAG 与选择性合流
src/application/goal_branches.rs  目标枝干事务与审核闭环
src/application/context_memory.rs Session 快照、来源目录、读取审计与派生重建
src/application/plugins.rs   插件目录、环境绑定与 Tool Broker
src/application/inputs.rs    Session 文件输入与内容寻址导入
src/web/handlers.rs          HTML/JSON 传输适配
src/web/views.rs             Maud 服务端页面
src/web/goal_projection.rs   可替换的 GoalBranch/Session 车道投影
src/artifacts.rs             文件产物边界
migrations/                  兼容现有数据的幂等 SQL
assets/                      CSS、渐进增强脚本和图标
```

更多设计取舍见 [架构说明](docs/architecture.md)。

长期产品语义、目标枝干模型、Agent Session、BranchProposal、插件系统与多设备部署约束，见 [产品设计基线](docs/product-design.md)。后续实现 Goal 应以该文件为准；当前架构文档只描述已经落地的技术状态。

“目标枝干核心 v0.1”的实现范围、完整验证、真实现场保护证据和下一 Goal 建议见 [阶段执行报告](docs/overnight-report.md)。

当前覆盖全产品第一阶段的 12 个方面、每项完成定义、缺口与实施顺序，以 [12 方面总验收矩阵](docs/design-12-aspects.md) 为唯一准绳；执行状态见 [12 方面总 Goal 进度](docs/goal-12-progress.md)。

想法版本、关系、ProjectProposal、精确来源和原子立项协议见 [想法空间与 ProjectProposal v1](docs/idea-project-domain-v1.md)。

契约演进、探索模式、结构化 Evidence、候选撤回与停止/归档边界见 [目标契约、Evidence 与执行生命周期 v2](docs/core-domain-v2.md)。

精确父快照、不可折叠信封、来源边、固定窗口和摘要/片段/全文读取协议见 [Session 上下文、来源与渐进式披露 v1](docs/context-memory-v1.md)。

本阶段可执行的状态转换、审核边界与旧模型共存规则见 [目标枝干领域与状态机](docs/goal-branch-domain.md)；中央插件、环境指纹、Tool Broker、Lease 和文件输入约束见 [中央插件、环境与文件协议](docs/tool-protocol.md)。

新目标枝干纵向接口及幂等/错误语义见 [目标枝干 HTTP API v1](docs/api-goal-branch-v1.md)。

插件目录、不可变环境、Session 绑定和参考 Tool Broker 接口见 [中央插件与环境 API v1](docs/tooling-api-v1.md)。

分段上传、内容验证、Session inbox 与冻结边界见 [Session 安全文件输入 API v1](docs/input-api-v1.md)。

可操作的 Git 车道投影、Session 工作现场、桌面/手机截图与替换边界见 [目标枝干工作台投影 v1](docs/workbench-projection-v1.md)。

## 主要路由

| 方法 | 路由 | 用途 |
| --- | --- | --- |
| `GET` | `/` | 项目总览 |
| `GET` | `/ideas` | 想法时间流或关系投影 |
| `GET` | `/ideas/:id` | 想法版本、关系与立项工作区 |
| `GET/POST` | `/api/v1/ideas` | 想法摘要与幂等创建 |
| `POST` | `/api/v1/ideas/:id/commands` | 想法修订、关联及建立 ProjectProposal |
| `POST/GET` | `/api/v1/ideas/:id/sources[/:source_id/content]` | 校验附加或读取想法来源 |
| `POST` | `/api/v1/project-proposals/:id/commands` | 立项提案修订与人工决定 |
| `GET` | `/projects/:id` | 项目工作区 |
| `POST` | `/projects/:id/actions` | 成果契约与产物动作 |
| `POST` | `/projects/:id/graph` | 图谱动作 |
| `GET` | `/api/health` | 数据库健康检查 |
| `GET/POST` | `/api/projects` | 项目列表与创建 |
| `GET` | `/api/projects/:id` | 完整项目快照 |
| `POST` | `/api/projects/:id/actions` | JSON 项目动作 |
| `POST` | `/api/projects/:id/graph` | JSON 图谱动作 |
| `GET` | `/api/artifacts/:id` | 带 ETag 的产物读取 |
| `GET` | `/api/v1/projects/:id/goal-graph` | 新目标枝干领域快照 |
| `POST` | `/api/v1/projects/:id/goal-commands` | 幂等目标枝干命令 |
| `GET` | `/api/v1/projects/:id/sessions/:session_id/context` | 当前不可折叠上下文与默认目录 |
| `GET` | `/api/v1/projects/:id/sessions/:session_id/context/entries` | 检索/分页完整上下文目录 |
| `POST` | `/api/v1/projects/:id/sessions/:session_id/context/read` | 审计式摘要、片段或全文读取 |
| `POST` | `/api/v1/projects/:id/sessions/:session_id/context/rebuild` | 重建派生摘要与索引的新代 |
| `GET/POST` | `/api/v1/plugins` | 渐进式插件目录与注册 |
| `POST` | `/api/v1/environments` | 固定 EnvironmentManifest |
| `POST` | `/api/v1/projects/:id/sessions/:session_id/tool-calls` | 无状态工具调用 |
| `GET/POST` | `/api/v1/projects/:id/sessions/:session_id/inputs` | 列出/建立 Session 文件输入 |
| `PUT` | `/api/v1/projects/:id/sessions/:session_id/inputs/:input_id/chunks` | 上传受限文件分段 |
| `POST` | `/api/v1/projects/:id/sessions/:session_id/inputs/:input_id/finish` | 完整性与可信类型验证 |
| `POST` | `/api/v1/projects/:id/sessions/:session_id/inputs/:input_id/import` | 显式导入 worktree/inbox 或产物引用 |

工作台隔离验收：

```bash
./scripts/test-workbench-http.sh
./scripts/test-workbench-browser.sh
```

第二个命令使用固定版本的一次性官方 Playwright 容器，不在应用镜像或仓库内安装 Node 运行时。

## 技术基线

- Rust 2024 edition，最低 Rust 1.94
- 构建与 CI 固定 Rust 1.97；`Cargo.toml` 仍声明最低 Rust 1.94
- Axum 0.8、SQLx 0.9、Maud 0.27
- PostgreSQL 17
- Debian slim 生产运行镜像，非 root 用户

依赖版本已锁定在 `Cargo.lock`；不要在未备份和未跑完整检查时批量升级。
