# 中央插件、环境与文件协议 v0.1

- 状态：里程碑 1 的实现契约
- 上位语义：[`product-design.md`](product-design.md) 第 10—13 节
- 适用实现：`feat/goal-branch-core-v0.1`

## 1. 设计边界

目标枝干 worktree 只保存项目输入、源码、声明文件和有意义的产出。C/C++、Python、Rust、Playwright、PPTMaster 等能力由中央版本化插件目录提供，Session 不得在自己的 worktree 里下载或维护一套工具环境。

中央共享的是不可变包、镜像和内容寻址缓存，不是一个所有 Session 共同修改的 Python/Rust/系统依赖环境。实际执行发生在受限 Worker 中；worktree 本身不是安全边界。

协议最初以 Mock 插件验证注册表、确定性环境指纹和 Broker trait；BP-05 加入 Ed25519 签名安装、固定 OCI 镜像/入口、真实一次性 Worker 和 Git CAS 回写，BP-06 又以持久 ActionRun/Worker Lease/fencing 完成真实 ToolLease 启停与崩溃恢复。BP-10 补齐签名内容包、不可变 Skill/reference 存储、Session 级提示词引导和按需读取审计。MCP 最终采用 stdio、HTTP 还是进程内适配仍保持未知，但不再阻塞稳定内容/能力语义，详见 [`plugin-system-v1.md`](plugin-system-v1.md)、[`action-scheduler-v1.md`](action-scheduler-v1.md) 与 [`tooling-api-v1.md`](tooling-api-v1.md)。

## 2. 标识、版本与摘要

- 插件 ID 使用稳定的小写反向域名或命名空间形式，例如 `fudian.tools.rust`。
- `version` 必须是完整 SemVer，已解析引用中禁止 `latest`、范围和通配符。
- `contentDigest` 使用 `sha256:<64 lowercase hex>`，覆盖规范化 Manifest 及包内受保护内容；计算时 Manifest 自身的摘要字段为空。
- 插件身份是 `(pluginId, version, contentDigest)`。同名不同版本或同版本不同摘要可以存储，但解析同一版本出现多个摘要时必须报供应链冲突，不能静默选择。
- `latest` 只允许出现在用户/Agent 的解析请求中；注册表立即把它解析为准确版本与摘要并持久化，Session 环境里永不保存 `latest`。

## 3. PluginManifest

规范 JSON 结构如下；可选字段省略而不是写入含义不明的 `null`：

```json
{
  "schemaVersion": 1,
  "pluginId": "fudian.tools.example",
  "version": "1.0.0",
  "contentDigest": "sha256:…",
  "displayName": "Example tools",
  "description": "Short progressively disclosed catalog text",
  "capabilities": ["text.inspect"],
  "permissions": {
    "network": "denied",
    "workspaceRead": ["**/*"],
    "workspaceWrite": ["out/**"],
    "externalWrites": false
  },
  "tools": [
    {
      "name": "inspect",
      "description": "Inspect text without changing the input",
      "inputSchema": {},
      "outputSchema": {},
      "idempotency": "pure"
    }
  ],
  "skill": { "entry": "SKILL.md" },
  "runtime": {
    "kind": "mock",
    "contentDigest": "sha256:…",
    "entrypoint": "inspect"
  },
  "assets": [
    { "path": "SKILL.md", "contentDigest": "sha256:…" },
    { "path": "references/usage.json", "contentDigest": "sha256:…" }
  ],
  "resourceHints": { "cpuMillis": 1000, "memoryMiB": 64, "timeoutSeconds": 30 }
}
```

约束：

