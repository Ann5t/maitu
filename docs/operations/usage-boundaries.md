# 脉图使用边界与待修问题

2026-10-10 记录。本页对应[完成路线](../product/completion-roadmap.md)阶段 6 的交付要求「可重复启动、升级、备份和恢复，记录使用边界与待修问题」。

这里只写**有证据**的结论，并逐项标明状态：

- **已合并**：在 `main` 上生效；
- **待合并**：已经有 PR 和验证，但尚未合并，**不算已具备的能力**；
- **需你决定**：涉及删除、覆盖、授权或日用取舍的事项。

## 一、已经验证的使用范围

| 能力 | 证据要点 | 记录位置 |
| --- | --- | --- |
| 资料执行基础 | 三请求重叠、下游使用确定版本、失败隔离、容器重建 | [实施进度](../development/maitu-progress.md) |
| 真实编码工作流 | 一个真实小功能从要求到可采用补丁；两个并行任务互不污染 | 同上 |
| 从想法到项目图 | 自然语言目标形成可执行图；用户改后能继续 | 同上 |
| 多连接与资源调度 | 连接之间真实调度、退避与重试有界、排队不丢任务 | 同上 |
| 日常使用与项目历史 | 同一项目两轮需求迭代；离开再进入能理解现状 | 同上 |
| 五库内容收拢 | 逐库内容清单、迁入去向、校验值、空目录恢复核对 | [旧仓库迁入](legacy-repository-migration.md) |
| 阶段 6 四个动词 | 见下节 | 本页 |

### 阶段 6 四个动词的本机证据（2026-10-10）

全部在**隔离对象**上完成，未触碰正在使用的实例；期间活实例一直 `health` 200、镜像摘要未变。

| 动词 | 做法 | 结果 |
| --- | --- | --- |
| 可重复启动 | `MAITU_INSTANCE=maitu-verify` + 独立端口，`docker compose up -d --no-build` | 全新库应用 18 项迁移，`health=200`（10 秒）；`down` 后再 `up` 仍 200（5 秒） |
| 升级 | `docker build --target runtime` 带 `FUDIAN_SOURCE_REVISION=<HEAD>`，另用独立标签 | 新镜像的 `org.opencontainers.image.revision` 等于源码修订；用它起的栈 `health=200`、18 项迁移。2026-10-10 复核迁移三方一致指的是 **`main`**：`migrations/` 磁盘 18 个、`src/migrations.rs` 注册 18 条（最后是 `0018_maitu_daily_history.sql`）、活库 `schema_migrations` 18 条，且没有「磁盘上有但未注册」的文件。**但第 19 个迁移是存在的，在待合并的 #20 里**：它新增 `migrations/0019_maitu_retry_hold.sql`（7 行）并在 `src/migrations.rs` 注册（+4 行），所以 18 是 `main` 的数字、合并 #20 后是 19。活实例在 `main` 口径下不落后，但**合并 #20 时必须应用 0019**；活库 `maitu_tasks` 现在 16 列（有 `connection_key`、无 `hold_connection`），也印证 #20 尚未部署。但运行中的活实例镜像 `7f6d55df3e16`（2026-10-06 构建，晚于 `main` 最新提交 2026-10-02）只有 compose 标签、**没有** `org.opencontainers.image.revision`，因此它的源码修订无法确认——补这个标签正是 **#12** 的作用 |
| 备份 | 用修好的 `backup-v2.sh`，备份目录写在**会触发该缺陷的 Windows 盘路径**上 | 目录可被恢复路径的只读挂载读取；`SHA256SUMS` 13/13 OK；元数据 `sourceMatchesRunningImage: true` |
| 恢复 | 把真实备份包恢复进带 `com.fudian.restore-target=true` 的**空**目标 | 退出 0；数据库 6 项计数与活实例一致；四个内容卷按文件清单逐个一致 |

复现命令见本页第四节。CI 在 Linux 上跑到同一份脚本（`scripts/test-backup-recovery.sh` 会依次调用 `backup-v2.sh` 与 `restore-v2.sh`），但**覆盖不到 Windows/WSL 宿主层**，所以本机证据单独记录。

## 二、使用边界

