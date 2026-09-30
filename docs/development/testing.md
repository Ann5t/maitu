# 测试指南

> 状态：当前操作指南。

## 日常检查

```bash
make check
```

该命令运行 Markdown 链接检查、rustfmt、Clippy（警告视为错误）和全部 Rust 测试。文档检查也可以单独运行：

```bash
./scripts/check-docs.py
```

## 完整质量门

```bash
./scripts/quality-gate.sh
```

完整入口依次检查：

1. Markdown 结构和相对链接；
2. Rust 格式、Clippy 和测试；
3. 空库、增量迁移和旧数据 fixture；
4. 目标、上下文、工作区、调度、审核、工具和输入 HTTP 闭环；
5. 桌面、平板、手机及大图 Chromium 测试；
6. 非 root 生产镜像、安全 HTTPS、存储调和和备份恢复；
7. 工作树空白错误检查。

质量门使用唯一命名的一次性容器、网络和数据库，不连接 Compose 正在使用的正式卷。浏览器截图默认写入临时目录；需要更新文档视觉证据时显式设置 `SCREENSHOT_DIR=docs/assets/screenshots`，并人工审查差异。

专项脚本及其覆盖范围见[脚本索引](../../scripts/README.md)。

## 脉图资料任务

完成依赖下载与 Rust 检查后运行 `./scripts/test-maitu-workflow.sh`，使用隔离 PostgreSQL 与本机 HTTP 测试接口验证请求并行、失败隔离、显式重试、版本引用、并发调整、中断恢复及大量依赖等待时的可执行任务。该检查已加入完整质量门，但不能证明 DeepSeek 账户已接通。

`tests/browser/maitu.spec.js` 使用真实网页表单和数据库，验证资料导入、创建与启动、等待说明、失败与历史、刷新和移动布局。它用本机关闭端口验证网络失败，不调用付费服务。真实 DeepSeek 验收单独保存在[实施进度](maitu-progress.md)。
