# Fudian 目标枝干核心 v0.1：执行进度

- Goal thread：`01a024d6-1224-7fe1-a7f2-ea8dafd226e4`
- 启动时间：2026-08-22T01:20:29+08:00
- 执行计划：[`overnight-plan.md`](overnight-plan.md)
- 产品基线：[`product-design.md`](product-design.md)

## 启动基线

- 工作目录在启动时尚未初始化 Git；
- Rust 生产预览：`fudian-nextgen-app-prod-1`，仅绑定 `127.0.0.1:3001`；
- 真实 PostgreSQL：`fudian-nextgen-postgres-1`；
- 启动计数：3 个项目、8 条旧模型分支、23 个旧模型节点、2 个产物；
- 最新备份 `backups/20260821T160940Z` 的数据库、产物和源码校验通过；
- 恢复归档 `recovery/legacy-docker-source-20260821.tar.gz` 校验通过；
- 主机没有 Cargo，Rust 质量门使用现有 Docker 构建环境；
- 未发现常见私钥、GitHub Token、OpenAI Key 或 AWS Access Key 模式；
- 没有大于 5 MiB 的非忽略文件。

## 里程碑状态

| 里程碑 | 状态 | 证据 |
| --- | --- | --- |
| 0. 安全基线与 Git | 已完成 | 基线提交 `60b6cad`；功能枝干 `feat/goal-branch-core-v0.1`；无远端；真实数据计数未变 |
| 1. 领域与协议基线 | 已完成 | `goal-branch-domain.md`、`tool-protocol.md`；关键不变量自动检索核对通过 |
| 2. 数据库与领域实现 | 已完成 | `0002_goal_branch_core.sql`；16 个 Rust 单元测试；隔离迁移/约束脚本通过 |
| 3. HTTP/API 纵向流程 | 进行中 |  |
| 4. 插件与环境版本基础 | 待开始 |  |
| 5. 安全文件导入 | 待开始 |  |
| 6. 项目图与工作台原型 | 待开始 |  |
| 7. 质量门与早晨交付 | 待开始 |  |

## 运行日志

### 2026-08-22 01:20 +08:00

- 创建持久 Goal；
- 写入有限里程碑执行计划；
- 核验当前服务、真实数据计数、备份、恢复归档和秘密/大文件边界；
- 扩充 `.gitignore`，避免误提交备份、恢复包、测试输出和日志。

### 2026-08-22 01:22 +08:00

- 初始化本地 Git，创建基线提交 `60b6cad`；
- 创建专用功能枝干 `feat/goal-branch-core-v0.1`，没有配置 Git 远端；
- 确认备份、数据卷、构建缓存、恢复压缩包和测试输出均不在跟踪列表中；
- 再次只读核验真实库计数：3 个项目、8 条旧模型分支、23 个旧模型节点、2 个产物；
- 第 0 级验收通过，开始第 1 级领域与协议基线。

### 2026-08-22 01:28 +08:00

- 完成 `docs/goal-branch-domain.md`：定义 Proposal、契约版本、目标枝干、Session、贡献、审核、事件、待处理和幂等收据；
- 为拟分支、请求判断、异常/手动暂停、恢复、拟合并、独立 AI 审核、用户退回/接受/部分接受和下一 Session 写明合法转换；
- 完成 `docs/tool-protocol.md`：定义插件 Manifest、不可变环境指纹、ToolCall/Result、ToolLease 和 InputArtifact；
- 明确新 `goal_` 模型只增不减地与旧探索图共存，旧数据不自动重解释；
- 核对用户最终决定权、单写者、冻结候选、请求幂等、固定插件版本和可替换图投影等关键不变量；
- 第 1 级验收通过，开始第 2 级数据库与领域实现。

### 2026-08-22 01:40 +08:00

- 新增只增不减的 `0002_goal_branch_core.sql`，建立 14 张 `goal_` 表；旧图表不重命名、不删除、不回填；
- 数据库约束 BranchProposal 批准来源、每枝干单一 `running` Session、冻结版本/事件/决定不可变；
- 新增 Rust `goal_domain`：目标契约最低充分明确度、Proposal/Session/审核状态机、用户最终权限、候选指纹和幂等语义；
- 新增可重复的 `scripts/test-goal-migrations.sh`，在临时 PostgreSQL 中验证空库、SQL 重放、旧基线增量迁移和关键拒绝路径；
- `cargo fmt --check`、Clippy `-D warnings`、16 个单元测试全部通过；隔离迁移报告 `legacy counts stayed 1:1:1`；
- 临时数据库容器已清理；真实库仍为 3 个项目、8 条旧模型分支、23 个旧模型节点、2 个产物；
- 第 2 级验收通过，开始第 3 级 HTTP/API 纵向流程。
