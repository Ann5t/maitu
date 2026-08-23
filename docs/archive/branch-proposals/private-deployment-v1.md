# BP-09：单用户私有部署、安全与恢复目标契约

> 历史归档：本文保留当时的目标、过程或证据，不定义当前产品行为。

## 目标

在不触碰当前真实生产卷、不擅自公开部署的前提下，把 Fudian 收口为可在中等性能服务器上安全私有运行的候选：单用户身份、浏览器会话、所有写请求防伪造、限速、可信代理边界、HTTPS 入口、密钥文件、受保护的持续工具入口，以及 PostgreSQL、Git、对象和配置元数据的备份—恢复—升级—回退闭环。

## 硬约束

- 安全模式默认拒绝启动不完整的远程配置；无认证模式只能通过明确开关启用，并且不得被安全 Compose 使用。
- 口令使用 Argon2id 和独立随机 salt；pepper、初始化凭据和数据库密码从文件读取，不进入 Git、普通产物、日志或备份包。
- 会话 token 全随机、客户端无意义、服务端只存带 pepper 的摘要；Cookie 使用 `Secure`/`HttpOnly`/`SameSite=Strict`/`Path=/`，登录、口令恢复和定期边界轮换 Session ID。
- 所有非安全 HTTP 方法都要求已认证会话，并通过服务端固定的目标 Origin 与 CSRF token 双重核对；`SameSite` 只是额外防线。Worker/launcher 机器端点继续使用独立 Bearer/Lease 凭据，不伪装成浏览器会话。
- 只有明确列出的可信 CIDR 才能提供 `Forwarded`/`X-Forwarded-*`；其他来源的这些头被忽略并不用于限速、审计或 HTTPS 判定。
- 应用和 PostgreSQL 不向宿主机公开端口；只有反向代理入口可发布。真实域名、证书和服务器防火墙仍须用户决定。
- 备份必须先成功并校验，才能进入升级；恢复和回退仅在一次性卷/网络演练。当前真实 PostgreSQL 的公开 `55432` 映射只记录为待维护风险，不在未授权窗口重建。
- 调和和孤儿清理先只读扫描和预览；执行清理时先移入内部 quarantine，保留期满前可恢复，不直接删除。

## 最低验收表

- [x] 从空库进行受 setup token 保护的单用户初始化，只显示一次恢复码；并发初始化只成功一次。
- [x] 浏览器完成登录、登出、会话轮换、会话到期、一次性恢复码改密并令旧会话/旧恢复码失效；安全审计不含原始凭据。
- [x] 所有已有 HTML/API 写路由统一中间层保护：缺会话、缺/错 CSRF、错 Origin 和跨站请求均失败；同源表单、带 token API 和上传正常。
- [x] 登录/恢复按客户端与账户双桶限速，高成本写入另有限额；服务重启后限速记录仍有效。
- [x] 伪造代理头不影响客户端身份；只有来自配置 CIDR 的 HTTPS 代理请求被接受，响应包含 HSTS、CSP、`nosniff`、DENY frame 和不缓存私有页面。
- [x] 活跃 ToolLease 只能通过已认证的同源代理访问；过期/终止 Lease、非允许 endpoint、越限 body/响应和 hop-by-hop/Cookie 头均被拒绝或剔除。
- [x] 安全 Compose 通过非 root、只读根、内部应用/DB、只有 HTTPS 入口、健康检查、资源限额、日志轮换、持久卷与文件 secret 验证；安全模式缺配置时启动失败。
- [x] 备份包含 PostgreSQL、Git repositories、内容对象/产物、Runner 产出、可构建源码和脱敏部署元数据，并且校验和、迁移目录和镜像摘要可核对。
- [x] 从备份恢复到空卷/空库后，逐项校验数据计数、Git `fsck`、对象哈希和环境清单；随后升级当前镜像，再用上一个候选镜像进行应用回退启动。
- [x] 孤儿分段、对象、repository/worktree 的只读扫描与预览结果稳定；故障注入的半状态能调和，清理只移入 quarantine 并可恢复。
- [x] Chromium 通过隔离 HTTPS 在 1440/820/390 完成 setup/login/关键项目流程，无混合内容、页面溢出或不安全 Cookie。
- [x] 完整质量门从头通过，真实生产容器 ID/启动时间、`3:8:23:2`、原 4 条迁移与两个 Artifact 摘要不变，没有测试容器/网络残留。