1. 未知 `schemaVersion` 拒绝加载。
2. 工具名在插件版本内唯一；能力和写入路径排序、去重且有数量/长度上限。
3. Manifest 声明的是最大需求，实际调用还必须被枝干授权收窄。
4. `externalWrites: true`、私人网络、凭据或宿主设备访问不能靠插件自我声明获得，必须有系统级能力授予。
5. Skill、工具 schema 和 references 按需披露；Session 默认只看到 ID、固定版本、摘要、简介与能力目录。
6. Runtime 必须用内容摘要固定。容器引用使用镜像 digest，不接受只含可漂移 tag 的运行环境。
7. Skill 入口必须对应一个 Asset。签名安装必须提交与 Assets 路径集合完全相同且逐项摘要匹配的内容；目录和详情只给元数据，绑定 Session 的 Worker 才取得 Skill 正文，其他资源必须显式按需读取。

## 4. EnvironmentManifest 与确定性指纹

每个 Session 固定一个不可变 EnvironmentManifest：

```json
{
  "schemaVersion": 1,
  "baseRuntime": { "kind": "oci", "digest": "sha256:…" },
  "plugins": [
    { "pluginId": "fudian.tools.rust", "version": "1.85.1", "contentDigest": "sha256:…" }
  ],
  "toolchains": { "rust": "1.85.1" },
  "dependencyLocks": [
    { "path": "Cargo.lock", "sha256": "…" }
  ],
  "targetPlatform": "x86_64-unknown-linux-gnu",
  "features": ["default"],
  "buildParameters": {},
  "networkPolicy": "public_read_only",
  "resourcePolicy": { "cpuMillis": 2000, "memoryMiB": 2048, "diskMiB": 4096, "timeoutSeconds": 900 },
  "environmentPolicy": { "allowedNames": ["CI"], "secretReferences": [] }
}
```

环境指纹算法：

1. 校验所有插件引用都已固定 SemVer 和摘要，所有 lock 哈希为小写十六进制。
2. 插件按 `(pluginId, version, contentDigest)` 排序；依赖锁按规范相对路径排序；features 和环境变量名排序去重；map 使用 Unicode 码点顺序的键。
3. 路径只允许 `/` 分隔的规范相对路径，拒绝绝对路径、`.`、`..`、NUL 与平台相关前缀。
4. 对结构进行无多余空白的稳定 UTF-8 JSON 序列化；数字字段均为非负整数，不使用浮点数。
5. 计算 `sha256(canonical_json)`，表示为 `sha256:<hex>`。

相同语义必得相同指纹；插件摘要、工具链、lock 内容、目标、feature、构建参数、网络/资源/环境策略任一变化都产生新指纹。秘密的值不进入 Manifest；只记录由安全系统解析的引用 ID，因此指纹不会泄露秘密。

子 Session 默认引用枝干当前环境。子枝干继承父环境 ID；依赖变化创建新 EnvironmentManifest，不修改父环境。内容缓存可以按摘要/指纹去重，但 Worker 的可变层、进程、home、包管理目录和凭据绝不跨环境共享。

## 5. Tool Broker 协议

### 5.1 ToolCall

```json
{
  "callId": "uuid",
  "clientRequestId": "uuid",
  "projectId": "uuid",
  "goalBranchId": "uuid",
  "sessionId": "uuid",
  "plugin": { "pluginId": "…", "version": "1.0.0", "contentDigest": "sha256:…" },
  "toolName": "inspect",
  "input": {},
  "environmentFingerprint": "sha256:…",
  "baseWorkspaceSnapshot": "sha256:…",
  "allowedWrites": ["out/**"],
  "timeoutSeconds": 30
}
```

调用前置条件：Session 为 `running`；插件存在于该 Session 环境；工具和权限已声明；调用授权不宽于 Manifest 与枝干授权的交集；工作区基线仍等于请求快照；幂等键未被不同输入使用。

### 5.2 ToolResult

```json
{
  "callId": "uuid",
  "status": "succeeded",
  "output": {},
  "baseWorkspaceSnapshot": "sha256:…",
  "resultWorkspaceSnapshot": "sha256:…",
  "environmentFingerprint": "sha256:…",
  "changeSet": [],
  "artifacts": [],
  "evidence": [],
  "logReference": "artifact:uuid",
  "retrySafety": "safe",
  "startedAt": "…",
  "completedAt": "…"
}
```

