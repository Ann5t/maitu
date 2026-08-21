# 中央插件与环境 API v1

本接口实现 [`tool-protocol.md`](tool-protocol.md) 的第一条可运行切片。它证明不可变插件包、固定环境、Session 绑定、无状态 ToolCall 与审计链；当前唯一 Runner 是无文件/网络副作用的进程内 Mock，不代表 OCI/Wasm 隔离 Runner 已经完成。

## 渐进式插件目录

| 方法与路径 | 用途 |
| --- | --- |
| `GET /api/v1/plugins` | 只返回固定引用、短简介与能力目录 |
| `GET /api/v1/plugins/:plugin_id/:version` | 按需读取完整 Manifest；版本也可用 `latest` |
| `POST /api/v1/plugins/resolve` | 把 exact/`latest` 选择器解析成固定 SemVer + digest |
| `POST /api/v1/plugins` | 注册 Manifest 草稿，由服务器规范化并计算内容摘要 |

持久化 PluginManifest 永不包含 `latest`。同名不同版本可以并存；同一版本出现多个内容摘要时，版本解析返回 `plugin_digest_conflict`，不会静默选择。包记录不可更新或删除。

应用启动时会幂等登记 `fudian.tools.reference@1.0.0`。它包含纯函数 `echo` 与 `inspect`，Runtime 类型为 `mock`，用于验证协议和审计。

## EnvironmentManifest

| 方法与路径 | 用途 |
| --- | --- |
| `POST /api/v1/environments` | 校验、规范化、内容去重并创建不可变环境 |
| `GET /api/v1/environments/:id` | 读取准确 Manifest 与指纹 |
| `POST /api/v1/projects/:project_id/sessions/:session_id/environment` | running Session 首次固定环境 |

环境中的每个插件都必须已经解析为准确 `pluginId/version/contentDigest`。插件顺序、Feature、lock path 与 map 会规范化；插件、工具链、lock、目标、Feature、构建参数或策略任一改变都会改变 SHA-256 指纹。

Session 绑定不可变：相同环境重放返回 `replayed: true`，不同环境重绑返回 409。子枝干首个 Session 和审核退回后的下一 Session 默认继承父/上一 Session 的绑定，写入独立 binding 记录；它们不能修改全局插件或原 Session 的 Manifest。

## ToolCall

`POST /api/v1/projects/:project_id/sessions/:session_id/tool-calls`

请求包含：

- `clientRequestId`；
- 完整固定插件引用和工具名；
- 结构化输入；
- `baseWorkspaceSnapshot`；
- 收窄后的 `allowedWrites`；
- 不超过插件资源上限的超时。

Broker 验证 Session 为 running、环境 binding/指纹一致、插件准确存在于该环境、工具与权限已经声明。参考 Mock 返回 ToolResult 后，系统在同一事务保存不可变 ToolCall 和 GoalEvent。结果绑定 project/branch/Session、插件版本/摘要、环境指纹、基础/结果工作区快照和时间；Mock 不产生 change set，因此结果快照等于基础快照。

同一 ToolCall 幂等键与相同输入只产生一条审计记录；更换输入返回 `idempotency_conflict`。当前不执行真实进程、网络或 worktree 写回，非 mock Runtime 返回 `runner_unavailable`。

## ToolLease

Rust 领域层与数据库已经定义 `requested → active → released/expired/cancelled` 状态、心跳、软/硬到期、资源策略、端点引用和保留输出。实际持久 Worker/Lease 管理器、崩溃恢复和端口代理留给隔离 Runner 目标；普通 Mock ToolCall 不创建后台进程。

## 当前权限边界

当前恢复版仍没有身份认证，注册与绑定接口只能在本机或受信任私人网络开放。插件 Manifest 只能声明需求，不能靠声明获得外部写、私人账号、凭据、宿主目录或 Docker Socket。未来的管理员认证、包签名和 Runner 能力授予必须在不改变固定版本与最小权限语义的前提下补上。
