# 恢复与切换说明

## 已找到的资产

2026-08-21 对本机 Docker 做了只读盘点：

- `fudian-app` 镜像保留了桌面版演进而来的旧界面、编译产物和 Drizzle 迁移。
- `fudian-nextgen-app` 镜像保留了大部分 TypeScript 源码、测试、组件和初始数据库逻辑。
- `fudian-nextgen-app-prod` 镜像保留了最新编译页面，以及最新的项目图谱服务和数据库 schema 源码。
- `fudian_nextgen_postgres_data` 中保留现有项目和完整项目 DAG。
- `fudian_nextgen_artifacts` 中保留文件产物。

这些内容被用于恢复行为、数据模型和旧版视觉语言。Rust 仓库是新的可维护源代码，不依赖临时恢复目录或 `.next` 反编译产物才能构建。

排除 `node_modules` 与 `.next` 后的取证文件已经固化为 `recovery/legacy-docker-source-20260821.tar.gz`，并由同目录的 `SHA256SUMS` 校验。原生产镜像还额外保留为 `fudian-nextgen-app-prod-legacy:recovered-20260821`，供应用级回退使用。

盘点和预览期间没有删除或重建任何现有卷。当前真实数据库仍只有恢复时已有的迁移登记（包括 `0001_rust_baseline.sql`）；本功能枝干新增的 `0002_goal_branch_core.sql` 至 `0008_context_memory.sql` 只在一次性 PostgreSQL 中验证，尚未应用到真实库。未来首次用本枝干镜像启动时会以只增不减方式执行它们，因此仍须先备份再预览。

当前仓库的 Compose 默认把应用和 PostgreSQL 都限制在 `127.0.0.1`。早于该设置启动的容器不会自动改变既有端口映射；切换前应使用 `docker inspect` 核对，若数据库仍映射到 `0.0.0.0`，请在维护窗口、完成备份后重建 PostgreSQL 容器但保留数据卷。

## 推荐切换流程

1. 创建备份并校验 `SHA256SUMS`：

   ```bash
   make backup
   sha256sum -c backups/<时间>/SHA256SUMS
   ```

2. 在备用端口预览：

   ```bash
   APP_PORT=3001 make start
   curl --fail http://localhost:3001/api/health
   ```

   启动日志应只增加七条新迁移登记（`0002`–`0008`）；旧项目、旧分支和旧节点计数应保持不变。已有 Session 的 `context_snapshot_id` 保持空值，不会伪造历史快照；新 Session 或下一次安全生命周期边界才建立可审计快照。

3. 用浏览器检查两个现有项目的图谱、契约、产物和历史，不先执行写动作。

4. 确认后停止占用 3000 端口的旧应用容器；不要停止或删除 PostgreSQL 和产物卷。

5. 在 3000 端口启动 Rust 生产服务：

   ```bash
   APP_PORT=3000 make start
   ```

6. 通过健康检查和一个非关键测试项目完成写入验收。

## 回退

Rust 服务和数据库共享现有 schema，因此应用回退不需要回滚数据：

1. 停止 `app-prod`；
2. 重新启动原 `fudian-nextgen` 应用镜像；
3. 保持 `fudian_nextgen_postgres_data` 和 `fudian_nextgen_artifacts` 原样挂载。

不要为了应用回退删除 `0001_rust_baseline.sql` 迁移记录；旧应用会忽略不认识的迁移名。若确实要恢复到备份时刻，请先把当前卷做第二份快照，再使用 `pg_restore --clean --if-exists` 恢复数据库，并单独恢复产物归档。这是破坏性操作，不应作为常规回退手段。

## 备份内容

`scripts/backup.sh` 生成：

- `database.dump`：PostgreSQL custom-format dump
- `artifacts.tar.gz`：产物卷归档
- `source.tar.gz`：当前 Rust 工程的可构建源码
- `source.tar.gz` 内的 `recovery/`：旧镜像取证源码及校验和
- `SHA256SUMS`：前三个文件的完整性校验

备份必须复制到 Docker 主机之外或版本化对象存储中；只放在同一块磁盘上无法防止再次丢失。
