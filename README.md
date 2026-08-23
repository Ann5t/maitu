# 浮点（Fudian）

Fudian 是一个用目标枝干组织长期工作、由 Agent 推进并由用户最终审查的自托管项目系统。本仓库是产品第一版的 Rust 全栈实现。

当前应用采用 Axum、SQLx、Maud 和 PostgreSQL。页面由 Rust 服务端渲染，原生 JavaScript 只承担渐进增强；项目文件工作区使用 Git worktree。

## 快速开始

在本机环回地址启动预览：

```bash
APP_PORT=3001 make start
```

打开 `http://localhost:3001`。

`compose.yaml` 明确关闭身份认证，只能用于本机预览。不要把它直接暴露到公网，也不要运行 `docker compose down -v`；后者会请求删除数据卷。服务器部署从[安全部署指南](docs/operations/deployment.md)开始。

开发模式：

```bash
APP_PORT=3001 make dev
```

本机直接运行 Rust 服务时，可以把 `.env.example` 复制为被 Git 忽略的 `.env`。

## 检查改动

日常检查：

```bash
make check
```

提交前完整质量门：

```bash
./scripts/quality-gate.sh
```

完整质量门使用隔离的 PostgreSQL、Git worktree、OCI Worker 和 Chromium，不连接正式数据卷。检查层次和专项入口见[测试指南](docs/development/testing.md)。

## 文档入口

- [文档总览](docs/README.md)：现行文档、状态和阅读顺序
- [产品设计](docs/product/README.md)：产品意图与界面方向，当前仍在讨论
- [实现架构](docs/architecture/README.md)：已经落地的系统边界
- [接口参考](docs/reference/README.md)：HTTP API、工具和输入协议
- [运维指南](docs/operations/README.md)：安全部署、备份和恢复
- [架构决策](docs/decisions/README.md)：重要选择及其理由
- [历史归档](docs/archive/README.md)：阶段计划、进度、验收和旧原型
- [贡献指南](CONTRIBUTING.md)：代码、测试和文档改动规则

历史 Docker 源码取证材料独立保存在 [recovery/](recovery/README.md)，不参与当前构建。

## 仓库结构

Cargo 约定目录保持不变：源码位于 `src/`，集成测试位于 `tests/`，其他可执行文件位于 `src/bin/`。数据库迁移、部署配置、插件夹具和运行脚本各自保持独立。完整边界见[仓库布局](docs/development/repository-layout.md)。

## 技术基线

- Rust 2024 edition；最低 Rust 1.94，构建与 CI 固定 Rust 1.97
- Axum 0.8、SQLx 0.9、Maud 0.27
- PostgreSQL 17
- Debian slim 非 root 生产镜像

依赖版本固定在 `Cargo.lock`。批量升级依赖前先完成备份，并运行完整质量门。
