# 恢复与切换说明

> 状态：当前操作指南；历史取证与当前备份必须分开处理。

本页区分两种完全不同的资产：2026-08-21 的源码取证归档，以及当前 Rust 系统的可运行数据备份。取证归档不能代替 PostgreSQL/Git/对象备份；旧 `scripts/backup.sh` 也不覆盖目标枝干新增的 repositories、worktrees 和 runner_outputs。

安全部署、secret 和升级入口见[安全部署](deployment.md)。原始验收边界保存在[已归档的 BP-09](../archive/branch-proposals/private-deployment-v1.md)。

## 已找到并保留的旧资产

- `fudian-app` 镜像保留了桌面版演进而来的旧界面、编译产物和 Drizzle 迁移；
- `fudian-nextgen-app` 镜像保留大部分 TypeScript 源码、测试、组件和初始数据库逻辑；
- `fudian-nextgen-app-prod` 镜像保留最新编译页面、项目图谱服务和数据库 schema 源码；
- `fudian_nextgen_postgres_data` 与 `fudian_nextgen_artifacts` 保留恢复时的真实数据；
- 排除依赖和编译缓存后的源码已固化为 `recovery/legacy-docker-source-20260821.tar.gz`，由 `recovery/SHA256SUMS` 校验。

Rust 仓库不依赖取证目录或 `.next` 反编译产物才能构建。任何预览、迁移和测试都不得写入上述真实卷；当前 Goal 的自动化使用唯一命名的一次性容器、网络、卷和 PostgreSQL。

## 当前可恢复单元

`scripts/backup-v2.sh` 同时保存：

- PostgreSQL custom-format dump 与业务/迁移计数；
- artifacts、bare repositories、goal worktrees、runner outputs 四个卷的压缩归档和逐文件 SHA-256；
- 可构建 Rust 源码、14 个迁移的目录摘要；
- 脱敏的 Git commit/dirty、容器 ID、不可变镜像 ID、镜像上的源码修订标签和存储类型元数据；
- 覆盖所有文件的 `SHA256SUMS`。

备份开始前，源数据库必须运行、四个卷必须显式命名。提供应用容器时还会先执行所有托管 bare repository 的严格 `git fsck`；校验失败不会发布最终备份目录。源码归档不包含 `.env`、`secrets/`、真实 secret 或 Docker 数据卷。

部署元数据（`deployment-metadata.json`，`schemaVersion` 3）把这次备份能自证的信息写下来：`sourceCommit`/`sourceDirty` 是归档源码所在工作树的状态，`appContainerId`/`appImageId` 是运行中的应用容器与镜像，`appImageRevision` 取自镜像标签 `org.opencontainers.image.revision`，`sourceMatchesRunningImage` 是两者的同源判断（`true`/`false`；镜像没有该标签时为 `null`）。

源码修订标签由 `Dockerfile` 的 `FUDIAN_SOURCE_REVISION` 构建参数写入；`scripts/start-local.ps1` 与 `compose.maitu.yaml` 在构建时带上当前工作树的提交，工作树不干净时带 `-dirty`。`sourceMatchesRunningImage` 为 `false` 时，备份脚本会在结尾明确告警：归档源码与运行镜像不同源，恢复后的行为可能不同；为 `null` 时说明镜像没有该标签，这次备份无法完成这项自证，用当前入口重新构建镜像即可。

需要这条判断，是因为生产镜像只有运行层、没有构建它的源码：只比对 commit 或镜像 ID 都不能说明两者同源。历史备份 `20261009T121159Z` 记下的 `appImageId` 是 `sha256:7f6d55df3e16…`，而该镜像实际提供的界面源码并不在 `main` 上（由尚未合入的界面分支保存）。带上修订标签后，这类偏差会在备份时就报出来，而不是等到恢复时才发现。

`scripts/restore-v2.sh` 只写入满足全部条件的目标：

- 数据库容器和四个卷都带 `com.fudian.restore-target=true`；
- public schema 无表，四个卷没有任何条目；
- 调用者显式设置 `FUDIAN_RESTORE_CONFIRM=EMPTY_LABELED_TARGETS`；
- 备份总校验和、tar 路径、逐文件摘要和恢复后的 Git `fsck` 全部通过。

脚本不使用 `--clean`，不删除、不覆盖、不猜测目标。真正的灾难恢复前也应先保存故障现场，避免为了恢复一个版本而失去另一个可调查版本。

## 已完成的跨版本演练

`scripts/test-backup-recovery.sh` 在隔离环境中执行了完整路径：

1. 从提交 `0a73f58` 构建 BP-08 旧应用并建立包含 12 条迁移、Project、GoalBranch、Git worktree、Artifact 和 Runner 输出的源现场；
2. 对 BP-08 空仓库曾遗漏的 canonical empty-tree 对象执行一次明确修复，并在备份前通过严格 `git fsck`；当前代码已改为初始化时写入该对象且有 Rust 回归测试；
3. 生成 v2 备份并验证没有数据库口令进入源码或部署元数据；
4. 恢复到带专用标签的空 PostgreSQL 和四个空卷，逐文件、Git 和数据库复核；
5. 启动当前镜像，把 schema 从 12 条迁移升级到 14 条，确认业务计数和 Artifact 哈希不变；
6. 停止当前镜像，重新启动 BP-08 应用，确认它能读取升级后的数据库，且不会删除不认识的 `0013`/`0014`。

这证明应用回退路径，不等于 schema downgrade。正常回退只恢复上一审核镜像；不能删除 `schema_migrations`、不能回滚数据库文件，也不能用 `pg_restore --clean` 覆盖当前实例。

## 真实切换仍需人工授权

当前真实 PostgreSQL 是早于新 Compose 端口约束启动的容器，仍有 `0.0.0.0:55432` 的既存风险。修复需要先完成经过复核的异地 v2 备份，再在明确维护窗口重建容器并复用数据卷。这个操作会改变真实服务可用性和端口，不在自动 Goal 的授权范围内。

真实部署还需用户决定：

- 私人网络 + Caddy 内部 CA，还是公网域名 + ACME；
- 真实主机防火墙、DNS 和各设备证书信任；
- 备份复制到哪块独立磁盘或对象存储，以及恢复演练频率；
- 哪个审核过的镜像 digest 是当前版和回退版。

在这些决定和维护授权之前，仓库只交付可重复部署候选与隔离证据，不自行公开端口、变更 DNS、推送镜像或操作真实卷。
