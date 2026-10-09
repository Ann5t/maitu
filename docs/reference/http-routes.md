# 主要 HTTP 路由

> 状态：已实现参考。本页列出日常使用与外部集成会直接接触的主要路由；完整注册以 `src/web/mod.rs` 为准。

## 脉图工作台

| 方法 | 路由 | 用途 |
| --- | --- | --- |
| `GET` | `/`、`/maitu` | 脉图工作台与项目列表 |
| `GET` | `/maitu/settings` | 模型连接设置 |
| `GET` | `/maitu/projects/{project_id}` | 项目工作现场 |

## 脉图 API

| 方法 | 路由 | 用途 |
| --- | --- | --- |
| `GET/POST` | `/api/maitu/connections` | 模型连接列表与保存 |
| `DELETE` | `/api/maitu/connections/{key}` | 删除一条模型连接 |
| `GET` | `/api/maitu/provider` | 当前连接配置摘要 |
| `GET` | `/api/maitu/projects/{project_id}` | 项目快照 |
| `POST` | `/api/maitu/projects/{project_id}/tasks` | 创建任务 |
| `POST` | `/api/maitu/projects/{project_id}/plans` | 生成执行计划 |
| `POST` | `/api/maitu/projects/{project_id}/plans/adopt` | 采纳计划并生成任务 |
| `POST` | `/api/maitu/projects/{project_id}/code` | 导入代码（请求体上限 40 MiB） |
| `GET` | `/api/maitu/projects/{project_id}/code/export` | 导出项目代码 |
| `POST` | `/api/maitu/projects/{project_id}/sources` | 添加资料源 |
| `GET` | `/api/maitu/sources/{source_id}` | 读取资料源内容 |
| `GET` | `/api/maitu/tasks/{task_id}` | 任务详情 |
| `POST` | `/api/maitu/tasks/{task_id}/start` | 启动任务 |
| `POST` | `/api/maitu/tasks/{task_id}/cancel` | 取消任务 |
| `POST` | `/api/maitu/tasks/{task_id}/accept` | 采纳任务成果 |
| `GET` | `/api/maitu/tasks/{task_id}/attempts/{attempt_id}/diff` | 读取一次尝试的代码差异 |

## 旧浮点表面

| 方法 | 路由 | 用途 |
| --- | --- | --- |
| `GET` | `/legacy` | 旧浮点项目总览 |
| `GET` | `/settings` | 旧浮点设置页：接入状态、外观和账号设置 |
| `GET` | `/ideas`、`/ideas/new`、`/ideas/{idea_id}` | 想法工作区 |
| `GET` | `/projects/{project_id}` | 旧浮点项目工作区 |
| `GET/POST` | `/api/v1/ideas` | 想法摘要与幂等创建 |
| `GET` | `/api/v1/ideas/{idea_id}` | 想法快照 |
| `POST` | `/api/v1/ideas/{idea_id}/commands` | 想法修订、关联与 ProjectProposal |
| `POST/GET` | `/api/v1/ideas/{idea_id}/sources[/{source_id}/content]` | 附加或读取想法来源 |
| `POST` | `/api/v1/project-proposals/{proposal_id}/commands` | 立项提案修订与人工决定 |
| `GET/POST` | `/api/projects` | 项目列表与创建 |
| `GET` | `/api/projects/{project_id}` | 项目快照 |
| `GET` | `/api/v1/projects/{project_id}/goal-graph` | 目标枝干领域快照 |
| `POST` | `/api/v1/projects/{project_id}/goal-commands` | 幂等目标枝干命令 |
| `GET` | `/api/v1/projects/{project_id}/sessions/{session_id}/context` | Session 默认上下文 |
| `GET` | `/api/v1/projects/{project_id}/sessions/{session_id}/context/entries` | 上下文目录 |
| `POST` | `/api/v1/projects/{project_id}/sessions/{session_id}/context/read` | 按需读取上下文 |
| `GET/POST` | `/api/v1/plugins` | 插件目录与注册 |
| `POST` | `/api/v1/environments` | 固定环境清单 |
| `POST` | `/api/v1/projects/{project_id}/sessions/{session_id}/tool-executions` | 准备真实工具执行 |
| `GET/POST` | `/api/v1/tool-leases/{tool_lease_id}/proxy/{endpoint_index}/*path` | 持续工具的认证代理 |
| `GET/POST` | `/api/v1/projects/{project_id}/sessions/{session_id}/inputs` | Session 文件输入 |
| `GET` | `/api/artifacts/{artifact_id}` | 带 ETag 的产物读取 |

## 认证与健康检查

| 方法 | 路由 | 用途 |
| --- | --- | --- |
| `GET/POST` | `/auth/setup`、`/auth/login`、`/auth/recover` | Owner 初始化、登录和恢复 |
| `POST` | `/auth/logout` | 登出 |
| `GET` | `/auth/status` | 当前会话状态 |
| `GET/POST` | `/account/password` | 修改口令页面与提交 |
| `GET` | `/api/health` | 数据库健康检查 |

目标枝干命令见[目标枝干 API](api-goal-branch-v1.md)，工具调用见[工具 API](tooling-api-v1.md)，输入上传见[输入 API](input-api-v1.md)。