`status` 为 `succeeded | failed | timed_out | cancelled | workspace_conflict | policy_denied`；`retrySafety` 为 `safe | unsafe | unknown`。日志必须先脱敏，不能直接把环境、Cookie 或 Token 保存为普通 Artifact。

### 5.3 原子执行过程

1. Broker 持久化 ToolCall，并以 `clientRequestId + input hash` 去重。
2. 校验 Session、环境、权限和基础快照，获取该枝干短期写租约。
3. 启动只含固定 Runtime/插件的临时 Worker；父快照只读，可写输出层独立。
4. Worker 执行并返回结构化结果、候选 change set、产物和日志。
5. Broker 验证写入路径、大小、秘密扫描和基线是否仍未变化。
6. 校验成功后原子应用允许的 change set，并登记 Artifact/Evidence/Event；冲突时不部分写回。
7. 销毁 Worker 和未选择的可变层，释放租约。

ToolResult 必须绑定准确 Session、插件摘要、环境指纹和输入快照。仅有自由文本“执行成功”不构成可复验证据。

### 5.4 Broker trait 的 v0.1 形状

Rust 应用层使用与传输无关的接口：

```text
resolve_plugin(selector) -> ResolvedPluginRef
describe_plugin(resolved_ref) -> PluginManifest
execute(call, workspace_capability) -> ToolResult
acquire_lease(request, workspace_capability) -> ToolLease
heartbeat_lease(lease_id, token) -> ToolLease
release_lease(lease_id, token, retained_outputs) -> ToolLeaseResult
```

模拟插件必须走同样校验与审计链，但只处理内存/临时目录中的确定性输入，不依赖 Docker、浏览器或网络。它用于证明协议，不代表 Runner 隔离已经完成。

## 6. ToolLease

开发服务器、交互浏览器、调试器等短期持续状态使用显式 Lease，普通一次性调用不得悄悄留下后台进程。

`ToolLease` 包含：

- lease ID、Session、固定插件/工具/环境/工作区快照；
- `status`：`requested | active | expired | released | failed | cancelled`；
- 不可猜测的续租 token 摘要；
- CPU/内存/磁盘/进程/端口限制；
- 创建、最后心跳、硬到期时间；
- 日志和可访问端点的受控引用；
- 释放时选择保留的输出。

转换使用统一命令幂等规则：

| 命令 | 当前 → 新状态 | 前置条件 | 输出与事件 |
| --- | --- | --- | --- |
| `lease.acquire` | 不存在 → `active`/`failed` | Session running；能力允许；配额可用 | Lease + `tool_lease.acquired` 或失败审计 |
| `lease.heartbeat` | `active` → `active` | token 匹配；未过硬到期；Session 仍允许 | 延长软到期 + `tool_lease.heartbeat`（可采样） |
| `lease.release` | `active` → `released` | token 匹配 | 终止进程、选择产物、`tool_lease.released` |
| `lease.expire` | `active` → `expired` | 系统时间超过到期；仅系统 Actor | 强制终止、`tool_lease.expired`、必要时异常暂停 Session |
| `lease.cancel` | `requested`/`active` → `cancelled` | 用户或授权系统动作 | 清理资源 + `tool_lease.cancelled` |

服务重启后只能依据持久 Lease、ActionLease fencing 和 launcher 实际状态恢复；不能假设旧 PID 仍对应原进程。BP-06 的物理验收已证明服务容器可被杀死并重建而持续 endpoint 保持，launcher 丢失则转为 `expired`、暂停 Session 并要求 `tool.cleanup` 明确确认，不能原地复活未知进程。

## 7. InputArtifact 协议

### 7.1 状态与元数据

InputArtifact 实际关联一个 Session，状态为：

`staging → verified → available → imported`

失败分支为 `rejected | quarantined`。记录至少包括：

