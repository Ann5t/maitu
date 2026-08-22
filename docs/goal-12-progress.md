# Fudian 第一阶段 12 方面总 Goal：执行进度

- Goal thread：`01a024d6-1224-7fe1-a7f2-ea8dafd226e4`
- 启动时间：2026-08-22T10:39:39+08:00
- 总验收矩阵：[`design-12-aspects.md`](design-12-aspects.md)
- 产品基线：[`product-design.md`](product-design.md)
- Git 枝干：`feat/goal-branch-core-v0.1`
- 状态：执行中

## 启动基线

- 前一个有限 Goal 的最后提交为 `ff9a358`；用户已在 Git 中配置 `origin/feat/goal-branch-core-v0.1`，本 Goal 不推送远端；
- 工作树带有第 11 项 Session 02 的可读性候选：CSS、Chromium 断言和两张新截图，尚未拟合并；
- 原预览 `fudian-nextgen-app-prod-1` 仍只绑定 `127.0.0.1:3001`；
- 真实数据库计数为 3 个项目、8 条旧枝干、23 个旧节点、2 个产物；
- 真实产物路径/内容指纹为 `f0f167bf097e0bc6d8f7c821b6af98eb065acd0ab0281462c07610a65e5aff0e`；
- 原 PostgreSQL 仍存在启动前即有的 `0.0.0.0:55432` 映射，列为第 12 项必须处理的风险；
- 隔离审查环境使用 `fudian-review-app-v01`、`fudian-review-db-v01` 和 `127.0.0.1:3012`，不挂载真实数据卷；
- 当前没有真实 Agent Runner、worktree manager、后台队列、认证或公开部署授权。

## 总状态

| # | 方面 | 状态 | 当前工作单元 |
| --- | --- | --- | --- |
| 1 | 想法 / 项目 | 待用户判断 | BP-01 机器门通过；等待用户体验两种投影 |
| 2 | 核心领域 | 完成 | BP-06 已补齐同一 Session 内持久执行与崩溃恢复 |
| 3 | 目标契约 | 完成 | BP-02 已通过契约演进与探索模式验收 |
| 4 | 执行循环 | 完成 | BP-07 已补齐冻结候选、独立审核、选择性父整合、CAS、冲突暂停与崩溃恢复 |
| 5 | 上下文与记忆 | 完成 | BP-03 精确快照、来源目录与按需披露已通过完整质量门 |
| 6 | worktree 与隔离 | 完成 | BP-04 全部机器门和既定用户权限语义通过 |
| 7 | 中央插件 | 部分实现 | 一次性/持续 OCI 与按需插件/运行现场 UI 完成；BP-09 补认证 endpoint proxy 和保留策略 |
| 8 | 数据与恢复 | 部分实现 | Git CAS/操作日志完成；BP-09 补 reconcile 与完整恢复演练 |
| 9 | 后台执行 | 完成 | BP-06 队列、Lease/fencing、恢复、取消、暂停和通知矩阵已通过 |
| 10 | 证据与审核 | 完成 | BP-07 已验证准确 commit/环境绑定、独立只读复验、父契约复验及用户唯一终审权 |
| 11 | 多设备工作台 | 待用户判断 | BP-08 机器门完成；等待用户实际体验 `goal-worksite-v2` |
| 12 | 私有部署 | 部分实现 | BP-09 待开始 |

## 运行日志

### 2026-08-22 10:39 +08:00

- 用户明确要求创建一个 Goal，把昨晚讨论的 12 个方面全部完成；
- 创建新的持久总 Goal，不设置虚假时间或 token 上限；
- 明确 12 项必须逐项有设计、实现、验证、风险和用户判断，不能再用有限纵向版本代替整体完成；
- 记录禁止自行合并/push/公开部署、禁止测试写真实数据及遇到主观或外部授权问题暂停的边界；
- 建立本执行进度和唯一总验收矩阵。

### 2026-08-22 10:43 +08:00

- 用户明确退回初版工作台，指出文字难以看清；隔离审查库将 Session 01 候选记为 `rejected`，在同一 GoalBranch 创建 Session 02；
- 审计确认初版大量正文仅 `7–10px`、最小状态文字为 `6px`，属于实现错误而非用户问题；
- Session 02 移除所有低于 `12px` 的有意义文字，将正文提升到至少 `14px`、主要任务提升到 `16px`，并提高浅色/深色文字对比度；
- Playwright 新增计算样式字号和正文对比度断言，1440px 与 390px 的完整上传/审核流程、无横向溢出继续通过；
- 更新桌面/手机截图和 `127.0.0.1:3012` 隔离预览；第 11 项仍为“待用户判断”，没有因为自动测试通过而标记完成。

