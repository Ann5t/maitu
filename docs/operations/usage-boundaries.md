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
2. **检出**：`main` 目前没有 `.gitattributes`，Windows 检出是 CRLF；Git 里存的仍是 LF。直接 `bash scripts/*.sh` 会把 CR 当成命令的一部分（`bash -n` 也会失败，`BASH_SOURCE` 定位可能落到错误的根目录）。修正在 **#7（待合并）**；在那之前请用 LF 检出（例如 `git -c core.autocrlf=false worktree add <目录> <提交>`）或经容器执行。
3. **镜像**：正在运行的活实例仍是旧镜像 `sha256:7f6d55df3e16`，**不含**下面第三节列出的修复。升级路径已在本机演练通过，但**尚未应用**到日用实例。
4. **凭据**：备份默认**不含**模型连接凭据（`credentialStoreIncluded: false`）。迁移或另行保管连接配置时，需要单独复制 `provider_config` 卷。真实 secret、`.env`、数据库、模型文件、构建缓存和运行日志都不进入 Git。
5. **入口**：`maitu` 保持**一个**实际运行的产品入口；五个旧库作为来源档案保留，不作为并列应用长期启动。

## 三、待修问题与待决定事项

| 事项 | 证据 | 状态 |
| --- | --- | --- |
| 限流故障转移用例有时序性失败 | 同一个提交 `b009cc5` 的两次运行**一成一败**（`pull_request` 通过、`push` 失败），失败断言是 `src/maitu/integration.rs:690` 的 `assert_eq!(produced.attempts.len(), 2, "one failover retry, no more")`，实测 `left: 1`；同一份代码在 `infra/backup-host-write-archives` 的两次运行均通过 | **待修**：需要查清"任务已标 `produced` 但第二次尝试记录未被读到"的竞态，或让用例不依赖该时序 |
| 备份目录在 Windows 盘上可能变得无法恢复 | 宿主目录里一旦出现由容器创建的文件，再经 `mv` 改名，该目录在 WSL 侧显示 `d?????????`、只读挂载报 `mkdir …: file exists`；旧写法可复现，改为宿主落盘后不可复现，真实数据上也验证过 | 修复在 **#17（待合并）**，运行期自检在 **#12（待合并）** |
| Windows 上按 CRLF 检出仓库 | 仓库脚本 `bash -n` 失败、`BASH_SOURCE` 相对根定位错误 | 修复在 **#7（待合并）** |
| 两份原地坏备份 `20261009T013722Z`、`20261009T121159Z` | Windows 侧可正常读取、WSL 与容器侧不可读；**内容完好**，复制成新目录即可用于恢复（已实测校验与解包） | **需你决定**：是否复制留存或重做备份；原件未删 |
| 陈旧 Docker 数据约 9.7 GB | 三个当前 compose 未声明、也无容器使用的命名卷（`maitu_check_target` 5.1 GB、`maitu_check_registry` 270 MB、`maitu_check_git` 4 KB），加 83 个悬空匿名卷约 4.3 GB | **需你决定**：清理需要授权 |
| 7 个 `action_runs` 停在 `ready` | 2026-09-30 至 2026-10-06，均为待人工确认的成果确认 | **需你决定**：逐项确认或放弃 |
| `maitu_tasks` 与 `action_run` 没有关联列 | `maitu_tasks` 现有列中有 `latest_attempt_id`、`accepted_attempt_id`、`task_kind`，但没有指向 `action_runs` 的列；两套执行系统的关系没有落库 | 已记录；#11（待合并）更新路线图状态 |
| 尚未合并的修复与推荐顺序 | 开放 PR 两两之间的冲突已用 `git merge-tree --write-tree` 核实（`scripts/quality-gate.sh`、`scripts/README.md` 上存在真实冲突） | **需你决定**：建议顺序 **#7 → #12 → #17 → #9 → #11 → #15 → #8 → #14**；`#8`、`#14` 合并前需要一次 rebase |
| 五个旧库的删除 | 内容迁移与独立恢复验收已完成，删除条件是独立状态 | **需你决定**：未达删除条件，原库继续保留 |
| 远端保留了较多历史分支（含已合并的） | 只做过统计，未改动 | **需你决定**：清理需要授权 |

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