1. **平台**：本机日用是 Windows + WSL + Docker Desktop，CI 在 Linux 上跑。宿主文件系统（drvfs）与 Linux 的差异会产生 CI 发现不了的缺陷，涉及备份与恢复时请以**本机验证**为准。
2. **检出**：`main` 目前没有 `.gitattributes`，Windows 检出是 CRLF；Git 里存的仍是 LF。直接 `bash scripts/*.sh` 会把 CR 当成命令的一部分（`bash -n` 也会失败，`BASH_SOURCE` 定位可能落到错误的根目录）。修正在 **#7（待合并）**；在那之前请用 LF 检出（例如 `git -c core.autocrlf=false worktree add <目录> <提交>`）或经容器执行。2026-10-10 本机复核：`origin/main` 无 `.gitattributes`，`origin/repo/verified-merge-candidate`（#7）新增了它；当前 Windows 检出里 `scripts/quality-gate.sh` 是整文件 63 行 CRLF，`bash -n` 在第 10 行报 `syntax error near unexpected token '$'{\r'`，而同一个提交在 `git -c core.autocrlf=false worktree add` 得到的 LF 工作树里 0 行 CRLF、`bash -n` 无输出。注意 CRLF 脚本**不一定立刻报错**：`echo ok\r` 这类行会照常执行、只把 CR 留在输出里，直接失败的是结构行（`() {`、`[[ … ]]`）。运行期也会撞上同一问题：从 Windows 检出直接运行 `scripts/test-scheduler-http.sh`，会在 source `scripts/docker-test-lib.sh` 时报 `line 2: $'\r': command not found` 并以退出码 127 结束（2026-10-10 实测）；改用 LF 工作树后同一套件完整通过。
3. **镜像**：正在运行的活实例仍是旧镜像 `sha256:7f6d55df3e16`，**不含**下面第三节列出的修复。升级路径已在本机演练通过，但**尚未应用**到日用实例。
4. **凭据**：备份默认**不含**模型连接凭据（`credentialStoreIncluded: false`）。迁移或另行保管连接配置时，需要单独复制 `provider_config` 卷。真实 secret、`.env`、数据库、模型文件、构建缓存和运行日志都不进入 Git。
5. **入口**：`maitu` 保持**一个**实际运行的产品入口；五个旧库作为来源档案保留，不作为并列应用长期启动。

## 三、待修问题与待决定事项