### 2026-08-22 11:08 +08:00

- BP-01 建立独立 Idea、不可变 IdeaRevision、固定版本 IdeaLink、ProjectProposal/Revision 与 ProjectOrigin；旧项目表不被重解释或破坏；
- ProjectProposal 支持未知、暂不采用内容和精确多想法来源，提交时校验最低充分明确度；批准在一个事务中创建 Project、来源账本和根 BranchProposal 草案，批准立项不自动开始执行；
- 新增“想法 / 项目”两个一级导航、时间流/关系图可替换投影、想法修订/关联/立项页面和幂等 JSON API；
- 32 个 Rust 测试、迁移重放/旧 fixture、隔离 HTTP 立项流程以及 Chromium 1440/820/390 三尺寸通过；
- BP-01 仍执行中：文件/图片/语音采集、多来源 HTML 编辑与用户手感判断尚未关闭，不将第 1 项虚报为完成。

### 2026-08-22 11:24 +08:00

- 增加内容寻址 IdeaSource：文件、图片、语音使用服务端魔数嗅探可信类型，客户端扩展名/media type 不作为信任依据；附加来源产生新 IdeaRevision，后续修订继承精确来源链接；
- 网页可从文件/图片/语音直接建立想法，也可在详情追加来源；多来源 ProjectProposal 可按需选择其他想法并冻结各自版本；
- 隔离 HTTP 实测 JPEG/MP3/PDF、错误声明类型、SHA-256、幂等重放、内容读取和三来源版本继承；Chromium 实测图片起始、语音追加、多来源立项和三尺寸无横向溢出；
- BP-01 的机器验收已满足，状态改为“待用户判断”；时间流还是关系图作为默认、密度和整体手感必须由用户实际体验决定，未擅自标记完成。

### 2026-08-22 12:06 +08:00

- BP-02 增加不可变契约修订请求、人工决定和逐字段来源；接受新契约时安全暂停运行中的 Session，拒绝时活动契约不变；
- 对验收暂不明确的目标增加 `delivery | exploration | hybrid` 模式。探索/混合模式明确要求预算或判断边界、候选产出和不确定性收敛方式，同时继续把详细字段放在折叠区；
- 新增结构化不可变 Evidence、Contribution/Evidence 与 ReviewGate/Evidence 关联；拟合并冻结后禁止继续写，发现反例只能撤回并在同一目标枝干创建下一 Session；
- 补齐用户专属停止、终态归档和 `archivedFromStatus`，停止不会冒充完成；并发启动下一 Session 的两次请求实测只有一次获得写权；
- 35 个 Rust 测试、Clippy、迁移重放/旧 fixture、隔离 HTTP 和 Chromium 桌面/平板/手机流程通过。完整一键质量门及真实现场只读复核仍须在提交前执行；
- 第 3 项的既定语义与机器验收均已满足，标为完成；第 2、4、8、10 项只更新已验证基础，不把后续 Git、Runner、调度、独立复验与恢复工作虚报为完成。

### 2026-08-22 12:27 +08:00

- `scripts/quality-gate.sh` 从头一次通过：rustfmt、Clippy `-D warnings`、35 个 Rust 测试、7 个迁移、5 套隔离 HTTP、Chromium 三尺寸和生产 runtime；
- 提交前加固后再次从头通过同一完整质量门；负向 SQL 另验证无人工决定的契约终态、契约修订删除、冻结 Gate 篡改/跳级、非法探索策略和伪造归档均被数据库拒绝；
- runtime 以 UID 1000、只读根文件系统、随机回环端口和一次性 PostgreSQL 验证 7 个迁移及隔离写入；
- 真实数据库仍为 `3:8:23:2`，迁移登记仍只有原有 4 条；两个正式 Artifact 的数据库哈希与卷内文件逐项相同；
- 原应用 `bebb50d…` 与数据库 `482ccdee…` 的容器 ID/启动时间未变；只保留原生产和隔离审查两组容器，没有 Goal 测试容器残留；
- 旧 PostgreSQL 的 `0.0.0.0:55432` 风险仍原样保留给 BP-09，未在没有备份维护授权时擅自重建真实容器。

### 2026-08-22 13:07 +08:00

