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