| 事项 | 证据 | 状态 |
| --- | --- | --- |
| 限流后自动重试会落回同一个连接 | 隔离诊断（每次独立 PostgreSQL、只跑该用例）：改前的 `main` 连跑 12 次失败 2 次（flaky 连续服务第 1、2 次尝试、第 3 次才轮到 steady，断言 `left: 3`）；同一个提交 `b009cc5` 的两次检查也一成一败。修法一（存在别的连接就不许重领）让 `parallel_files_retry_pinned_dependencies_and_process_recovery` 3/3 超时失败；修法二（只在别的连接空闲时才不许重领）把自动阶段修好（11/12 记录为 `1 failed flaky` + `2 produced steady`），但 `integration.rs:963` 的"所有连接都在退避"断言 12/12 失败。只改用例也不行：先只注册 flaky、等 fixture 收到请求后再登记 steady 并删掉 flaky，20 次里 6 次仍失败，比改前的 2/12 更差。第四次（内层循环每轮复查冷却 `d9bad44` + 该用例改在任务入队后再起 worker `7b8e775`）做 40 对 40 对照：基线 `origin/main` 失败 4/40（全为 `left: 3`），改动后失败 8/40（`left: 3` 4 次、`left: 1` 4 次），反而更差，两个提交均未推送。第五次（把退避写成数据库持锁 `b832650`）：40 次诊断 39 通过，本缺陷相关的 `left: 3` 为 **0/40**（基线 4/40，Fisher 单侧 p≈0.015），完整工作流脚本从 LF 工作树连跑 3 次通过；剩下 1 次是另一种既有竞态 `left: 1`（steady 抢先领到第一次尝试） | 修复在 **#20（待合并）**（#19 已关闭，两次产品改法均被证伪）。机制：每个连接的退避原本只活在 worker 进程内，而领取路径是"先查冷却 → 若干次数据库往返 → 才真正 claim"，`fail_or_retry` 的"惩罚 + 插入重试"若落在中间，同一个连接就会在同一 tick 内领走重试。现在退避在"重新入队"的事务里同时写入 `hold_connection`/`hold_until`，`claim` 三处过滤并在最后一处按影响行数回滚，持锁因此是事务级的。**剩余**：`left: 1` 是用例自身的调度竞态（自动任务本就允许任一连接领取），已在 **#21（待合并）** 处理：初始只注册限流连接、观察到第一次请求后再登记另一连接，于是第一次尝试只可能落到 flaky，重试则被持锁挡开。该分支单独的 40 次诊断中 `left: 1` 为 0/40（仍保留 4/40 的 `left: 3`，那是本行上方的产品缺陷），与 #20 合并后 40 次诊断 **40/40 通过**、完整工作流脚本 3/3 通过。**该修复现在自带一个由质量门执行的确定性回归用例**（`src/maitu/integration.rs` 的 `queued_task_held_for_a_connection_is_not_claimed_by_it`，已登记进 `scripts/test-maitu-workflow.sh`，因此随完整质量门真正执行）：它绕开失败路径，直接给一条 `queued` 任务写上活跃持锁，于是进程内冷却并未激活、持锁成为唯一解释，另有一条同形无持锁的对照任务；判定按"失败尝试数"计数而不是 attempt 行数，因为 `fail_or_retry` 会在**新的 attempt 行**里为重试排队（`workflows.rs:1073`）。实测正向为 `status=queued failed=0 hold_left=4.83s attempts=[(1,"queued")]`，把三处持锁谓词改成恒假之后同一用例失败、观测为 `status=queued failed=1 hold_left=0.90s attempts=[(1,"failed"),(2,"queued")]`；`c723c9e` 的两遍完整质量门均为 success |
| 备份目录在 Windows 盘上可能变得无法恢复 | 宿主目录里一旦出现由容器创建的文件，再经 `mv` 改名，该目录在 WSL 侧显示 `d?????????`、只读挂载报 `mkdir …: file exists`；旧写法可复现，改为宿主落盘后不可复现，真实数据上也验证过。2026-10-10 本机再做一次**前后对照**（同一活实例、同一类 Windows 盘路径、同一批脚本）：`origin/main` 备份后该目录在 WSL 侧 `ls` 无输出、`sha256sum` 找不到 `SHA256SUMS`、连 `rm -rf` 都报 `Is a directory`，恢复因此直接失败（退出码 1）；换成 #17 的分支 `6a9f73e` 则目录可读（14 项、4.6 MB）、`sha256sum --check --strict SHA256SUMS` 全部通过、`restore-v2.sh` 退出码 0 并报「恢复完成：目标数据库、四类内容卷及 Git 对象均已校验」。恢复库与活库一致：项目 7、任务 16、迁移 18，artifacts 20 个文件、repositories 4 个裸库（活库的 18 个迁移也印证第 40 行的「活实例仍是旧镜像」）。演练产物（专用标签卷、专用库容器、LF 工作树、备份目录）均已删除，活实例未改动 | 修复在 **#17（待合并）**，运行期自检在 **#12（待合并）** |
| Windows 上按 CRLF 检出仓库 | 仓库脚本 `bash -n` 失败、`BASH_SOURCE` 相对根定位错误 | 修复在 **#7（待合并）** |
| 两份原地坏备份 `20261009T013722Z`、`20261009T121159Z` | Windows 侧可正常读取、WSL 与容器侧不可读；**内容完好**，复制成新目录即可用于恢复（已实测校验与解包） | **需你决定**：是否复制留存或重做备份；原件未删 |
| 可回收的 Docker 数据（2026-10-10 复测：卷 17.89 GB、构建缓存 16.66 GB、镜像 4.674 GB） | 三个当前 compose 未声明、也无容器使用的命名卷（2026-10-10 再复核体积为 `maitu_check_target` 5.456 GB、`maitu_check_registry` 242.3 MB、`maitu_check_git` 0 B），并逐容器核对挂载确认它们**不被任何容器引用**（含已停止的 `maitu-storage-init-1`、`maitu-code-runtime-1`），四份 compose 也都不声明；三者创建时间同为 2026-09-30T10:54:47Z 且没有 compose 项目/服务标签。`maitu_check_registry` 从命名与体积看是检查容器的 cargo 缓存，属**可再生成**数据（删除后下次检查需重新下载）；`maitu_check_target` 从先前记录的 5.1 GB 增至 5.456 GB，增量未归因。加悬空匿名卷：记录时 83 个，同日复核依次为 93 → 106 → 113 → **118** 个（各次增量都来自本维护任务隔离诊断新建的临时卷；维护只删过自己新建且已悬空的匿名卷，记录里的 83 个未动）；**2026-10-10 用 `docker system df -v` 复测**（三个卷体积不变：5.456 GB / 242.3 MB / 0 B，LINKS 均为 0，`docker ps -a --filter volume=<卷>` 也确认无引用）：Local Volumes 总量 17.97 GB 中 **17.89 GB（99%）标记为无引用**，含这 118 个悬空匿名卷，它们的体积远大于那三个命名卷的 5.7 GB；Build Cache 20.71 GB 中 **16.66 GB** 可回收（227 个条目，属可再生成）；Images 11.16 GB 中 4.674 GB 可回收。⇒ 原标题的「约 9.7 GB」低估了总量，已按复测更正。 | **需你决定**：清理需要授权 |
| 7 个 `action_runs` 停在 `ready`（2026-10-10 复核为**不是缺陷**） | 2026-09-30 至 2026-10-06 这 7 行全部是 `kind='confirm_outcome'`、`owner='human'`、`requires_approval=1` 的**人工确认闸门**，7 个项目各 1 条（无重复派发），本来就在等用户处理。**2026-10-10 在活实例（127.0.0.1:3033，只读 GET）逐标签核验**：确认入口确实存在且可用，但只出现在 `contract`、`graph`、`history`、`outputs` 四个标签（各渲染 1 次 `confirm_outcome` 与按钮「确认完成标准」，POST 目标 `/projects/{id}/actions` 对 GET 返回 405，符合预期）；**默认标签 `goals` 不显示它**（该页有「当前」面板，列的却是另一条动作）⇒ 这 7 条待办可操作，但停在默认视图的用户看不到，需要自己切到图形等标签。真正执行的一套用**另一张表** `goal_action_runs`（带 `available_at`/`deadline_at`/`client_request_id`，状态 `queued`/`running`/`succeeded`/`cancelled` 等，另有 `reconcile_action_runs` 与截止时间清扫）：只读复核为 1 行 `queued`（`kind='review'`、`capability='review.goal_candidate.v1'`、`subject_kind='review_gate'`，2026-10-05T19:40:23Z 创建，`available_at` 就是创建时刻，截止 2026-10-12T19:40:23Z，`attempt_count=0`，`started_at` 为空）。**2026-10-10 复核更正**：执行侧的调度是一套**面向外部 worker 的 API**——`goal_action_runs` 的读写集中在 `src/application/scheduler.rs`，对外只有 `/api/v1/scheduler/claim`、`…/reconcile`、`…/heartbeat` 等端点，客户端按 `docs/reference/api-goal-branch-v1.md` 是**独立的 Review Worker**（身份取自调度器注册信息，且不得与工作 Agent 相同）；应用启动时唯一的后台任务是 `maitu::workflows::serve_worker`，只处理 `maitu_tasks`，`maitu-check-worker` 也不调用这些端点，全仓库没有任何地方周期性调用 `reconcile_action_runs`。活库 `scheduler_workers` 为 **0 行**、`action_run_leases` 为 **0 行** ⇒ 这条 review 五天内从未被领取。后果**有时效性**：`claim_action_run` 的选择条件含 `deadline_at > now()`，**过期即无法再被领取**，而清扫（`scheduler.rs:1831`，把过期 queued 置为 `action_deadline_exceeded` 失败）没有周期调用者 ⇒ 2026-10-12T19:40:23Z 之后这行会变成**永久死行**，对应 goal 的评审闸门停在 `pending_ai_review` 无法完成。⇒「没有卡住的执行」只对**人工确认的那 7 行**成立，对执行侧不成立 | 记录更正：等待用户确认是正常状态；是否逐项确认或放弃仍由你决定。**需你决定**：执行侧要不要有驱动——(a) 在应用内加周期性 reconcile，(b) 实现并部署《api-goal-branch-v1》描述的独立 Review Worker（当前仓库只有测试脚本模拟它），或 (c) 明确接受 pending 状态并写进文档。活实例当前没有 worker，所以除 (a)/(b) 外无法自愈。**2026-10-10 已在隔离栈实测这三条（纯 API 造数据，不直接改库）**：入队一条 `deadlineAt = now+4s` 的 queued（`action=392bece4-…`）后为 `queued`；等 6 秒且**未调用 reconcile** 时它仍是 `queued` ⇒ 运行中的应用**不会**自动清扫；此时 `POST /api/v1/scheduler/claim` 返回 `action: null` ⇒ 过期即领不到；接着只 `POST /api/v1/scheduler/reconcile` 一次，该行立刻变成 `failed | action_deadline_exceeded` ⇒ 只有显式 reconcile 才能判失败。隔离栈自带的调度器 HTTP 套件在同一轮通过（退出码 0） |
| 执行侧的 Review Worker 没有实现，路线图也未列 | [领域规范](../architecture/goal-branch-domain.md) 第 186 行规定 `pending_ai_review → pending_human_review` 必须由「Review Worker complete」驱动，第 217 行另列不变式 `independent_reviewer_required`（审核者与工作 Agent 必须身份不同）；协议见 [api-goal-branch-v1](../reference/api-goal-branch-v1.md) 第 54 行。但仓库里没有这个生产者：`src/bin/` 只有 `fudian-runner`、`fudian-tool-runtime`、`maitu-check-worker`、`maitu-code-check`、`fudian-maintenance`（最后这个只做四类存储根的 `scan`/`quarantine`/`restore`，内部不含任何调度器调用，也只被 `scripts/test-storage-reconciliation.sh` 调用）；除测试脚本外全仓库没有任何进程调用 `/api/v1/scheduler/claim`、`…/reconcile`、`…/heartbeat`；应用启动时唯一的后台任务是只处理 `maitu_tasks` 的 `workflows::serve_worker`。[整体完成路线](../product/completion-roadmap.md) 全文没有出现 Review Worker ⇒ 它是**规范要求但未实现**的组件，不是偶发缺陷。归档的[第一阶段报告](../archive/runs/first-stage/goal-12-final-report.md)把「独立只读 Review Worker」记为已完成，指的是协议与测试闭环，而不是可部署的 worker | **需你决定**：与第 53 行是同一件事——(a) 应用内加周期性 reconcile，(b) 实现并部署 Review Worker，(c) 接受 pending 状态并写进文档；**2026-10-10 进一步核实：这对用户是硬停。** `src/goal_domain.rs:340` 要求闸门已经是 `PendingHumanReview` 才允许用户决定，`:325` 是唯一能把闸门升格到该状态的转换（即 Review Worker 完成），`:367` 只允许从 `pending_ai_review` 直接 `Withdrawn`；界面同样如此：`src/web/views.rs:2465-2482` 在该状态下只显示「独立审核已排队／系统正在独立复验；工作 AI 不能代录结论」，没有任何接受/退回/部分接受按钮（那些只在 `pending_human_review` 分支 2483-2519 出现），唯一可点的出口是「AI 发现候选不再可信」的撤回。2026-10-10 又对**正在运行的活实例**（127.0.0.1:3033，只读 GET）确认了一遍：该项目页里「独立审核已排队」与「系统正在独立复验」各出现 1 次，而 `review.human_decide`、「接受整条枝干产出」、「退回到下一 Session」均为 **0 次**，枝干状态显示为「拟合并审核」；工作台首页并不提示这条待办，需要打开该项目才能看到。⇒ 部署中的旧镜像给用户的正是同一个死胡同。⇒ 用户**无法接受**自己的成果，只能撤回（Session 变 `review_rejected`、枝干回 `active`）；而界面写着「已排队／正在复验」，实际没有任何 worker 会执行它。**选项修正**：(a) 应用内周期性 reconcile **解决不了这个停摆**——它只会把过期 queued 判为失败，闸门仍停在 `pending_ai_review`；真正解封只有 (b) 实现并部署 Review Worker，或 (c) 接受 pending 状态并写进文档，或另立 (d) 放宽 `independent_reviewer_required` 不变式（与现有审核完整性设计冲突，需单独决定） |
| `maitu_tasks` 与动作记录之间没有关联列 | `maitu_tasks` 有 `latest_attempt_id`、`accepted_attempt_id`、`task_kind`，但没有指向动作记录的列。`action_runs` 通过 `node_id`、`artifacts.action_run_id`、`evidence.action_run_id` 关联；执行侧的调度记录另存 `goal_action_runs`（有 `subject_kind` 与 `client_request_id`，但没有 `maitu_tasks` 主键）。⇒ "哪个任务产出这条动作、哪条动作跑了这个任务"在 schema 上无法表达 | 已记录；#11（待合并）更新路线图状态 |
| 尚未合并的修复与推荐顺序 | 2026-10-10 用 `git merge-tree --write-tree` 复核全部 **12** 个开放 PR：12 条分支相对 `main` 都是落后 0 提交、都能干净合入；两两组合 66 种里只有三对冲突，都集中在 `#14`、`#8`、`#7` 这一组（`#14`×`#8` 与 `#14`×`#7` 踩 `scripts/quality-gate.sh`，`#8`×`#7` 踩 `scripts/README.md`），其余 63 种均可干净合并；新增的 `#22` 与其他任何分支都不冲突。核对方式：先取 GitHub API 的 `head.sha`，再与本地 `origin/<分支>` 逐条比对，12 条全部一致后才采信矩阵（一次 `git fetch` 曾被网络重置，不能只看本地引用就下结论）。`#21` 依赖 `#20`（用例的确定性靠持锁成立），两者 CI 均已全绿。**2026-10-10 又做了累积链验证**（`git merge-tree --write-tree` + `commit-tree`，只写对象、不动任何 ref）：把推荐顺序真正串起来跑，`#20 → #21 → #7 → #12 → #17 → #9 → #11 → #15 → #18` 九步都干净（文件数 265→445），**第 10 步 `#8` 冲突**；反向（先 `#8` 再 `#7`）同样冲突。再把三种两两顺序都实测：`#8` 在 `#7` 之后、`#7` 在 `#8` 之后都踩 `scripts/README.md`；`#14` 在 `#7`/`#8` 之后、`#7` 与 `#8` 在 `#14` 之后都踩 `scripts/quality-gate.sh` ⇒ {`#7`,`#8`,`#14`} 是一个**任何顺序都解不开的冲突团**，只能合并时手工解决这两个文件（团成员规模：`#7` 197 文件 +2692−323、`#8` 3 文件 +138、`#14` 1 文件 +21−1，冲突面很小）。不含该团的 9 个 PR 可**整链干净合并**：`#20 → #21 → #12 → #17 → #9 → #11 → #15 → #18 → #22`（本轮实测，文件 265→268）。另核实 `#17` 的 merge-base 就是 `#12` 的头 `b009cc5`，所以 `#12` 必须先于 `#17`。**2026-10-10 再量了冲突团的手工解决规模**（`git merge-file` 逐对生成冲突原文 + `git diff --stat`）：`#7` 对这两个文件只有 `scripts/README.md` +2、`scripts/quality-gate.sh` +13−2，`#8` 是 +1、+2，`#14` 只有 `scripts/quality-gate.sh` +21−1。冲突一共 **4 块，且每块都是双方各自新增、没有语义分歧**：`README.md` 的 1 块是 `#7` 的 `check-auth-page-contract.py` 条目对 `#8` 的 `check-secrets.py` 条目（两条都留）；`quality-gate.sh` 里 `#7`×`#14` 的 2 块是 `#7` 把 `docker pull` 包进 `fudian_retry_network`、把依赖下载单独重试，对 `#14` 新增的 `step "拉取基础镜像"`／`step "Rust 格式、lint 与单元测试"` 标签（都留）；`#8`×`#14` 的 1 块是 `#8` 新增 `./scripts/check-secrets.py` 对 `#14` 的 `step "Git 空白检查与收尾"`（都留）；`quality-gate.sh` 的 `#7`×`#8` 实测 **0 块**。⇒ 冲突团是「两边都保留」的机械并集，手工解决量约 6 至 8 行，没有需要取舍的设计分歧。**同日又验证了含 `#7` 的完整链**：`#20 → #21 → #7 → #12 → #17 → #9 → #11 → #15 → #18 → #22` 十步全部干净（文件数 265→445），所以「先合 `#7`（它带来 `.gitattributes`，可尽早解掉 CRLF 问题）再合另外 9 个」这条路也实测可走。`#8`/`#14` 的并集解析也已在纯 plumbing 下跑通：我第一版用 `<(...)` 进程替换喂 `git merge-file`，它面对不可 seek 的管道会静默失败并写出空 blob（`e69de29`），我据此误报过一次「结论作废」；改用真实临时文件后，在含 `#7` 与 `#22` 的链末依次合 `#8`（并集 `scripts/README.md`：53 行，`check-secrets.py` 与 `check-auth-page-contract.py` 两侧条目都在）与 `#14`（并集 `scripts/quality-gate.sh`：3 块冲突，正好等于 `#7`×`#14` 的 2 块加 `#8`×`#14` 的 1 块），最终树 446 个文件、两侧内容齐全，且并集后的脚本 `bash -n` 通过。⇒ 整条合并计划已端到端验证：10 步干净合并 + 2 次并集解析；并集只保证语法与两侧内容共存，控制流语义仍建议合并时人工过一眼。**2026-10-10 用当时的确切 PR 头复跑**（`#7` `40af1b9`、`#8` `e6dde36`、`#9` `c6ad31a`、`#11` `187654f`、`#12` `b009cc5`、`#14` `4d385de`、`#15` `273c845`、`#17` `6a9f73e`、`#18` `2f43b1c`、`#20` `c723c9e`、`#21` `515c281`、`#22` `bf818c8`）：十步依然全干净（265→445 个文件），两次并集的 blob 与上一轮**逐字节相同**（`scripts/README.md` `ba312d659631`、`scripts/quality-gate.sh` `5c4f76354f0c`，说明这套解析是可复现的），最终树 446 个文件、并集后的脚本 `bash -n` 通过；只被 `#11` 改动的 `docs/product/completion-roadmap.md` 和只被 `#18` 改动的 `docs/operations/usage-boundaries.md` 都不被其他 PR 触碰，所以本维护任务自己对两个 PR 追加提交不会改变这套结论。 | **需你决定**：先合**可整链干净的 9 个**：`#20 → #21 → #12 → #17 → #9 → #11 → #15 → #18 → #22`（本轮累积链实测干净；`#20`/`#21` 有依赖关系，`#12` 必须先于 `#17`，`#22` 位置可任意）。然后再处理冲突团 `#7`/`#8`/`#14`：**顺序与 rebase 都消不掉冲突**（三种两两顺序实测全冲突），只能在合并时手工解决 `scripts/README.md`（`#7`×`#8`）与 `scripts/quality-gate.sh`（三者两两）。原先的顺序把 `#7` 放中间并假定「相冲的 `#8`、`#14` 只需一次 rebase」，与本轮实测不符，已更正 |
| 五个旧库的删除 | 内容迁移与独立恢复验收已完成，删除条件是独立状态 | **需你决定**：未达删除条件，原库继续保留 |
| 远端保留了较多历史分支（含已合并的） | 只做过统计，未改动 | **需你决定**：清理需要授权 |
| CI 会因匿名拉取 caddy 镜像撞 Docker Hub 限流而整门失败 | `c227786`（只改文档）的 run #169/#170：Rust 测试、生产镜像与存储核对**都已通过**，随后 `Unable to find image 'caddy:2.11.4-alpine@sha256:5f5c…' locally`、`docker: Error response from daemon: toomanyrequests: You have reached your unauthenticated pull rate limit`，以退出码 125 失败 | **需你决定**：限流是六小时配额，"加重试"在证据上解决不了，需要配置 Docker Hub 凭据（`docker/login-action` + 仓库 secret）或调整该步骤；此后多次运行（#175、#176、#181 至 #189，其中 #183、#184 因被新推送取代而取消）未再出现该失败，与六小时配额的间歇特征一致，所以它是**间歇性**风险，不是每次都红 |
| 运行中实例的日志审计（2026-10-10） | App 容器自 2026-10-06 起总共只记 9 行：2 行启动、**7 行 HTTP 500**（`tower_http::trace::on_failure`，时间 2026-10-09 的 01:18:12、01:20:02、01:20:39、01:24:11 两次、03:22:06、03:24:40 UTC，全部 `latency=0 ms`，即秒级失败）。Postgres 全部日志 165 行里有 45 条 ERROR/FATAL，多数可解释：13 条 `the database system is starting up`、6 条 `role "fudian" does not exist`、2 条 `role "postgres" does not exist` 是启动期重试；`column "version" does not exist`、`column "created_at" does not exist`、`column p.name does not exist` 是**我自己的探针**；`column "hold_connection" does not exist`（含 `PREPARE s1(text) … t.hold_connection …`）是 **#20 的查询原文**——与 `src/maitu/workflows.rs` 里 `SELECT * FROM maitu_tasks t WHERE t.status='queued' AND …` 逐字对应——跑在**没有 0019 的库**上，也是我验证 #20 时的探针，不是缺陷；`relation "job_queue"/"scheduler_jobs"/"action_leases" does not exist` 来自 `SELECT status, count(*) FROM …` 这类临时统计查询，而 `origin/main` 对这几个名字（含 `action_key`）的引用都是 **0 处**，所以主分支代码不负责它们。三个容器重启次数都是 0、状态 healthy，`maitu-check-worker-1` 无错误日志 | **需你决定/待查**：那 7 个 500 与上述数据库错误时间对不上（500 在 01:18–03:24，DB 错误在 18:52、23:02、23:19），来源未明；日志行里没有 `uri=`，无法判断是哪个路由，因此我没有把它写成已定位缺陷。复现需要当时的操作步骤，或临时提高容器日志级别 |