- BP-03 增加不可变 ContextEntry、ContextSnapshot、完整目录成员、派生索引代际、读取审计和来源边；Session 当前指针只能指向同 Project/GoalBranch/Session 的准确快照；
- 分支和续接前会先封存新的父现场，再由子/下一 Session 精确引用；契约接受、显式恢复和 EnvironmentManifest 绑定也生成新快照，避免权限或回流贡献停留在旧状态；
- 默认信封完整保留当前/祖先目标契约、硬约束、固定权限、创建状态和未决事项；目录默认只显示 12 项，但完整成员不被预算裁剪；
- 新增目录检索/分页，以及摘要、片段、全文读取和三类派生索引重建 API；读取审计只保存请求/来源/结果哈希与字符数，外部/用户资料明确是不可信数据；
- 固定窗口测试真实创建 20 项长上下文：子快照精确指向分支前父快照，第 20 项可检索读取，父硬约束没有丢失；并发同 ID 读取只有一个审计记录，重建形成第二代且不覆盖权威来源；
- 完整质量门通过 39 个 Rust 测试、8 个迁移、目标/上下文/想法/工具/输入/工作台 6 套隔离 HTTP、Chromium 桌面/平板/手机及非 root 只读生产镜像；
- 真实数据库只读复核仍为 `3:8:23:2`，迁移登记仍只有原有 4 条；正式 Artifact 的两个数据库哈希与物理文件逐项相同，原应用/数据库容器 ID 与启动时间未变，未残留一次性测试容器；
- 第 5 项的六条必要验收全部满足并标为完成；Git commit/code 的物理来源由 BP-04 按现有 Artifact/来源边协议接入，第 8 项仍不提前标为完成。

### 2026-08-22 15:10 +08:00

- BP-04 建立服务器托管 bare repository、确定性 GoalBranch branch/worktree、父安全点精确继承，以及磁盘实测 HEAD/tree/dirty/snapshot 与数据库记录的双向核对；
- 单个 worktree 仅允许一个不可变写 Lease，Job 固定 request/spec/runtime/base/权限/资源/允许写与显式删除清单，单调 fencing token、过期和迟到 Worker 均不能回写；
- 临时 Worker 实测断网、只读根与输入、独立输出、无 Linux capability、`no-new-privileges`、无 Docker Socket/宿主 home，并强制 CPU、内存、磁盘、PID、墙钟和日志上限；
- 输出新增/覆盖/删除经过可移植路径、大小写碰撞、符号链接、全量清单与哈希复核；候选在 detached apply worktree 构建，再以 Git compare-and-swap 发布，旁路脏写和并发 commit 被保留并转为可恢复冲突暂停；
- 成功 Job（含无提交 no-op）均生成不可变 snapshot 和绑定 base/head/tree/runtime/spec/output/delete 清单的 Contribution；候选失败不会留下 active Lease 或伪成功；
- 44 个 Rust 测试、9 个迁移、全部隔离 HTTP、真实 Git/Runner 攻击与资源矩阵、Chromium 三尺寸、非 root 只读生产镜像及完整一键质量门从头通过；
- 只读复核真实库仍为 `3:8:23:2` 和原 4 条迁移，两个数据库 Artifact 摘要与卷内对应文件逐项相同；生产容器完整 ID/启动时间不变，仅保留原生产与隔离审查两组容器，旧数据库公网端口风险未擅自改动；
- 第 6 项全部必要验收满足并标为完成。BP-04 不冒充联网插件、后台调度、跨枝干整合或恢复演练；这些继续进入 BP-05、BP-06、BP-07 和 BP-09。

### 2026-08-22 16:23 +08:00

- BP-05 把目录原型推进为签名安装链：Ed25519 签名准确覆盖规范 Manifest、OCI 镜像、不可替换入口、Runner 与自检摘要；发布者和安装证明只能追加或单向撤销；
- Rust 1.97、Python 3.13 双冲突版本、GCC/G++ 15 与 Playwright/Chromium 1.62 均通过统一 ToolCall/RunnerJob/ToolResult 协议，在一次性断网 Worker 中读取只读快照并经 Git CAS 回写；
- EnvironmentManifest 固定准确安装证明与镜像 ID，运行时再次验签并核对入口摘要；错误 schema、越权能力、伪造入口、错误 Runner、未签名/撤销插件和同版本双摘要都被拒绝；
- Session worktree 只留下源码、报告和截图，不含 Rust target、Python 虚拟环境/缓存、Node modules、浏览器或工具链；PPTMaster 未假装安装，只形成可审计的中央安装请求；
- 完整质量门从头通过：46 个 Rust 测试、10 个迁移、全部隔离 HTTP/Git/OCI/输入/工作台流程、Chromium 桌面/平板/手机，以及非 root、只读根文件系统的生产镜像；
- 只读复核真实数据库仍为 `3:8:23:2`、迁移登记仍为原有 4 条；两个 Artifact 的数据库摘要与物理文件逐项一致，应用/数据库容器完整 ID 和启动时间未变，测试容器均已清理；
- BP-05 逻辑闭环完成，但第 7 项保持“部分实现”：持续工具的物理租约、心跳、重启接管和取消竞态属于 BP-06，插件安装 UI 属于 BP-08。

