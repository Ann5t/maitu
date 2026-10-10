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
| 升级 | `docker build --target runtime` 带 `FUDIAN_SOURCE_REVISION=<HEAD>`，另用独立标签 | 新镜像的 `org.opencontainers.image.revision` 等于源码修订；用它起的栈 `health=200`、18 项迁移 |
| 备份 | 用修好的 `backup-v2.sh`，备份目录写在**会触发该缺陷的 Windows 盘路径**上 | 目录可被恢复路径的只读挂载读取；`SHA256SUMS` 13/13 OK；元数据 `sourceMatchesRunningImage: true` |
| 恢复 | 把真实备份包恢复进带 `com.fudian.restore-target=true` 的**空**目标 | 退出 0；数据库 6 项计数与活实例一致；四个内容卷按文件清单逐个一致 |

复现命令见本页第四节。CI 在 Linux 上跑到同一份脚本（`scripts/test-backup-recovery.sh` 会依次调用 `backup-v2.sh` 与 `restore-v2.sh`），但**覆盖不到 Windows/WSL 宿主层**，所以本机证据单独记录。

## 二、使用边界

1. **平台**：本机日用是 Windows + WSL + Docker Desktop，CI 在 Linux 上跑。宿主文件系统（drvfs）与 Linux 的差异会产生 CI 发现不了的缺陷，涉及备份与恢复时请以**本机验证**为准。
2. **检出**：`main` 目前没有 `.gitattributes`，Windows 检出是 CRLF；Git 里存的仍是 LF。直接 `bash scripts/*.sh` 会把 CR 当成命令的一部分（`bash -n` 也会失败，`BASH_SOURCE` 定位可能落到错误的根目录）。修正在 **#7（待合并）**；在那之前请用 LF 检出（例如 `git -c core.autocrlf=false worktree add <目录> <提交>`）或经容器执行。2026-10-10 本机复核：`origin/main` 无 `.gitattributes`，`origin/repo/verified-merge-candidate`（#7）新增了它；当前 Windows 检出里 `scripts/quality-gate.sh` 是整文件 63 行 CRLF，`bash -n` 在第 10 行报 `syntax error near unexpected token '$'{\r'`，而同一个提交在 `git -c core.autocrlf=false worktree add` 得到的 LF 工作树里 0 行 CRLF、`bash -n` 无输出。注意 CRLF 脚本**不一定立刻报错**：`echo ok\r` 这类行会照常执行、只把 CR 留在输出里，直接失败的是结构行（`() {`、`[[ … ]]`）。
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
| 陈旧 Docker 数据约 9.7 GB | 三个当前 compose 未声明、也无容器使用的命名卷（2026-10-10 再复核体积为 `maitu_check_target` 5.456 GB、`maitu_check_registry` 242.3 MB、`maitu_check_git` 0 B），并逐容器核对挂载确认它们**不被任何容器引用**（含已停止的 `maitu-storage-init-1`、`maitu-code-runtime-1`），四份 compose 也都不声明；三者创建时间同为 2026-09-30T10:54:47Z 且没有 compose 项目/服务标签。`maitu_check_registry` 从命名与体积看是检查容器的 cargo 缓存，属**可再生成**数据（删除后下次检查需重新下载）；`maitu_check_target` 从先前记录的 5.1 GB 增至 5.456 GB，增量未归因。加悬空匿名卷：记录时 83 个，同日复核依次为 93 → 106 → 113 → **118** 个（各次增量都来自本维护任务隔离诊断新建的临时卷；维护只删过自己新建且已悬空的匿名卷，记录里的 83 个未动） | **需你决定**：清理需要授权 |
| 7 个 `action_runs` 停在 `ready`（2026-10-10 复核为**不是缺陷**） | 2026-09-30 至 2026-10-06 这 7 行全部是 `kind='confirm_outcome'`、`owner='human'`、`requires_approval=1` 的**人工确认闸门**，7 个项目各 1 条（无重复派发），本来就在等用户处理，页面"需要处理"面板会列出。真正执行的一套用**另一张表** `goal_action_runs`（带 `available_at`/`deadline_at`/`client_request_id`，状态 `queued`/`running`/`succeeded`/`cancelled` 等，另有 `reconcile_action_runs` 与截止时间清扫）：只读复核为 1 行 `queued`（2026-10-05 创建、截止时间 2026-10-12 尚未到期），过期未处理数为 0。⇒ 没有卡住的执行 | 记录更正：等待用户确认是正常状态；是否逐项确认或放弃仍由你决定 |
| `maitu_tasks` 与动作记录之间没有关联列 | `maitu_tasks` 有 `latest_attempt_id`、`accepted_attempt_id`、`task_kind`，但没有指向动作记录的列。`action_runs` 通过 `node_id`、`artifacts.action_run_id`、`evidence.action_run_id` 关联；执行侧的调度记录另存 `goal_action_runs`（有 `subject_kind` 与 `client_request_id`，但没有 `maitu_tasks` 主键）。⇒ "哪个任务产出这条动作、哪条动作跑了这个任务"在 schema 上无法表达 | 已记录；#11（待合并）更新路线图状态 |
| 尚未合并的修复与推荐顺序 | 2026-10-10 用 `git merge-tree --write-tree` 复核全部 **12** 个开放 PR：12 条分支相对 `main` 都是落后 0 提交、都能干净合入；两两组合 66 种里只有三对冲突，都集中在 `#14`、`#8`、`#7` 这一组（`#14`×`#8` 与 `#14`×`#7` 踩 `scripts/quality-gate.sh`，`#8`×`#7` 踩 `scripts/README.md`），其余 63 种均可干净合并；新增的 `#22` 与其他任何分支都不冲突。核对方式：先取 GitHub API 的 `head.sha`，再与本地 `origin/<分支>` 逐条比对，12 条全部一致后才采信矩阵（一次 `git fetch` 曾被网络重置，不能只看本地引用就下结论）。`#21` 依赖 `#20`（用例的确定性靠持锁成立），两者 CI 均已全绿 | **需你决定**：建议顺序 **#20 → #21 → #7 → #12 → #17 → #9 → #11 → #15 → #18 → #8 → #14 → #22**。先合 `#20`/`#21` 这对有依赖关系的；`#7` 是 48 提交的大分支，先合它可让相冲的 `#8`、`#14` 只需一次 rebase；`#22` 是纯文档且与任何分支无冲突，位置可任意 |
| 五个旧库的删除 | 内容迁移与独立恢复验收已完成，删除条件是独立状态 | **需你决定**：未达删除条件，原库继续保留 |
| 远端保留了较多历史分支（含已合并的） | 只做过统计，未改动 | **需你决定**：清理需要授权 |
| CI 会因匿名拉取 caddy 镜像撞 Docker Hub 限流而整门失败 | `c227786`（只改文档）的 run #169/#170：Rust 测试、生产镜像与存储核对**都已通过**，随后 `Unable to find image 'caddy:2.11.4-alpine@sha256:5f5c…' locally`、`docker: Error response from daemon: toomanyrequests: You have reached your unauthenticated pull rate limit`，以退出码 125 失败 | **需你决定**：限流是六小时配额，"加重试"在证据上解决不了，需要配置 Docker Hub 凭据（`docker/login-action` + 仓库 secret）或调整该步骤；此后多次运行（#175、#176、#181 至 #189，其中 #183、#184 因被新推送取代而取消）未再出现该失败，与六小时配额的间歇特征一致，所以它是**间歇性**风险，不是每次都红 |

补充两条诊断时用得到的事实，免得重复踩坑：fixture 的调用计数在返回 429 **之前**自增（`calls` 的 entry 写在 flaky 分支之前），所以"已收到请求"不等于"响应已发出"；`ProviderStore::delete` 拒绝删除最后一个连接（`provider.rs:383`，提示"至少保留一个模型连接"），而 `save` 与 `delete` 都会立即更新内存里的连接表（`provider.rs:370`、`389`），所以删掉的连接不会再被领取。

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