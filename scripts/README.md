# 脚本索引

脚本默认从仓库根目录运行，并使用 `set -euo pipefail`。测试脚本必须使用隔离资源并在退出时清理。

## 总入口

- `quality-gate.sh`：本地与 CI 共用的完整质量门
- `check-docs.py`：Markdown 标题和本地链接检查

## 公共测试库

- `docker-test-lib.sh`：Docker 测试清理与文件权限辅助
- `review-worker-test-lib.sh`：审核 Worker 测试辅助

## 领域与 HTTP

- `test-goal-migrations.sh`
- `test-goal-http.sh`
- `test-context-http.sh`
- `test-ideas-http.sh`
- `test-inputs-http.sh`
- `test-tooling-http.sh`
- `test-workspace-runner-http.sh`
- `test-scheduler-http.sh`
- `test-review-integration-http.sh`
- `test-real-plugins-http.sh`

## 浏览器、镜像与安全

- `test-workbench-http.sh`
- `test-workbench-browser.sh`
- `test-workbench-large.sh`
- `test-plugin-images.sh`
- `test-production-image.sh`
- `test-security-https.sh`
- `test-secure-compose.sh`
- `test-storage-reconciliation.sh`
- `test-backup-recovery.sh`

## 构建与运维

- `build-plugin-images.sh`：构建代表性工具镜像
- `backup-v2.sh`、`restore-v2.sh`：当前完整备份与空目标恢复
- `backup.sh`：旧本机兼容备份入口

操作顺序见[测试指南](../docs/development/testing.md)和[运维指南](../docs/operations/README.md)。