### 2026-08-22 17:35 +08:00

- BP-06 新增数据库权威的 `goal_action_runs`、SchedulerWorker、ActionLease、不可变 ActionEvent、站内 Notification 和可替换 outbox；旧 `action_runs` 与旧 DAG 原样保留；
- Worker 注册默认关闭，配置 bootstrap secret 后只保存 Worker/Lease token 摘要；并发 claim 使用 `SKIP LOCKED`，同 Action/Worker 单 active Lease且每次认领单调增加 fencing；
- 隔离 HTTP 实测幂等 enqueue、并发只领取一次、Web 服务重建续接、安全任务失联重排、旧 fencing 拒绝、未知副作用暂停、通知去重/已读、人工恢复、排队截止时间、空队列退避和取消先到竞态；
- Playwright 插件增加显式 `serve` 持续工具。外部 launcher 按签名返回的准确镜像 ID，以只读根/worktree、无 capability、无 Docker Socket和资源上限真实启动 endpoint；Web 容器被杀并重建后同一 Lease 续接，用户释放后 endpoint 物理消失；
- 另一个隔离项目实测 launcher/工具容器突然消失：ActionRun 不盲目重启，ToolLease 转 expired，Session 异常暂停并生成唯一通知，`tool.cleanup` Worker 确认容器不存在后封存清理结果；
- 迁移新旧库/重放与 SQL 负约束、48 个 Rust 测试、Clippy、调度 HTTP 和真实签名 OCI ToolLease 专项均已通过；完整一键质量门及真实生产只读复核仍待本 BP 提交前执行；
- 第 2 项和第 9 项的全部必要语义与机器门已满足并标为完成；第 4/7/10 项只更新已验证基础，父枝干整合、最终插件/运行现场 UI 和独立审核继续由 BP-07/BP-08 接管。

### 2026-08-22 17:45 +08:00

- 完整 `scripts/quality-gate.sh` 从头一次通过：rustfmt、Clippy `-D warnings`、48 个 Rust 测试、11 个迁移、全部隔离 HTTP/Git/Runner/调度/OCI/输入/工作台流程、Chromium 桌面/平板/手机，以及非 root、只读根文件系统的生产镜像；
- 真实生产数据库只读复核仍为 `3:8:23:2`，迁移登记仍为原有 4 条；两个 Artifact 的数据库摘要与卷内文件实际 SHA-256 逐项相同；
- 生产应用 `bebb50d…`、数据库 `482ccdee…` 的完整容器 ID和启动时间未变，端口仍分别是 `127.0.0.1:3001` 与启动前已有风险 `0.0.0.0:55432`；
- 所有一次性测试容器、ToolLease 容器和网络均已清理，只保留原生产与隔离 review 两组现场；BP-06 逻辑闭环完成，进入 BP-07 独立审核与父枝干物理整合。

### 2026-08-22 19:17 +08:00