- 服务器生成的 ID 与存储键；
- project/GoalBranch/Session；
- 客户端文件名（仅展示）、规范化显示名和可信 media type；
- 客户端声明大小、实际大小、SHA-256；
- 上传者、来源、创建时间；
- 可选分片清单与完整内容摘要；
- 病毒/压缩包/策略检查结果；
- 导入方式 `worktree_copy | artifact_reference | read_only_mount`；
- 导入后的相对路径或 Artifact 版本引用。

### 7.2 上传与导入转换

所有写命令使用 `(project_id, client_request_id)` 幂等收据；重复完成请求返回同一个 InputArtifact。

| 命令 | 当前 → 新状态 | 关键前置条件 | 原子输出与事件 |
| --- | --- | --- | --- |
| `input.begin` | 不存在 → `staging` | Session 存在；未冻结；声明大小在限额内 | 服务器存储 ID、分片策略、`input.upload_started` |
| `input.append_chunk` | `staging` → `staging` | 分片偏移/摘要匹配；累计不超限；只写授权暂存对象 | 分片收据；不写 worktree |
| `input.finish` | `staging` → `verified`/`rejected`/`quarantined` | 实际大小、完整哈希、文件策略与压缩包边界检查完成 | 可信元数据 + `input.verified` 或拒绝事件 |
| `input.publish` | `verified` → `available` | 暂存对象原子移动到内容寻址仓库 | InputArtifact 可读引用 + `input.available` |
| `input.import` | `available` → `imported` | Session `running`；基线匹配；目标路径安全且不冲突 | 小文件原子复制或不可变引用；`input.imported`、Session 输入事件 |

### 7.3 文件安全不变量

1. 存储路径只由服务器生成；客户端文件名永不直接参与磁盘路径拼接。
2. worktree 目标是规范相对路径，拒绝绝对路径、`..`、空段、NUL、Windows drive/UNC 前缀、符号链接逃逸和大小写折叠冲突。
3. 上传按流计数，不能只信 `Content-Length`；超限立即停止并删除/隔离暂存片段。
4. ZIP/TAR 等归档在解包前检查文件数、展开总量、压缩比、嵌套深度、特殊文件、硬链接/符号链接和路径穿越。v0.1 可以只保存而不解包未知归档。
5. 同内容可在存储层按哈希去重，但每次逻辑输入仍有独立来源和授权记录。
6. `awaiting_merge_review`、`accepted`、`review_rejected` 或其他非 running Session 不允许导入到其冻结现场；新文件必须关联后续 Session。
7. 下载按项目/Session 授权，通过 ID 查元数据后读取，不接受任意服务器路径。
8. 浏览器提供的 media type 只作提示；响应下载时使用可信检测结果、`Content-Disposition` 和 `X-Content-Type-Options: nosniff`。

## 8. 审计与失败语义

插件注册、解析、ToolCall、ToolResult、Lease、InputArtifact 状态变化都写入 GoalEvent 或专用不可变审计记录。审计载荷保存摘要和引用，不复制可能含秘密的大日志。

稳定错误码至少包括：

- `plugin_not_found`、`plugin_digest_conflict`、`unresolved_plugin_version`；
- `environment_fingerprint_mismatch`、`workspace_snapshot_conflict`；
- `tool_not_allowed`、`policy_denied`、`lease_expired`；
- `upload_too_large`、`invalid_upload_offset`、`artifact_hash_mismatch`；
- `unsafe_artifact_path`、`archive_policy_rejected`、`candidate_frozen`。

超时、进程崩溃或网络错误不自动说明可安全重试。只有 Manifest 和 Result 都标记为纯操作/安全幂等，且没有观察到外部写入时，调度器才能有限重试。

## 9. 本阶段不锁定

- 插件市场页面、签名根和发布审批的最终形式；
- MCP 是进程内、stdio 还是网络传输；
- OCI、Wasm 与专有软件 Runner 的最终组合；
- 私有网络、身份认证和秘密管理产品；
- PPTMaster 等具体插件的参数设计；
- 项目图如何可视化 ToolCall、Lease 与 InputArtifact 事件。

这些选择不能改变固定版本、环境隔离、最小权限、原子写回、冻结候选和可追溯性不变量。
