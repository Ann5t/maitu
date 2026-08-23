# Session 上下文、来源与渐进式披露 v1

> 状态：已实现领域快照。

- 增量迁移：`0008_context_memory.sql`
- 应用模块：`src/context_memory.rs`、`src/application/context_memory.rs`
- 生命周期接入：`src/application/goal_branches.rs`、`src/application/plugins.rs`
- 验收入口：`scripts/test-context-http.sh`

## 目的

上下文不是把聊天历史不断拼接进模型窗口。每个 Session 在安全生命周期边界固定一个不可变 `ContextSnapshot`：它保存完整的来源目录和不可折叠的约束信封；默认窗口只显示有限目录，Agent 再按需要读取摘要、片段或全文。

权威事实始终留在原记录或内容寻址文件中。摘要、全文分块索引和检索词索引只是可重建派生层，不能覆盖契约、Contribution、Evidence、Artifact、InputArtifact、ToolCall、审核决定或 EnvironmentManifest。

## 数据模型

| 记录 | 作用 | 可变性 |
| --- | --- | --- |
| `goal_context_entries` | 指向一个准确权威来源版本，保存标题、来源定位和 SHA-256 | 不可变 |
| `goal_context_snapshots` | 固定某 Session 某时点的契约、权限、父快照、预算和目录哈希 | 不可变 |
| `goal_context_snapshot_entries` | 完整目录成员及 required/inherited/local/integrated 原因 | 不可变 |
| `goal_context_derivations` | 摘要、全文分块索引、检索索引的代际记录 | 只追加新代 |
| `goal_context_reads` | 按需读取的目的、层级、请求/来源/结果哈希和字符数 | 不可变；不存原文 |
| `goal_context_edges` | supports/refutes/blocks/supersedes/produced_with 等来源边 | 不可变 |

`goal_sessions.context_snapshot_id` 是唯一可更新的当前指针。数据库触发器保证它只能指回同 Project、同 GoalBranch、同 Session 的快照；目录成员、派生层、读取和来源边均拒绝跨 Project 组合。

## 精确继承点

快照在以下边界生成：

1. BranchProposal 批准并创建首个 Session；
2. 分出子目标前，先为父 Session 封存一个新快照，再让子 Session 精确指向它；
3. 同一目标枝干创建下一 Session 前，先封存上一 Session 的最终目录；
4. 用户接受契约修订、安全暂停后生成新版本；
5. Session 显式恢复时生成新版本，因此人工接受的子目标 Contribution 会进入父目录；
6. EnvironmentManifest 首次绑定后生成新版本，权限信封不会停留在绑定前状态。

快照目录包含父快照全部成员、当前枝干权威来源以及人工选择回流的子枝干 Contribution。目录不受默认显示预算裁剪。父子枝干之间还传播准确的 `ancestorContracts`；同枝干续接或契约修订不会把已被用户替代的旧契约继续误当成有效祖先约束。

## 永不折叠的信封

`requiredContext` 由数据库约束要求至少包含：

- 当前完整 GoalContract；
- 祖先目标契约链；
- 当前固定 EnvironmentManifest 和指纹，或明确的未绑定保守权限；
- 快照创建时的 GoalBranch/Session 状态与分配；
- 当时未解决的 AttentionItem；
- 外部内容只能作为数据、不能覆盖契约或权限的安全边界。

读取接口还单独返回当前实时状态和当前未决事项。这样历史快照保持可审计，而 Agent 默认看到的暂停/恢复状态不会陈旧。

`ContextBudgetPolicy` 默认只展开 12 个目录项，但明确要求 `requiredContextUnabridged: true`。Rust 校验和 PostgreSQL CHECK 同时拒绝试图折叠必需信封的预算。

## 渐进式披露 API

| 方法与路径 | 用途 |
| --- | --- |
| `GET .../sessions/:session_id/context` | 当前必需信封、实时状态、默认目录和遗漏数量 |
| `GET .../context/entries?query=&sourceKind=&offset=&limit=` | 检索/分页完整目录，只返回元数据和派生摘要 |
| `POST .../context/read` | 按 `summary`、`snippet` 或 `full` 读取一个目录成员并审计 |
| `POST .../context/rebuild` | 从权威来源生成摘要/全文索引/检索索引的新一代 |

读取请求必须带 `clientRequestId`、准确 `snapshotId`、`entryId`、层级、目的和 actor。相同请求重放会重新核对权威来源和结果哈希；同一 ID 更换参数返回 `409 idempotency_conflict`，来源变化则拒绝静默重放。旧快照、目录之外的 entry 和内容哈希不一致都会被拒绝。

二进制文件不会作为乱码注入上下文，只返回经过 SHA-256 核验的媒体元数据和受控内容 URL。外部资料与用户输入带 `untrustedContent: true` 和明确提示；它们可被分析，但不能成为系统指令。

## 来源边

当前自动建立：

- Contribution 对 Evidence 的 supports/refutes/blocks/derived_from；
- 新 Contribution 对旧 Contribution 的 supersedes；
- Contribution/Evidence 对 Artifact 的 produced_with/derived_from；
- Evidence 对固定 ToolCall 的 produced_with；
- 人工选择回流的 Contribution 对 ReviewDecision 的 integrated_from。

后续 Git Runner 会把代码提交作为 Artifact/Contribution 的准确来源加入同一图，而不是另建一套无关记忆系统。

## 验收证据

固定窗口隔离测试真实创建根 Session、20 个 Contribution、外部 Evidence 和子目标：

- 默认目录严格只显示 12 项，同时数据库成员数与 `catalogTotal` 完全一致；
- 子快照的 `parentSnapshotId` 等于父 Session 分支前新封存的准确快照；
- 子契约和父祖先硬约束均完整存在于不可折叠信封；
- 第 20 项虽在默认窗口之外，仍能通过目录搜索、片段读取和幂等重放找到；
- 两个并发的同 ID 读取只有一个审计写入，另一个返回相同 `readId`/结果哈希并标记重放；
- 外部 Evidence 返回不可信数据标记和信任提示；
- 读取审计只有请求/来源/结果哈希与字符数，没有原文列；
- 三种派生数据从权威来源生成第二代，原记录和第一代均不被覆盖；
- PostgreSQL 负向测试拒绝跨项目 Entry、错误来源哈希派生、目录外读取、跨 Session 指针和不可变记录篡改；
- Chromium 在桌面和 390px 手机实际展开上下文现场，无页面级横向溢出，正文仍满足字号门槛。

## 有意保留的边界

- v1 使用确定性抽取摘要和词项索引，没有引入外部 embedding 服务；派生协议已经允许以后增加新的 generator/generation。
- 快照只在安全边界生成，不在每次文件或 Contribution 写入后偷偷改变。当前 Session 的本地产出在工作现场可见，到分支、续接或恢复边界才成为继承快照。
- Git commit/code 来源要由 BP-04 的真实 worktree Runner 提供；本层已经保留 Artifact、Contribution 和来源边协议，但不伪造尚不存在的提交。