- BP-07 新增不可变 ReviewGate 物理绑定：`merge.propose` 只从干净、无 active Lease 且磁盘/数据库一致的 worktree 冻结契约、Contribution、Evidence、环境、HEAD/tree/snapshot 和候选摘要；客户端伪造的 Git 字段会被物理观察覆盖；
- 每个候选自动创建持久 Review ActionRun。已注册且与工作 Agent 身份不同的 Worker 通过 Lease/fencing 认领，只读挂载准确候选；报告必须逐项回传候选摘要、契约、Git 现场、环境、检查、反例与隔离证明，错误摘要、错误观察值、同身份和旧 token 均被拒绝；
- 用户完整或部分接受只创建 `pending` Integration 与独立 Integration ActionRun，不提前终结子枝干；选中 `code_change` 只按其成功 RunnerJob commit 顺序在 detached worktree 选择性应用，非代码 Contribution 只经来源边进入父上下文；
- 父契约复验成功后才用 `git update-ref <candidate> <expected-parent>` CAS 发布，并原子确认父 worktree/snapshot、上下文和领域状态；内容冲突、父 ref 抢先移动、父 worktree 漂移与验证失败均保留父安全点并生成 Attention/Notification；
- 故障注入覆盖 CAS 后数据库前崩溃及 Web 容器重建：重放可识别 ref 已是准确候选并幂等收尾；状态不唯一时只暂停，不猜测。父 Session 恢复但不会自动完成，根目标无虚假 Integration 且只能由用户在独立审核后确认完成；
- 第一次完整门在最后的发行镜像检查发现迁移计数断言仍写死为 11；服务本身已正常启动。断言更新为 12 后，发行镜像专项和完整 `scripts/quality-gate.sh` 均从头通过：48 个 Rust 测试（46 个主程序、2 个库测试）、12 个迁移、SQL 负约束、全部 HTTP/Git/Runner/调度/OCI/故障恢复、Chromium 三尺寸及非 root 只读 runtime；
- 最终只读复核真实库仍为 `3:8:23:2`、迁移登记仍为原 4 条；两个 Artifact 的数据库摘要与物理文件逐项一致，生产应用 `bebb50d…` 和数据库 `482ccdee…` 的完整 ID/启动时间未变；无一次性测试容器或网络残留，既有 `0.0.0.0:55432` 风险仍未擅自处理；
- 第 4、10 项的必要闭环满足并标为完成。审核/暂停的视觉密度继续由第 11 项让用户判断；备份、回退与部署恢复继续属于第 8、12 项，未被 BP-07 冒充完成。

### 2026-08-22 20:36 +08:00

- BP-08 将旧车道详情推进为 `goal-worksite-v2`：指挥摘要与筛选统一投影运行、Attention、ActionRun、ReviewGate、Integration 和 Notification；父子枝干按稳定前序排列，Session 以 URL 深链并支持方向键/Enter；
- 选中现场从权威记录展示 Git workspace、输入与 Runner 文件、ActionRun payload/result 和事件、ToolCall、ToolLease/endpoint、Evidence、Contribution、Notification、ReviewGate 及物理 Integration 阶段；没有事实时明确为空；
- 中央插件目录按需披露准确版本、能力/工具、安装或撤销状态、Runtime、权限和签名发布者；网页提交 PPTMaster 安装请求后刷新仍可见，但没有绕过签名安装的入口；
- 第一次 100 枝干/300 Session 大图审查发现 20 个 Proposal 遮挡主图、手机现场被长图推远；改为多 Proposal 默认折叠、移动图固定滚动窗口并自动定位当前 Session 后重测通过；
- 最近一次隔离基准包含 20 Proposal、20 ReviewGate、10 Integration、14 ActionRun、9 判断暂停和 7 未读通知：TTFB 三次为 `162.946/138.443/61.858ms`（中位 `138.443ms`），DOMContentLoaded `309.4ms`，筛选 `22.7ms`；
- 普通 HTTP 闭环、Chromium 1440/820/390、字号/对比度、44px 核心触控、键盘、减少动画、深链和无横向溢出已通过；Rustfmt、Clippy 和 48 个 Rust 测试也已通过。完整一键门和真实现场只读复核仍待本 BP 提交前执行；
- 第 11 项只能保持“待用户判断”：机器门不能替用户接受布局、密度、暂停/拟合并表达或想法空间默认投影。

### BP-08 提交前终检

- 完整 `scripts/quality-gate.sh` 在修正一条仍匹配旧标题的 Idea HTTP 断言后重新从头通过：Rustfmt、Clippy `-D warnings`、48 个 Rust 测试、12 个迁移、全部隔离 HTTP/Git/Runner/调度/签名 OCI/审核集成流程、Chromium 三尺寸和非 root 只读发行镜像；
- 100 枝干/300 Session 完整门实测 TTFB 中位数 `88.531ms`、`DOMContentLoaded` `349.5ms`、筛选 `47.3ms`，均低于 `1500/2500/100ms` 预算；
- 真实生产库只读复核仍为 `3:8:23:2`，`schema_migrations` 仍只有原 4 条；两个 Artifact 的数据库摘要 `f6bc647…` / `2038951…` 与卷内文件实际 SHA-256 逐项一致；
- 生产应用 `bebb50d…`、数据库 `482ccdee…` 的完整容器 ID、启动时间与端口不变；运行中只有原生产与隔离 review 两组容器，无一次性测试容器或网络残留；
- BP-08 机器验收闭环完成，但第 11 项仍保持“待用户判断”，不用测试数字替代人的真实体验。
