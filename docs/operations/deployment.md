# 安全部署

> 状态：当前操作指南。安全约束和隔离验收来源于[已归档的 BP-09](../archive/branch-proposals/private-deployment-v1.md)。

## 选择运行方式

- 本机预览：使用 `compose.yaml`，只绑定环回地址，身份认证关闭。
- 私有 HTTPS：使用 `compose.secure.yaml` 和 `deploy/Caddyfile.private`。
- 公网域名：在安全 Compose 上叠加 `compose.secure-public.yaml`，并由用户配置 DNS、防火墙和真实证书入口。

应用和 PostgreSQL 不应直接向公网发布端口。Caddy 是安全部署的唯一入口。

## 准备 secret

在目标服务器创建被 Git 忽略的 `secrets/`。不要把真实值复制到聊天、Issue 或日志。

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

## 启动私有 HTTPS

把 `.env.secure.example` 复制为被忽略的 `.env.secure`，设置站点主机、精确外部 Origin、应用镜像和 Runner digest：

```bash
docker compose -f compose.secure.yaml --env-file .env.secure config --quiet
docker compose -f compose.secure.yaml --env-file .env.secure up -d --no-build
docker compose -f compose.secure.yaml --env-file .env.secure ps
curl --fail --insecure "${FUDIAN_PUBLIC_ORIGIN}/api/health"
```

私人网络模板默认使用 Caddy 内部 CA。把根证书安装到需要访问的电脑、手机和平板；不要通过关闭 HTTPS 校验绕过证书问题。

首次访问 `/auth/setup` 时使用 setup token 创建唯一 Owner，并立即把只显示一次的恢复码保存到离线密码管理器。

## 启动公网候选

只有在 DNS、防火墙和公开入口已经由用户确认后才能执行：

```bash
docker compose -f compose.secure.yaml -f compose.secure-public.yaml \
  --env-file .env.secure config --quiet
docker compose -f compose.secure.yaml -f compose.secure-public.yaml \
  --env-file .env.secure up -d --no-build
```

将 `FUDIAN_CADDYFILE` 指向 `deploy/Caddyfile.public`，并把站点主机、外部 Origin 和 HTTPS 监听地址改为真实值。

## 升级前检查

升级前先按照[备份与恢复](recovery.md)创建并校验备份。记录当前镜像 digest 和业务计数，再启动新镜像并验证 HTTPS 健康、迁移数量和只读业务抽样。应用回退只恢复上一审核镜像，不执行数据库 schema downgrade。