## 诚实未知与暂停边界

- 真实访问最终使用 Tailscale/其他私人网络，还是公网域名 + ACME，必须由用户决定。仓库只提供两种明确模板，不擅自绑定域名。
- 用户的真实用户名、口令、恢复码、域名、证书和通知渠道均不在本枝干代为创建。
- 当前真实 PostgreSQL 容器的 `0.0.0.0:55432` 只能在用户审查备份并明确给出维护窗口后处理；到该边界必须暂停并通知用户。
- 自动测试可证明部署候选的安全不变量与恢复能力，不能代替真实服务器防火墙、DNS、证书信任和异地备份的用户验收。

## 已实现边界

- `FUDIAN_SECURITY_MODE=required` 是安全 Compose 的固定值；普通 `compose.yaml` 仅为显式关闭认证的本机兼容预览。
- 浏览器入口只接受配置的精确 HTTPS Origin。应用只信任 Caddy 在内部 edge 网络上的固定地址，其他同网段或伪造转发头不会获得代理身份。
- Caddy 是唯一加入 ingress 网络的服务。私有模板只发布 `127.0.0.1:8443`；应用、PostgreSQL 和工具网段没有宿主端口。Docker 要求发布端口的容器至少加入一个非 internal 网络，因此 ingress 本身不是“无出站”沙箱；它只承载最小 Caddy 配置，应用仍隔离在 internal edge/data/tools 网络。
- ToolLease 页面不泄漏内部 endpoint。只有活动且未过期的 Lease 能通过同源 `/api/v1/tool-leases/:id/proxy/:index/...` 访问，代理不跟随重定向，不使用宿主代理，并限制目的 CIDR、方法、头、请求体和响应体。
- 原始 Runner/Tool 日志受资源上限约束；权威结果、摘要、哈希、ActionEvent 和审计记录保存在 PostgreSQL，明确保留的输出进入 Runner 卷并随备份保存。未引用文件先经 7 天默认保留期、扫描和 quarantine，当前实现不自动永久删除。

## 准备文件型 secret

下面命令应由部署者在目标服务器执行。`secrets/` 已被 Git 忽略；不要把真实值复制到聊天、Issue、日志或备份源码包中。

```bash
install -d -m 0700 secrets
umask 077
database_password="$(openssl rand -hex 32)"
printf '%s' "$database_password" > secrets/postgres-password
printf 'postgres://fudian:%s@postgres:5432/fudian' "$database_password" > secrets/database-url
openssl rand -hex 32 > secrets/auth-pepper
openssl rand -hex 32 > secrets/setup-token
openssl rand -hex 32 > secrets/worker-bootstrap-token
unset database_password

chown root:root secrets/postgres-password
chmod 0400 secrets/postgres-password
chown 1000:1000 secrets/database-url secrets/auth-pepper secrets/setup-token secrets/worker-bootstrap-token
chmod 0400 secrets/database-url secrets/auth-pepper secrets/setup-token secrets/worker-bootstrap-token
```

本地 Docker Compose 的 file secret 保留宿主文件 UID/mode；因此应用读取的四个文件必须能被容器 UID 1000 读取。数据库镜像入口先以 root 读取自己的口令文件，随后降权运行 PostgreSQL。

## 私人网络或本机 HTTPS

复制 `.env.secure.example` 为一个被忽略的 `.env.secure`，至少替换站点主机、精确外部 Origin、审核过的应用镜像和 Runner digest。若默认 `172.30.0.0/24`–`172.32.0.0/24` 与宿主已有 Docker 网络重叠，要成组修改三个子网及其固定地址。

```bash
docker compose -f compose.secure.yaml --env-file .env.secure config --quiet
docker compose -f compose.secure.yaml --env-file .env.secure up -d --no-build
docker compose -f compose.secure.yaml --env-file .env.secure ps
curl --fail --insecure "${FUDIAN_PUBLIC_ORIGIN}/api/health"
```

内部 CA 根证书位于 Caddy 数据卷中的 `/data/caddy/pki/authorities/local/root.crt`。把它导出后，分别安装到需要访问的电脑、手机和平板的系统信任库；未完成设备信任前，浏览器警告是预期行为，不能通过关闭应用的 HTTPS 校验来绕过。

```bash
docker compose -f compose.secure.yaml --env-file .env.secure \
  cp caddy:/data/caddy/pki/authorities/local/root.crt ./fudian-private-root.crt
```

