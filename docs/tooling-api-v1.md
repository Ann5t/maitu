# 中央插件与环境 API v1

本接口实现 [`tool-protocol.md`](tool-protocol.md) 的两条兼容执行路径：进程内 Mock 用于轻量协议回归；签名 OCI 插件通过隔离 Runner、单写 Lease 和 Git compare-and-swap 真实执行。应用本身不持有 Docker Socket，外部调度器只能按 API 返回的准确镜像摘要启动一次性 Worker。

## 渐进式插件目录

| 方法与路径 | 用途 |
| --- | --- |
| `GET /api/v1/plugins` | 只返回固定引用、短简介、能力、安装状态和发布者 |
| `GET /api/v1/plugins/:plugin_id/:version` | 按需读取完整 Manifest；版本也可用 `latest` |
| `GET /api/v1/plugins/:plugin_id/:version/install-proof` | 按需读取 Manifest、签名安装证明、发布者状态和资源元数据；不返回正文 |
| `POST /api/v1/plugins/resolve` | 把 exact/`latest` 选择器解析成固定 SemVer + digest |
| `POST /api/v1/plugins` | 注册 Manifest 草稿，由服务器规范化并计算内容摘要 |
| `POST /api/v1/plugins/seal` | 只规范化并密封 Manifest，不登记或安装 |

持久化 PluginManifest 永不包含 `latest`。同名不同版本可以并存；同一版本出现多个内容摘要时，版本解析返回 `plugin_digest_conflict`，不会静默选择。包记录不可更新或删除。

应用启动时会幂等登记 `fudian.tools.reference@1.0.0`。它包含纯函数 `echo` 与 `inspect`，Runtime 类型为 `mock`，用于验证协议和审计。

## 签名安装与撤销

| 方法与路径 | 用途 |
| --- | --- |
| `POST /api/v1/plugin-publishers` | 登记稳定发布者 ID 与 Ed25519 公钥 |
| `POST /api/v1/plugins/install-statement` | 生成规范签名声明及摘要 |
| `POST /api/v1/plugins/install` | 验证签名、当前 Runner、自检和准确 OCI/入口摘要后安装 |
| `POST /api/v1/plugin-installations/:id/revoke` | 只追加撤销状态和原因，不删除历史证明 |
| `POST /api/v1/plugin-publishers/:id/revoke` | 撤销发布者；其插件不能进入新环境或继续启动 |

签名声明直接绑定 Manifest、插件三元组、OCI 镜像摘要、`/runtime/` 入口、入口文件摘要、Runner 摘要与结构化自检摘要。Manifest 的每个 Asset 路径和 SHA-256 也因此进入签名边界；`POST /plugins/install` 必须原子提交完全相同的资源集合。服务端解码并现场哈希内容，媒体类型由签名路径的扩展名确定且上传值必须一致，因此不能用未签名的类型改变解释方式。缺失、额外、重复、篡改、路径逃逸或超限资源均被拒绝；Skill 还必须是至多 128 KiB 的 UTF-8 Markdown/纯文本。内容、摘要和安装证明均不可更新或删除，重复提交只有逐字节相同时才返回原记录。

单资源上限 512 KiB、单包资源总量上限 2 MiB、最多 64 项；Runtime 镜像不作为 Asset 上传，仍由 OCI digest 固定。撤销只影响新启动、Session 插件上下文读取和回写前检查，已经完成的历史 ToolCall 仍可按原证据读取。

## EnvironmentManifest

| 方法与路径 | 用途 |
| --- | --- |
| `POST /api/v1/environments` | 校验、规范化、内容去重并创建不可变环境 |
| `GET /api/v1/environments/:id` | 读取准确 Manifest 与指纹 |
| `POST /api/v1/projects/:project_id/sessions/:session_id/environment` | running Session 首次固定环境 |
| `POST /api/v1/projects/:project_id/sessions/:session_id/plugin-context/read` | 读取该固定环境的 Skill，并按显式选择器取其他资源 |

环境中的每个插件都必须已经解析为准确 `pluginId/version/contentDigest`。OCI 插件还必须具有未撤销的签名安装和有效发布者。插件顺序、Feature、lock path 与 map 会规范化；插件、工具链、lock、目标、Feature、构建参数或策略任一改变都会改变 SHA-256 指纹。

Session 绑定不可变：相同环境重放返回 `replayed: true`，不同环境重绑返回 409。子枝干首个 Session 和审核退回后的下一 Session 默认继承父/上一 Session 的绑定，写入独立 binding 记录；它们不能修改全局插件或原 Session 的 Manifest。

Agent Worker 启动时使用新的 `clientRequestId` 调用 `plugin-context/read`。响应只包含当前 Session 固定插件的目录摘要、全部资源元数据和可直接注入系统提示词的 Skill UTF-8 正文；不会带入目录中其他版本或未选择插件。其他 reference/asset 只有出现在请求的准确 `(pluginId, version, contentDigest, path)` 选择器中才返回正文（通用内容同时给出 Base64，文本另给 UTF-8）。单环境最多 32 个插件，一次最多显式读取 16 项且正文合计不超过 1 MiB；超出时应关闭自动 Skill 并进一步按需选择。每次读取绑定环境指纹、请求哈希和实际披露摘要写入不可变审计；同请求可重放，复用同一 ID 改变含义返回冲突。MCP 的 stdio/HTTP 传输仍可作为适配层演化，不改变这套内容与能力语义。

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

同一 Mock ToolCall 幂等键与相同输入只产生一条审计记录；更换输入返回 `idempotency_conflict`。Mock 路径不执行真实进程或 worktree 写回，非 mock Runtime 不能从此兼容入口执行。

## 真实 OCI ToolExecution

