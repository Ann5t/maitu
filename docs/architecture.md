# 架构说明

## 核心决定

这个版本是模块化单体，而不是把前端、API、任务服务拆成多个部署单元。当前规模下，这样能让一次用户动作、数据库事务、事件记录和产物写入保持在同一个可追踪边界中，也能把失去工程文件后的恢复复杂度降下来。

浏览器只接收由 Maud 生成的 HTML、一个 CSS 文件和一段很小的原生 JavaScript。服务端渲染不是临时占位：它是当前正式前端方案。项目图谱的 SVG 边和节点卡片同样由 Rust 根据数据库快照生成。

## 边界

```text
浏览器 / JSON 客户端
        │
        ▼ HTTPS
Caddy（唯一 ingress / 可信代理）
        │
        ▼
Axum 认证、CSRF、限速与传输适配
        │
        ▼
项目用例 ─────── 图谱用例
        │             │
        └──────┬──────┘
               ▼
       SQLx / PostgreSQL
          │          │
          │          ├── 内容寻址文件与 Session inbox
          │          └── 不可变 ContextSnapshot / 来源目录 / 读取审计
          ├── 持久 ActionRun / Worker Lease / fencing / 通知 outbox
          └── 固定 EnvironmentManifest ── Tool Broker ── 外部受信 launcher
                                                   │
                                                   ├── 一次性签名 OCI Worker ── Git CAS
                                                   └── 有界 ToolLease / endpoint / 清理确认
```

- `domain` 不依赖 HTTP 或数据库，保存校验、状态能力和 Markdown 产物生成规则。
- `application` 负责用例、事务和事件记录。
- `web` 只做表单/JSON 解码、响应和页面渲染。`goal_projection` 是可替换的只读投影，不把车道布局写回领域数据。
- `artifacts` 约束文件路径始终位于配置的产物根目录内。
- `migrations` 使用应用内迁移记录，启动时顺序执行且校验迁移名唯一性。

## 数据兼容

Rust 版本直接读取当前 `fudian-nextgen` 的 14 张业务表，不引入第二套影子数据。基线迁移使用 `CREATE TABLE IF NOT EXISTS`、受控的约束补全和幂等索引，因此既能初始化空库，也能接管已有库。

`0002_goal_branch_core.sql` 以只增不减方式添加新 `goal_` 聚合，`0003_tooling_core.sql` 添加插件/环境/工具审计，`0004_input_artifacts.sql` 添加 Session 输入，`0005_ideas_and_project_proposals.sql` 添加想法、立项提案和精确项目来源，`0006_idea_sources.sql` 添加内容寻址的文件/图片/语音来源，`0007_goal_domain_v2.sql` 添加契约修订/来源、探索策略、不可变 Evidence 及撤回/停止/归档约束，`0008_context_memory.sql` 添加精确 Session 快照、完整来源目录、派生索引代际和读取审计，`0009_workspace_runner.sql` 添加托管 Git/worktree、单写 Lease、RunnerJob 与 CAS 操作日志，`0010_signed_real_plugins.sql` 添加发布者、签名安装、自检、安装请求和 ToolExecution/Runner 绑定，`0011_action_scheduler.sql` 添加持久 ActionRun、Worker/Action Lease、fencing、ToolLease 运行证明、通知和 outbox，`0012_review_integration.sql` 添加准确候选冻结、独立审核 Worker 与可恢复父枝干物理集成，`0013_private_security_recovery.sql` 添加 owner、恢复码、会话、持久限速、安全审计和跨存储调和账本，`0014_plugin_resources.sql` 添加摘要绑定的不可变插件内容与 Session 级披露审计。旧 DAG、旧 `action_runs` 与新 Goal ActionRun 并存，应用不会自动重解释旧数据。

已有的 `0001_initial.sql`、`0002_project_evolution_graph.sql`、`0003_selective_branch_merges.sql` 迁移记录会保留；Rust 基线以 `0001_rust_baseline.sql` 单独登记。该记录代表 Rust 已确认数据库具备所需结构，不代表重建或复制已有数据。

## 一致性与幂等

- 每个业务动作在数据库事务中写入状态、节点、边、贡献和事件。
- 开分支、追加进展、暂停和合流接受 `clientRequestId`，重复请求返回成功但不重复写入。
- ActionRun enqueue、Worker 注册/claim、人工恢复、取消、通知已读和 ToolLease 停止均有持久幂等身份；原始 Worker、ActionLease 和 ToolLease token 只在首次交付，数据库只保存摘要。
- claim 每次单调增加 fencing token；旧 Worker、过期 Lease 和迟到完成不能改变 ActionRun 或关联 ToolLease。
- 合流只接受来源分支实际产生的贡献 ID，不能把其他分支的内容错误带回主线。
- 文件产物保存 SHA-256；下载响应返回相同哈希作为 ETag。
- 产物存储路径会在读写前规范化并拒绝 `..`、绝对路径和其他越界片段。