首次访问 `/auth/setup` 时使用 setup token 创建唯一 owner。页面只显示一次恢复码；应立即保存到离线密码管理器。setup 完成后数据库只保留摘要，再次初始化会冲突失败。

## 公网域名 + ACME 候选

只有在用户已配置 DNS、防火墙并明确授权公开入口后，才叠加公网模板。应用和数据库仍不发布端口；Caddy 额外发布 80 用于 ACME/跳转，443 是唯一业务入口。

```bash
docker compose -f compose.secure.yaml -f compose.secure-public.yaml \
  --env-file .env.secure config --quiet
docker compose -f compose.secure.yaml -f compose.secure-public.yaml \
  --env-file .env.secure up -d --no-build
```

公网环境必须将 `FUDIAN_CADDYFILE` 指向 `deploy/Caddyfile.public`，把 `FUDIAN_SITE_HOST` 设为真实域名、`FUDIAN_PUBLIC_ORIGIN` 设为无歧义的外部 HTTPS Origin，并把 HTTPS bind/port 显式设为 `0.0.0.0:443`。仓库不会代替用户购买域名、改变 DNS 或开放防火墙。

## 备份、升级与应用回退

`backup-v2.sh` 要求明确写出源数据库容器和四个卷；源卷只读挂载。提供应用容器时，脚本会在落盘前对所有托管仓库运行严格 `git fsck`。任何失败都留下 `.partial-*`，不会发布为可恢复备份。

```bash
FUDIAN_BACKUP_DATABASE_CONTAINER=<compose-postgres-container> \
FUDIAN_BACKUP_APP_CONTAINER=<compose-app-container> \
FUDIAN_BACKUP_ARTIFACT_VOLUME=<project>_artifacts \
FUDIAN_BACKUP_REPOSITORY_VOLUME=<project>_repositories \
FUDIAN_BACKUP_WORKTREE_VOLUME=<project>_worktrees \
FUDIAN_BACKUP_RUNNER_VOLUME=<project>_runner_outputs \
  ./scripts/backup-v2.sh /path/on/separate-disk/fudian-backups
```

升级顺序固定为：校验备份 `SHA256SUMS` → 记录旧镜像 digest 和业务计数 → 拉取/构建新镜像 → `docker compose up -d --no-build` → HTTPS 健康、迁移计数和只读业务抽样 → 才允许普通写入。应用回退只把 `FUDIAN_APP_IMAGE` 改回审核过的前一 digest；不要删除迁移记录或执行 schema downgrade。当前隔离演练已实测旧 BP-08 应用可在 14 条迁移的数据库上启动。

`restore-v2.sh` 只接受 `com.fudian.restore-target=true` 的空数据库容器和空卷，并要求显式确认字符串。它用于灾难恢复演练或真正的空目标重建，不会覆盖当前实例。任何非空或未标记目标都会被拒绝。

## 存储调和

```bash
docker compose -f compose.secure.yaml --env-file .env.secure exec app \
  fudian-maintenance scan
docker compose -f compose.secure.yaml --env-file .env.secure exec app \
  fudian-maintenance quarantine
docker compose -f compose.secure.yaml --env-file .env.secure exec app \
  fudian-maintenance restore <quarantine-run-id>
```

`scan` 永远不移动文件；`quarantine` 只移动超过保留期且未被权威记录引用的文件；`restore` 在目标不存在时才原路移回。缺失引用和摘要不一致不会被“清理”，而会保留为需要人工处理的证据。

## 可重复验收证据

- `scripts/test-security-https.sh`：认证、会话、恢复码、CSRF/Origin、代理边界、持久限速、ToolLease 同源代理和三尺寸 Chromium；
- `scripts/test-storage-reconciliation.sh`：稳定扫描、故障半状态、四类孤儿、可逆 quarantine 和往返哈希；
- `scripts/test-backup-recovery.sh`：BP-08/12 迁移备份、空目标恢复、12→14 升级、Git/对象校验和旧应用回退；
- `scripts/test-secure-compose.sh`：真实 Compose 启动、文件 secret、非 root/只读根、资源/日志上限、网络和唯一 HTTPS 端口；
- `scripts/quality-gate.sh`：上述专项与既有全部领域、HTTP、Runner、插件、审核、浏览器和发行镜像门的总入口。
