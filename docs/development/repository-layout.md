# 仓库布局

> 状态：当前工程规则。

本仓库优先遵循 Cargo、Docker Compose 和 GitHub 的常见位置。没有明确收益时，不为“整齐”改动工具默认目录。

| 路径 | 内容 | 规则 |
| --- | --- | --- |
| `src/` | Rust 应用、领域、Web 和可执行文件 | `main.rs` 为主服务，其他程序放在 `src/bin/` |
| `tests/` | 浏览器、SQL 和插件夹具 | 按测试媒介分目录，不混入生产代码 |
| `migrations/` | 顺序 SQL 迁移 | 只追加，不改写已发布文件 |
| `assets/` | 浏览器直接使用的 CSS、JavaScript 和图标 | 不引入未声明的前端构建产物 |
| `plugins/` | 隔离工具镜像与测试 Runtime | 与主应用镜像分开 |
| `scripts/` | 质量门、专项验证和运维脚本 | 入口和用途见 [scripts/README.md](../../scripts/README.md) |
| `deploy/` | Caddy 配置 | Compose 文件保留在根目录以符合发现惯例 |
| `docs/` | 当前文档、ADR 和历史归档 | 从 [docs/README.md](../README.md) 导航 |
| `recovery/` | 历史源码取证包及校验 | 不参与当前构建 |
| `secrets/` | 本地文件型 secret | 只跟踪 `.gitkeep`，真实内容禁止提交 |
| `.github/` | GitHub Actions | 复用仓库内质量门，不复制测试逻辑 |

根目录只保留工具会主动寻找或首次使用必须看到的文件：`README.md`、`CONTRIBUTING.md`、Cargo 清单、Compose 文件、Dockerfile、Makefile 和环境示例。

`target/`、`backups/`、真实 `.env*`、运行数据和测试报告是生成或私有状态，必须保持忽略。整理仓库时不移动、展开或提交这些目录。

源码模块按稳定职责拆分，而不是按行数拆分。只有当一个文件同时承担多个可独立测试、可清楚命名的职责时，才迁移为子模块；这类重构应与功能变化分开审查。
