# Session 安全文件输入 API v1

本接口实现 [`tool-protocol.md`](tool-protocol.md) 中 `InputArtifact` 的第一条可运行切片。它把“用户选中的本地文件”与“Session worktree 中的可写文件”分开：先分段上传、验证和建立可审计输入，再由 running Session 显式导入。

## 资源与状态

`InputArtifact` 遵循：

```text
staging ──完整分段 + 摘要/媒体检测通过──> available ──显式导入──> imported
   └──摘要不符──> rejected
```

服务器保留客户文件名作为显示元数据，但存储键始终由服务器生成。完整文件以 SHA-256 内容寻址；同一内容可被多个逻辑输入引用，但不复制对象。客户声明的 media type 只记录不信任，下载与导入使用服务器检测的可信类型。

## 路由

| 方法与路径 | 用途 |
| --- | --- |
| `GET /api/v1/projects/:project_id/sessions/:session_id/inputs` | 列出该 Session 的输入 |
| `POST .../inputs` | 声明文件名、大小和可选 media type，建立 `staging` 输入 |
| `PUT .../inputs/:input_id/chunks?clientRequestId=...&offset=...&sha256=...` | 上传受限大小的原始字节分段 |
| `POST .../inputs/:input_id/finish` | 检查连续 offset、重算分段/完整摘要并固定可信元数据 |
| `POST .../inputs/:input_id/import` | 将已验证输入导入 running Session |
| `GET .../inputs/:input_id/content` | 授权范围内下载，带可信 `Content-Type`、ETag 与 `nosniff` |

所有写请求都包含 UUID `clientRequestId`。相同 ID 与相同输入返回 `replayed: true`；相同 ID 改变输入则返回 `idempotency_conflict`。分段可以乱序到达，但不得重叠、超出声明大小或在 `finish` 时留有缺口。

## 导入策略

- 小于 `INPUT_INBOX_COPY_MAX_BYTES` 且通过 UTF-8/NUL 检查的文本、JSON、XML 或 SVG，使用 `worktree_copy`。
- 二进制或较大文件使用 `artifact_reference`，不把它们复制进工作目录。
- ZIP/gzip 归档只保存、不解压，`verification.archiveExtracted` 恒为 `false`。后续解压必须作为单独的受限工具调用。
- 可选 `inboxRelativePath` 必须是规范相对路径；绝对路径、`..`、空段、Windows drive/保留名被拒绝。
- inbox 使用 `create_new`：同名文件返回 `filename_conflict`，不会静默覆盖。

`INPUT_MAX_BYTES`、`INPUT_CHUNK_MAX_BYTES` 和 `INPUT_INBOX_COPY_MAX_BYTES` 控制总大小、单段大小和 worktree 复制阈值，默认分别是 64 MiB、4 MiB 和 1 MiB。

## 冻结与审计边界

只有 `running` Session 能开始上传、追加新分段、完成上传或导入。一旦 Agent 提出拟合并，Session 变为 `frozen_candidate`，上述新写入返回 `candidate_frozen`；已完成请求的宍等重放仍可返回原结果。

上传开始、每个分段、验证、可用、拒绝和导入都写入不可变 `GoalEvent`。分段记录也不可更新/删除。当前版本尚无身份认证，所以这些端点只能放在本机或受信任的私有反向代理之后。