“写文件/Git + 写数据库”不假装成分布式原子事务。正常路径先用临时文件、detached 候选和 Git compare-and-swap 避免暴露半写结果；操作日志保留每个跨存储阶段。`fudian-maintenance` 从数据库权威引用出发扫描四类根目录，把 referenced、missing、digest mismatch 和 orphan 分开记录。孤儿超过默认 7 天后也只能先移动到 `.fudian-quarantine/<run>`，可按 run ID 恢复；当前没有自动永久删除。

## 安全边界

安全模式已包含：

- Maud 默认 HTML 转义
- 外部链接协议和图谱颜色白名单
- JSON/Form 输入长度、枚举和 UUID 校验
- CSP、HSTS、禁止嵌入、MIME 嗅探关闭、Referrer Policy、Permissions Policy 和私有页面 no-store
- setup token 保护的单 owner、Argon2id + pepper、一次性恢复码、随机摘要会话与登录/恢复轮换
- 所有浏览器写请求的精确 Origin、CSRF token、Fetch Metadata 复核，以及持久 IP/账户/Session 速率桶
- 只信任明确 Caddy CIDR 的代理头；ToolLease endpoint 只经认证同源代理和允许 CIDR 访问
- 应用/Caddy 以 UID 1000、只读根、全部 capability drop（Caddy 仅补监听所需 capability）和资源上限运行
- PostgreSQL、应用和工具网络无宿主端口，只有 Caddy ingress 发布 HTTPS；私有模板默认只绑定 `127.0.0.1`

`compose.yaml` 继续作为显式 `FUDIAN_SECURITY_MODE=disabled` 的本机兼容预览，不能公开。真实域名、系统 CA 信任、主机防火墙、异地备份和当前遗留数据库公网映射的维护切换仍是部署者授权边界。

## 测试策略

仓库内单元测试覆盖领域输入、Unicode 标题、分支状态能力、产物路径边界、外部链接/颜色白名单和 HTML 转义。

2026-08-21 的首版验收还在一次性 PostgreSQL 17 容器中通过真实 HTTP 完成了：

1. 创建项目；
2. 确认成果契约；
3. 生成并批准 Markdown 产物；
4. 从主线创建探索分支；
5. 记录候选证据；
6. 选择该证据合回主线；
7. 创建第二分支并记录暂停/重开条件；
8. 读取图谱、契约、产物、历史页面及 JSON 快照；
9. 重放同一 `clientRequestId`，确认没有重复分支。

隔离验收最终得到 3 条分支、9 个节点、9 条边、5 项贡献、1 个文件产物和 1 次合流。一次性数据库在核验后删除，未写入现有项目。

2026-08-22 增加了 `scripts/quality-gate.sh` 作为本地与 CI 共用的单一完整入口：

1. 在固定 Rust 1.97 开发镜像中运行 rustfmt、Clippy `-D warnings` 和全部 Rust 测试；
2. 对空库、迁移 SQL 重放和带旧 DAG fixture 的数据库验证只增不减迁移；
3. 分别通过真实 HTTP 跑通想法立项、目标枝干、固定窗口上下文、插件环境、真实 Git/Runner、持久调度恢复、签名 OCI 工具、文件输入和结构化工作台流程；
4. 在固定 Playwright 容器中用真实 Chromium 验证 1440px、820px、390px、上传、立项、完整审核，以及 100 枝干/300 Session 的大图预算；
5. 构建 runtime 镜像，以 UID 1000、只读根文件系统、随机 `127.0.0.1` 端口和一次性 PostgreSQL 启动，再验证健康、静态资源、14 个迁移和隔离写入；
6. 构建固定 Rust、C/C++、Python 双版本与 Playwright/Chromium 插件镜像，验证断网 Worker、入口摘要、资源边界、签名/撤销、环境冲突、Skill/reference 摘要与渐进披露、Git CAS，以及真实 ToolLease 启动、服务重建续接、释放和 launcher 崩溃清理。
7. 通过隔离 HTTPS 验证 owner/会话/CSRF/限速/恢复码/ToolLease 代理及三尺寸 Chromium；通过真实安全 Compose 验证网络、文件 secret、唯一入口和资源边界；最后完成旧版 12 迁移备份、带标签空目标恢复、14 迁移升级、Git/对象校验和旧应用回退。

`.github/workflows/ci.yml` 不另造一套 CI 特例，而是直接运行该入口。测试脚本按唯一进程后缀命名容器和网络，并用 `trap` 清理；任何失败都会保留应用日志，但不会连接 `fudian_nextgen_postgres_data` 或 `fudian_nextgen_artifacts`。
