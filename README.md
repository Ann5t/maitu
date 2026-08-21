# 浮点 · Rust 重写版

这是从现有 Docker 镜像、PostgreSQL 数据和产物卷重新建立的 Rust 全栈版本。它保留 `fudian-nextgen` 已经形成的项目模型，同时把界面重新收束到桌面版 `fudian` 的视觉语言：纸张感背景、酸绿色强调、固定侧栏、紧凑卡片和以项目脉络为中心的工作区。

当前版本不依赖 Node.js、npm 或前端构建链。HTML 由 Rust 服务端渲染，少量原生 JavaScript 只负责主题切换、提交反馈和图谱定位；关闭 JavaScript 后核心表单仍可使用。

从旧 Docker 镜像找回的源码已独立封存并校验，见 [Docker 源码取证归档](recovery/README.md)；它不参与新版本构建。

## 已实现

- 项目列表、项目创建和原始意图修订
- 成果契约确认，以及可核验的完成标准
- 项目启动说明的生成、哈希存储、审阅和批准
- 项目 DAG：开分支、记录带类型的贡献、暂停、选择性合流
- 图谱、契约、产物和历史四个项目视图
- 与现有 `fudian-nextgen` PostgreSQL 表和 Docker 产物卷兼容
- HTML 表单与 JSON API 两套入口，共用同一应用服务和事务逻辑
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
src/application/projects.rs  项目、契约、产物用例
src/application/graph.rs     项目 DAG 与选择性合流
src/web/handlers.rs          HTML/JSON 传输适配
src/web/views.rs             Maud 服务端页面
src/artifacts.rs             文件产物边界
migrations/                  兼容现有数据的幂等 SQL
assets/                      CSS、渐进增强脚本和图标
```

更多设计取舍见 [架构说明](docs/architecture.md)。

长期产品语义、目标枝干模型、Agent Session、BranchProposal、插件系统与多设备部署约束，见 [产品设计基线](docs/product-design.md)。后续实现 Goal 应以该文件为准；当前架构文档只描述已经落地的技术状态。

本阶段可执行的状态转换、审核边界与旧模型共存规则见 [目标枝干领域与状态机](docs/goal-branch-domain.md)；中央插件、环境指纹、Tool Broker、Lease 和文件输入约束见 [中央插件、环境与文件协议](docs/tool-protocol.md)。

新目标枝干纵向接口及幂等/错误语义见 [目标枝干 HTTP API v1](docs/api-goal-branch-v1.md)。

## 主要路由

| 方法 | 路由 | 用途 |
| --- | --- | --- |
| `GET` | `/` | 项目总览 |
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

## 技术基线

- Rust 2024 edition，最低 Rust 1.94
- Axum 0.8、SQLx 0.9、Maud 0.27
- PostgreSQL 17
- Debian slim 生产运行镜像，非 root 用户

依赖版本已锁定在 `Cargo.lock`；不要在未备份和未跑完整检查时批量升级。