补充两条诊断时用得到的事实，免得重复踩坑：fixture 的调用计数在返回 429 **之前**自增（`calls` 的 entry 写在 flaky 分支之前），所以"已收到请求"不等于"响应已发出"；`ProviderStore::delete` 拒绝删除最后一个连接（`provider.rs:383`，提示"至少保留一个模型连接"），而 `save` 与 `delete` 都会立即更新内存里的连接表（`provider.rs:370`、`389`），所以删掉的连接不会再被领取。

### 若选 (b)：Review Worker 的接口已经写死

`scripts/review-worker-test-lib.sh` 里的 `test_review_gate` 已经把协议跑通，实现方照做即可：

1. 用 `POST /api/v1/scheduler/workers` 注册**独立身份**（请求头 `x-fudian-worker-bootstrap`，能力 `["review.goal_candidate.v1"]`，身份必须与工作 Agent 不同）；
2. 轮询 `POST /api/v1/scheduler/claim`，领到 `kind="review"`、`subjectId` 等于闸门 id 的 ActionRun，并持有 `softTtlSeconds`/`hardTtlSeconds` 内的 Lease 与 fencing token（测试里是 120/300 秒，真实复验应配合心跳或用更长的租约）；
3. 以**只读**方式打开冻结候选（payload 里的 `repositoryKey`/`worktreeKey`，摘要见 `candidateDigest`/`headCommit`/`treeId`/`workspaceSnapshot`），复跑契约检查与隔离复验、记录反例与复验证据；
4. 用 `POST /api/v1/scheduler/action-runs/:id/complete` 提交绑定报告，报文字段为 `schemaVersion`、`candidateDigest`、`contractVersionId`、`observedHeadCommit`、`observedTreeId`、`observedWorkspaceSnapshot`、`environmentFingerprint`、`decision`、`rationale`、`contractCheck`、`counterexamples`、`retestEvidence`、`isolation{candidateReadOnly,noWorkspaceWrites,noNewPrivileges,dockerSocketAbsent,hostSecretsAbsent,effectiveCapabilitiesHex}`；按[领域规范](../architecture/goal-branch-domain.md)第 186 行，摘要、HEAD/tree/snapshot、环境或 fencing 不匹配都会被拒（各项隔离声明的服务端校验强度我未逐项验证）；
5. 完成后闸门才转 `pending_human_review`，用户才会看到「接受／退回／部分接受」。

