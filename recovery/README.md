# Docker 源码取证归档

`legacy-docker-source-20260821.tar.gz` 保存 2026-08-21 从下列本机镜像/容器中恢复的非依赖文件：

- 桌面版演进而来的 `fudian-app`
- 开发版 `fudian-nextgen-app`
- 生产版 `fudian-nextgen-app-prod`

归档排除了 `node_modules` 和 `.next`，保留可读源码、数据库迁移、测试、静态资源、包清单和生产镜像中仅存的服务端源码。它只用于行为比对和灾难恢复，不参与 Rust 版本的构建。

验证：

```bash
sha256sum -c recovery/SHA256SUMS
```

解包到新目录，不要覆盖当前工程：

```bash
mkdir -p /tmp/fudian-legacy-inspect
tar -xzf recovery/legacy-docker-source-20260821.tar.gz -C /tmp/fudian-legacy-inspect
```