| 方法与路径 | 用途 |
| --- | --- |
| `POST /api/v1/projects/:project_id/sessions/:session_id/tool-executions` | 校验环境、签名、schema、权限和资源，准备 RunnerJob 与一次性 Lease |
| `POST /api/v1/projects/:project_id/sessions/:session_id/tool-executions/:execution_id/runner-jobs/:job_id/finalize` | 验证 Worker/入口隔离证明，经 Git CAS 回写并生成 ToolCall/Contribution |
| `POST /api/v1/projects/:project_id/sessions/:session_id/plugin-install-requests` | 对未安装或需授权能力建立持久安装请求 |

准备响应返回 `runtimeImageDigest`、`runtimeEntryDigest` 和完整 Runner spec。调度器在启动前必须把本地/注册表镜像解析为该准确摘要；标签不能作为证据。Worker 使用只读 worktree、独立输出层、断网 namespace、只读根文件系统、无 Linux capability、`no-new-privileges`、固定 UID 和 cgroup 上限。Runner 会现场哈希自身和实际 `/runtime/` 入口；任何伪造摘要、越权写入、符号链接、输出清单或基线漂移都会在 Git 发布前拒绝。

成功回写创建不可变 `tool_execution_requests`、`runner_jobs`、`workspace_snapshots`、`goal_contributions` 和 `tool_calls` 关联。ToolResult 绑定 base/result snapshot、head commit、插件安装、镜像/入口/Runner 摘要、环境指纹及输出清单。准备重放不返回第二份明文 Lease token，完成重放不重复执行或提交。

仓库提供固定镜像和真实验收：

- `fudian.tools.rust@1.0.0`：Rust 1.97 类型/语法检查；
- `fudian.tools.cxx@1.0.0`：GCC/G++ 15 的 C/C++ `-fsyntax-only`；
- `fudian.tools.python@1.0.0` 与 `2.0.0`：互不兼容示例库在两个 Session 环境中并存；
- `fudian.tools.playwright@1.0.0`：Playwright 1.62 + Chromium 对本地 HTML 截图；
- PPTMaster：只登记兼容 Manifest 和安装请求，未获得许可/适配器时不能执行。

## ToolLease

持续浏览器、开发服务器等操作通过唯一 `tool_leases` 记录和关联 `goal_action_runs` 调度；普通 ToolCall 仍在单次 Worker 后退出，不能借此留下进程。

| 方法与路径 | 用途 |
| --- | --- |
| `POST /api/v1/projects/:project_id/sessions/:session_id/tool-leases` | 固定签名安装、环境、快照、资源/时限并排队 `tool.lease` ActionRun |
| `GET /api/v1/projects/:project_id/tool-leases/:lease_id` | 读取状态、受控 endpoint、到期和清理状态；不返回 token 摘要 |
| `POST /api/v1/projects/:project_id/tool-leases/:lease_id/stop` | 持 renewal token 请求 release/cancel；运行中转为 cancellation requested |
| `POST /api/v1/scheduler/action-runs/:action_id/tool-lease/activate` | Worker 用 ActionLease/fencing 登记已启动 endpoint |
| `POST /api/v1/scheduler/action-runs/:action_id/heartbeat` | 同一心跳续 ActionLease 与关联 ToolLease，并取得取消信号 |
| `POST /api/v1/scheduler/action-runs/:action_id/tool-lease/finish` | launcher 确认进程/端点已终止后完成释放或取消 |
| `POST /api/v1/scheduler/tool-leases/:lease_id/cleanup` | `tool.cleanup` Worker 确认失联进程的外部清理结果 |
| `GET/POST /api/v1/tool-leases/:lease_id/proxy/:endpoint_index/*path` | owner 会话通过同源受控代理访问活动 endpoint；不下发内部地址 |

创建只允许 Manifest 中带 `persistent` 描述的签名 OCI 工具，且固定准确镜像、入口、Runner、EnvironmentManifest 和干净 workspace snapshot。当前内置适配器只接受断网静态预览等隔离控制面；联网、私人账号和外部写在没有专用适配器时拒绝。

应用不挂载 Docker Socket。受信 launcher 以返回的准确镜像 ID、只读 worktree、只读根、固定 UID、无 capability、`no-new-privileges` 和资源上限启动进程。Web 容器被杀并重建后，未到期 Worker 可用原 ActionLease/fencing 继续；Worker 丢失时持续进程不会盲目重启，而是令 ToolLease 过期、Session 暂停并要求清理确认。

BP-09 已关闭浏览器 endpoint 边界：工作现场只生成同源代理链接；服务端重新加载 Lease，要求 active 且软/硬期限均有效，endpoint 必须是配置 CIDR 内的 HTTP(S) 字面 IP。代理不跟随跳转、不读取宿主代理，使用头白名单并剔除 Cookie/Authorization/hop-by-hop，限制请求体和响应体，动作进入安全审计。Runner 原始 stdout/stderr 受 EnvironmentManifest 字节预算限制；结果/哈希/ActionEvent 长期保存在数据库，明确保留的输出进入 Runner 卷和 v2 备份，未引用文件只经保留期与可逆 quarantine。

## 当前权限边界

安全模式下，浏览器发布者/安装/绑定操作必须经过唯一 owner 会话、精确 Origin、CSRF 和限速；Worker/launcher 端点使用独立 bootstrap、ActionLease 或 renewal 凭据，不共享浏览器 Cookie。签名证明来源和完整性，但不等于外部账号或远程发布授权。插件 Manifest 只能声明需求，不能靠声明获得外部写、私人账号、凭据、宿主目录或 Docker Socket；当前没有安全适配器的联网/外部副作用继续明确拒绝。本机兼容 Compose 显式关闭认证，因此不能公开。