**缺的是生产者与部署**：一个独立进程（例如 `src/bin/` 新二进制或外部服务）、一条与工作 Agent 不同的模型连接、只读挂载且无 Docker socket 与主机密钥的隔离，以及 compose 服务与 bootstrap 密钥。验收套件已经存在（`scripts/test-review-integration-http.sh` 与上面这个共享库），所以实现后能直接在质量门里验证。

## 四、怎样复现上面的验证

```bash
  # 1) 可重复启动：另起实例，项目名、端口与数据卷都与日用实例隔离
MAITU_INSTANCE=maitu-verify MAITU_PORT=3133 \
  docker compose -f compose.maitu.yaml up -d --no-build
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3133/api/health

  # 2) 备份：备份目录故意写在 Windows 盘路径上（缺陷只在这一层出现）
FUDIAN_BACKUP_DATABASE_CONTAINER=<stack>-postgres-1 \
FUDIAN_BACKUP_POSTGRES_USER=maitu FUDIAN_BACKUP_POSTGRES_DB=maitu \
FUDIAN_BACKUP_APP_CONTAINER=<stack>-app-1 \
FUDIAN_BACKUP_ARTIFACT_VOLUME=<stack>_artifacts \
FUDIAN_BACKUP_REPOSITORY_VOLUME=<stack>_repositories \
FUDIAN_BACKUP_WORKTREE_VOLUME=<stack>_worktrees \
FUDIAN_BACKUP_RUNNER_VOLUME=<stack>_runner_outputs \
  bash scripts/backup-v2.sh /mnt/c/<会触发缺陷的路径>/backups

  # 3) 恢复：目标必须是带专用标签的空对象
docker volume create --label com.fudian.restore-target=true <目标卷>
docker run -d --name <目标库容器> --label com.fudian.restore-target=true \
  -e POSTGRES_USER=maitu -e POSTGRES_DB=maitu -e POSTGRES_PASSWORD=<本机开发口令> \
  postgres:17-alpine
FUDIAN_RESTORE_CONFIRM=EMPTY_LABELED_TARGETS \
FUDIAN_RESTORE_DATABASE_CONTAINER=<目标库容器> \
FUDIAN_RESTORE_POSTGRES_USER=maitu FUDIAN_RESTORE_POSTGRES_DB=maitu \
FUDIAN_RESTORE_ARTIFACT_VOLUME=<目标卷> … \
FUDIAN_RESTORE_APP_IMAGE=<runtime 镜像> \
  bash scripts/restore-v2.sh <已校验的备份目录>
```

