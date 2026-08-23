# BP-06：持久 ActionRun、Worker Lease 与恢复目标契约

> 历史归档：本文保留当时的目标、过程或证据，不定义当前产品行为。

## 目标

把目前由一次 HTTP 往返完成的工作推进成数据库权威的持久执行循环。网页服务、调度状态和实际 Worker 分离：服务或 Worker 消失后，系统仍能准确说明行动停在哪里、旧 Worker 是否还能写回、能否安全重试，以及用户需要处理什么。

本阶段沿用旧数据但不重解释旧 `action_runs`。新 GoalBranch/Session 执行使用 `goal_action_runs`（领域名称仍是 `ActionRun v2`）；后续只在显式迁移时把旧项目投影接到新模型。

## 核心模型

### ActionRun v2

- 固定 `Project / GoalBranch / Session`、幂等请求、行动种类、所需 Worker 能力、不可变 payload、重试安全性、最大尝试次数和截止时间；
- 状态为 `queued | running | waiting | cancellation_requested | succeeded | failed | cancelled`；
- `safe` 才能在 Worker 丢失后自动重排，`unsafe | unknown` 必须进入 `waiting` 并形成用户可见暂停；
- 每次 claim 增加单调 fencing token，旧 token 即使迟到也不能完成、续租或改变关联 ToolLease；
- 终态不可修改；每次 claim、心跳、失败、恢复、取消和完成另存不可变 Event。

### SchedulerWorker 与 ActionLease

- Worker 先用服务器启动时配置的 bootstrap secret 注册自己的随机 token 摘要和能力；原始 token 不进入数据库或日志；
- claim 使用 PostgreSQL `FOR UPDATE SKIP LOCKED`，同一 ActionRun 和同一 Worker 同时都只能有一个 active Lease；
- Worker 自己生成 claim token，服务器只保存摘要，因此 HTTP 重放不会依赖再次返回秘密；
- Lease 固定 Worker、ActionRun、attempt、fencing、软/硬到期；心跳只能单调续到硬到期；
- Worker 被 kill、服务重启、重复投递或网络丢包后，以持久 Lease 和 fencing 判定，而不相信旧 PID；
- 空队列返回明确的 `retryAfterSeconds`，等待由调用方定时唤醒，不在应用内忙循环。

### ToolLease

- 持续浏览器、开发服务器或调试进程仍使用唯一 `tool_leases` 表，不建立第二套状态；
- 新 Lease 绑定签名插件安装、准确 OCI/入口/Runner 摘要、EnvironmentManifest、workspace snapshot、ActionRun 和资源/时限；
- Action Worker 激活后才能写入受控 endpoint refs；普通 ToolCall 不允许借此留下后台进程；
- Action 心跳同时维护 ToolLease。用户释放、取消、硬到期或 Worker 丢失都会进入终态并要求 launcher 清理其进程/容器；
- 应用永远不挂载 Docker Socket。OCI 启停由注册 Worker/launcher 完成，测试也只在一次性隔离网络使用准确镜像 ID；
- 服务重启期间只要硬到期未越过，原 Worker 可继续带同一 token/fencing 心跳；越过后只能过期并重新申请，不能假装接管未知进程。

### 暂停与通知

- 自动重试耗尽、未知/不安全副作用、ToolLease 丢失和超时会把 Session 安全置为 `exception_paused`；
- 同一事务创建去重的 `goal_attention_items` 与站内 `goal_notifications`，内容包含原因、安全检查点、已尝试、风险、用户动作和建议；
- 站内记录始终存在。外部通知只写入可替换 outbox；没有配置适配器时明确记为 `suppressed`，不冒充已发送；
- 用户显式选择 `retry | fail | cancel` 才能处理 waiting ActionRun，并且只在没有其他未决事项时恢复 Session；
- 哪些情况值得站外打扰用户仍是主观判断，留给 BP-08/BP-09 的设置界面，不在本阶段伪造默认偏好。

## API 边界

- Session：创建、列出、查看、取消和恢复 ActionRun；创建请求以 `Project + clientRequestId + requestHash` 幂等；
- Worker control plane：注册、claim、heartbeat、activate ToolLease、complete/fail/cancel acknowledgement、reconcile；
- ToolLease：Session 创建/查看/停止，Worker 通过关联 ActionLease heartbeat、激活、完成和清理确认；
- 通知：按 Project 列出、标为已读；全部响应不返回保存过的 token 摘要；
- Worker control plane 在未设置 bootstrap secret 时默认关闭。BP-12 还会把它放进私有网络并增加服务身份认证；本阶段不把未认证公网部署视为可用。

## 验收表

- [x] 迁移支持新旧库、重放和数据库负向约束；旧 `action_runs` 与旧项目数据不变。
- [x] 幂等 enqueue、并发 claim、重复投递和单调 fencing 均通过，旧 Worker 无法迟到写回。
- [x] safe ActionRun 在 Worker 被 kill 后自动重排；unsafe/unknown 不盲目重放并持久暂停。
- [x] 服务进程重建后，未过期 Worker 可继续；过期 Lease 可由新 Worker reconcile，且没有忙循环。
- [x] timeout、最大重试、取消竞态和 cancel acknowledgement 形成唯一、可解释终态。
- [x] 签名 OCI ToolLease 真实启动、心跳、受控 endpoint、服务重建续接、释放/取消和崩溃过期；测试后无后台容器或公开端口。
- [x] 失败/等待同事务创建去重 Attention 与站内通知；无外部适配器时 outbox 明确 suppressed。
- [x] Rust 单元、Clippy、SQL、隔离 HTTP/Worker/OCI、生产镜像与完整质量门通过，真实现场指纹不变。

## 暂不锁定

- 生产 launcher 最终用 Kubernetes、systemd、Nomad 还是受限 Docker API；稳定 Worker/Lease 协议不依赖实现；
- 邮件、Push、Slack 等站外通知适配器及打扰策略；没有用户选择前只保证站内通知；
- 多服务器公平性、优先级和配额算法。本阶段保证正确 claim、无双写和 `SKIP LOCKED`，不声称完成大规模调度优化。
