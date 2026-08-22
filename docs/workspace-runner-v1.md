# BP-04：Git worktree 与隔离 Runner 目标契约

## 目标

把数据库中的 `GoalBranch` 从“保存 Git 占位字段”提升为真实、可核对、可恢复的工作现场：每条已批准目标枝干拥有独立 Git branch/worktree；同一枝干同一时刻只有一个带 fencing token 的写 Lease；命令在不接触宿主秘密或其他枝干的临时 Worker 中运行，候选输出通过路径、摘要、资源和基线复核后才回写当前 worktree。

本阶段不自动执行物理父枝干合并。子枝干的工作现场可以产生可冻结提交，但跨枝干回流仍停在 ReviewGate，留给 BP-07 完成“独立复验—用户决定—真实整合—父目标复验”。

## 硬约束

- Git 仓库、worktree 和 Worker 输出都只能落在配置的托管根目录；数据库只保存规范相对 key，不接受客户端宿主路径。
- GoalBranch 的 Git branch 名由系统 ID 确定，不能以用户文本拼接命令；所有 Git 调用使用参数数组，不经过 shell。
- 根枝干从托管仓库默认 HEAD 创建；子枝干从父 Session 在安全暂停点对应的准确、干净 HEAD 创建。
- 一个 GoalBranch 只有一个 worktree；一个 worktree 只有一个 `active` 写 Lease。并行只能先批准子目标枝干。
- Lease 固定 Session、基线 commit/snapshot、允许写路径、能力、资源限额、到期时间和单调 fencing token。过期或旧 token 永远不能写回。
- Worker 的输入 worktree 只读，输出层独立可写；Worker 不挂载宿主 home、SSH、浏览器资料、Docker Socket、数据库或其他 worktree。
- 网络、外部写、账号引用、付费和部署均为显式能力。v1 Worker 默认且可验证地断网；没有安全适配器的高风险能力即使被声明也不能静默执行。
- CPU、内存、输出磁盘、进程数和墙钟时间都有上限；资源失败不回写部分文件，而是形成可恢复的异常暂停与待处理项。
- 输出清单必须覆盖输出层全部普通文件；显式删除清单必须进入 Job spec 与 Lease 审计快照。新增、覆盖和删除都受相同写路径授权；输出与删除冲突、删除非跟踪文件、绝对路径、空段、`.`、`..`、反斜杠/Windows drive/UNC、NUL、符号链接逃逸、摘要不符、未授权路径和额外文件全部拒绝。
- 回写前再次核对 worktree 基线。候选先在独立 apply worktree 形成 Git commit，再以 compare-and-swap 更新目标 ref；操作日志允许检查“Git 已前进、数据库尚未确认”的中间状态。
- 不暴露或修改现有生产数据库、生产 volume、用户远程仓库、GitHub、真实账号或公开部署。

## 最小交互

用户仍只批准 BranchProposal。权限策略与目标契约一起版本化；没有填写时采用明确的保守默认值：读当前/父快照、写当前枝干、断网、无外部写、无账号、无付费、无部署，并使用中等资源上限。Session 内在固定边界内调用不逐次找用户审核。

Agent 请求执行时提供准确 workspace snapshot、结构化 argv、允许写路径和更窄或相等的资源需求。系统返回不含宿主绝对路径的 Runner spec 与一次性 Lease token；调度器只需把准确输入只读挂载、该 job 输出目录可写挂载，并按 spec 启动临时 Worker。

## 验收表

- [x] 批准根/子 BranchProposal 后分别产生真实 Git branch/worktree；数据库记录的 base、HEAD、tree、snapshot 与 Git 实测一致。
- [x] 子 worktree 的基线等于父安全点 HEAD，父 worktree 在子执行后保持不变。
- [x] 两个并发请求争抢同一 worktree 时恰好一个取得 Lease；旧 fencing token、过期 Lease 和错误基线均无法写回。
- [x] 正常 Worker 通过只读输入/独立输出形成提交；新增、覆盖和显式删除均可审计，重放不重复提交，额外或摘要不符输出不产生部分写入。
- [x] 绝对路径、`..`、Windows/UNC、符号链接逃逸、越权路径和 worktree 人工变脏均被拒绝。
- [x] Worker 实测无 Docker Socket/宿主秘密挂载、无有效 capabilities、`no-new-privileges`、只读根文件系统、输入只读和断网；伪造不安全证明被 Broker 拒绝。
- [x] CPU、内存、磁盘、进程数和时间限制均进入固定 spec；至少覆盖超时、超量输出、进程/内存耗尽，失败后 worktree HEAD 不变且 Session 有可恢复暂停。
- [x] 空库、迁移重放、旧 fixture、SQL 负约束、Rust 单元、真实 HTTP/Git/Worker 流程和完整质量门通过。
- [x] 生产容器 ID、启动时间、迁移、数据计数和既有 Artifact 摘要只读复核不变。

完成证据：`0009_workspace_runner.sql`、`test-workspace-runner-http.sh` 和完整 `quality-gate.sh` 于 2026-08-22 通过。隔离 Worker 可执行文件摘要为 `sha256:cce4e703c3a4fe43c06a9415cfd5b328b910a9e5927b1f694d7fc1649e6f74b4`；它只是本次构建证据，Job 始终绑定实际配置的不可变摘要，不把 `latest` 当持久身份。

## 当前未知与延后边界

- 允许联网时采用域名代理、请求代理还是网络策略引擎，由 BP-05 代表性联网插件验证后选择；当前不会用“容器有默认网络”冒充细粒度授权。
- Worker 自动领取、心跳、服务重启恢复和取消竞态属于 BP-06；本阶段保留持久 job/Lease/操作状态，并由测试调度器显式启动临时 Worker。
- 物理父整合与冲突 UI 属于 BP-07/BP-08；本阶段只保证每条枝干能提供准确候选 commit 和证据。
- 远程仓库 clone/push 需要账号与外部副作用授权，不在本阶段默认权限中；先使用服务器内托管 bare repository 验证完整语义。