脚本自带两道保护：备份结束前先用**恢复路径的只读挂载**自检产出；恢复只允许写入带 `com.fudian.restore-target=true` 的**空**目标，并在写入前执行 `sha256sum --check --strict SHA256SUMS`。

## 五、维护约定

- 本页只记录有证据的结论。某个修复合并后，请把对应状态从「待合并」改为「已合并」，并把行为变化写进[实施进度](../development/maitu-progress.md)；不要在本页把计划写成已实现。
- 阶段 6 的通过条件是「上述使用流程通过；影响核心使用或造成数据损失的问题已处理；用户试用接受」，**不是**本页列完即通过。
- 相关检查方法见[测试指南](../development/testing.md)，本机启动细节见[脉图本机使用](maitu-local.md)，备份单元与回退边界见[备份与恢复](recovery.md)。
- `scripts/check-docs.py` 按行统计以 `# ` 开头的行，所以代码块里**顶格**写的 shell 注释会被当成一级标题；注释请缩进两格，或移到代码块外。
- GitHub 对同一分支只保留最新一次推送的运行：每次 `git push` 都会把同分支更早的 `in_progress` 运行置为 `cancelled`。所以连续多轮推送时，早先提交的结论永远拿不到（本维护任务在 #191 至 #202 之间就反复只看到 `cancelled`）。需要某个提交的完整结论时，先等当前运行结束（完整质量门用时要按取样说明（2026-10-10 用 API 的 `run_started_at`→`updated_at` 重算）：最近 10 个提交的 20 次成功运行是 **15.0 至 20.7 分钟**（`9dce089` 15.5/16.6、`6183173` 15.8/16.8、`7f62785` 18.5/19.1、`442c7cf` 18.8/19.2、`54dce1f` 18.2/19.5、`e7d9b66` 15.0/19.7），而全部 110 次成功运行的区间是 **14.6 至 29.6 分钟**——所以单次跑到 25 分钟也属正常，别按更窄的区间误判为卡住）再推下一个提交；2026-10-10 用这个办法拿到了 #203/#204（`9dce089`）的 success。
- 核验活实例页面时用 WSL 里的 `curl`，不要用 PowerShell 的 `Invoke-WebRequest`：本轮 `GET /projects/{id}?tab=graph` 在后者下报 **400**，同一 URL 在 `curl` 下是 **200** 并带完整页面。拿 PowerShell 的状态码当结论会误报缺陷。
- 从这台 Windows 机器 `git push` 会间歇性失败（`Could not connect to github.com:443`、`Recv failure: Connection was reset`，2026-10-10 连续 16 次失败），但同一时刻 HTTPS 与 API 都正常：`curl` 到 `…/info/refs?service=git-receive-pack` 是 401、`…git-upload-pack` 与仓库页都是 200；TCP 443 直连三次里两次 0.2 秒、一次 25 秒超时；而 **WSL 里的 git 通信正常**（`git ls-remote origin` 退出码 0）。所以卡住时改走 WSL：把 `https://x-access-token:<token>@github.com` 写进临时凭据文件，执行 `git -c credential.helper="store --file=<该文件>" push origin <分支>`，用完立刻删除该文件（2026-10-10 这样第 1 次即成功：`54dce1f..331ecb9`）。
- 用 PowerShell 数行数不要用 `Measure-Object -Line`：对同一份 116 行的文档它给出 **92**（它按每个输入对象分别计行），行数定论请用 `wc -l`（本页 `wc -l` 是 116，末行没有换行符，所以编辑器显示 117 行）。
- 写文件时若遇到 `ReplaceFileW EIO (Win32 1175)`（另一个进程正占用该文件的瞬时错误），先**读回该文件确认内容没有被破坏**，再原样重试，不要因此重写整份文件。2026-10-10 追加本页一行时就遇到一次：读回确认无损，重试一次即成功。