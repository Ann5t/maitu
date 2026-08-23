# 主要 HTTP 路由

> 状态：已实现参考。完整注册以 `src/web/mod.rs` 为准。

| 方法 | 路由 | 用途 |
| --- | --- | --- |
| `GET` | `/` | 项目总览 |
| `GET` | `/settings` | 模型接入状态、联网能力、外观和账号设置 |
| `GET` | `/ideas` | 想法工作区 |
| `GET` | `/ideas/:id` | 想法版本、关系与立项工作区 |
| `GET/POST` | `/api/v1/ideas` | 想法摘要与幂等创建 |
| `POST` | `/api/v1/ideas/:id/commands` | 想法修订、关联与 ProjectProposal |
| `POST/GET` | `/api/v1/ideas/:id/sources[/:source_id/content]` | 附加或读取想法来源 |
| `POST` | `/api/v1/project-proposals/:id/commands` | 立项提案修订与人工决定 |
| `GET` | `/projects/:id` | 项目工作区 |
| `GET` | `/api/health` | 数据库健康检查 |
| `GET/POST` | `/auth/setup`、`/auth/login`、`/auth/recover` | Owner 初始化、登录和恢复 |
| `POST` | `/auth/logout`、`/auth/password` | 登出和修改口令 |
| `GET/POST` | `/api/projects` | 项目列表与创建 |
| `GET` | `/api/projects/:id` | 项目快照 |
| `GET` | `/api/v1/projects/:id/goal-graph` | 目标枝干领域快照 |
| `POST` | `/api/v1/projects/:id/goal-commands` | 幂等目标枝干命令 |
| `GET` | `/api/v1/projects/:id/sessions/:session_id/context` | Session 默认上下文 |
| `GET` | `/api/v1/projects/:id/sessions/:session_id/context/entries` | 上下文目录 |
| `POST` | `/api/v1/projects/:id/sessions/:session_id/context/read` | 按需读取上下文 |
| `GET/POST` | `/api/v1/plugins` | 插件目录与注册 |
| `POST` | `/api/v1/environments` | 固定环境清单 |
| `POST` | `/api/v1/projects/:id/sessions/:session_id/tool-executions` | 准备真实工具执行 |
| `GET/POST` | `/api/v1/tool-leases/:lease_id/proxy/:index/*path` | 持续工具的认证代理 |
| `GET/POST` | `/api/v1/projects/:id/sessions/:session_id/inputs` | Session 文件输入 |
| `GET` | `/api/artifacts/:id` | 带 ETag 的产物读取 |

目标枝干命令见[目标枝干 API](api-goal-branch-v1.md)，工具调用见[工具 API](tooling-api-v1.md)，输入上传见[输入 API](input-api-v1.md)。
