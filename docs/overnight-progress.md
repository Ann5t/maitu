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
| 3. HTTP/API 纵向流程 | 已完成 | `api-goal-branch-v1.md`；隔离真实 HTTP 闭环与 HTML 表单适配测试通过 |
| 4. 插件与环境版本基础 | 已完成 | `0003_tooling_core.sql`、`tooling-api-v1.md`；隔离版本/环境/Mock Broker HTTP 测试通过 |
| 5. 安全文件导入 | 已完成 | `0004_input_artifacts.sql`、`input-api-v1.md`；隔离分段/哈希/导入/冻结 HTTP 验收通过 |
| 6. 项目图与工作台原型 | 已完成 | `goal-lanes-v1` 可替换投影；结构化 HTML 与 Chromium 1440px/390px 全流程通过 |
| 7. 质量门与早晨交付 | 已完成 | 本地/CI 共用完整质量门通过；生产镜像隔离验收通过；真实现场前后指纹一致；最终报告已生成 |

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

### 2026-08-22 01:57 +08:00

- 新增事务化目标枝干应用服务与 `goal-branch-v1` 快照，JSON API 和普通 HTML 表单共用同一命令入口；
- 实现 Proposal 草拟、修订、提交、取消、批准，以及批准后原子创建 GoalBranch、契约 v1 和首个 Session；
- 实现拟子枝干、判断请求、异常/手动暂停、显式恢复、Contribution、冻结拟合并、独立 AI 审核、用户接受/部分接受/退回/放弃和下一 Session；
- 所有写命令锁定项目、验证稳定输入、写不可变事件，并用 `clientRequestId + canonical input hash` 返回原结果或拒绝语义漂移；
- `scripts/test-goal-http.sh` 在临时网络、数据库和服务中通过完整流程：根 Proposal 修订 → 子枝干 → 拟合并 → 退回 → 幂等重放/冲突 → 下一 Session → 接受 → 父 Session 恢复 → 根目标完成；
- 中途快照证明未接受时父枝干仍 `waiting` 且没有 Integration；最终快照证明子枝干 `integrated`、根枝干/项目 `completed`，物理 Git 集成诚实记录 `not_attempted`；
- HTML 表单适配入口以 303 回跳且产生同一 Proposal；空库/旧基线迁移、fmt、Clippy、16 个单元测试继续通过；
- 所有隔离容器和网络已清理；真实库计数仍为 3/8/23/2；
- 第 3 级验收通过，开始第 4 级中央插件与环境版本基础。

### 2026-08-22 02:14 +08:00

- 新增不可变插件包、EnvironmentManifest、Session 环境 binding、ToolCall 审计与 ToolLease 状态表；迁移在空库、重放和旧基线场景通过；
- 实现 Manifest 规范化/服务器封装、SemVer 目录与 `latest` 解析、同版本摘要冲突拒绝，以及按需目录/详情接口；
- 实现确定性环境规范化与 SHA-256 指纹；插件、工具链、lock、目标、Feature、参数或策略变化都会形成新环境；
- Session 首次环境绑定不可变；子目标首个 Session 与退回后的下一 Session 持久继承准确 binding，不共享可变环境；
- 实现 ToolBroker trait、ToolCall/Result 和 ToolLease 状态机，以及无文件、网络副作用的 `echo`/`inspect` 参考 Mock 插件；
- `scripts/test-tooling-http.sh` 验证同名 1.0/2.0/3.0 共存、`latest` 解析后不漂移、冲突环境并行、重绑拒绝、错误插件拒绝、调用幂等、结果绑定与不可变插件审计；
- 目标枝干 HTTP 回归继续通过；Rust 单元测试增至 21 个并全部通过，fmt/Clippy 无警告；
- 所有隔离容器和网络已清理；真实库计数仍为 3/8/23/2；
- 第 4 级验收通过，开始第 5 级安全文件导入。

### 2026-08-22 02:35 +08:00

- 新增 `InputArtifact` 与不可变分段迁移；上传记录绑定 project/GoalBranch/Session，客户文件名只作显示，存储键由服务器生成；
- 实现受限且可乱序的分段上传、重叠/缺口检查、分段与完整 SHA-256 复核、内容寻址去重、可信 media type 检测和带 ETag/`nosniff` 的下载；
- 实现安全 Session inbox：小型 UTF-8 文本显式复制，二进制/大文件保留为 Artifact 引用，同名不覆盖，绝对路径、`..`、Windows 保留名与空路径段被拒绝；
- ZIP/gzip 归档只存储不解压；每个输入阶段写入不可变 GoalEvent，摘要不符保留 `rejected` 审计记录；
- 拟合并后的 `frozen_candidate` Session 不得开始、追加、完成或导入新文件，但已完成请求仍可幂等重放；
- `scripts/test-inputs-http.sh` 在临时网络/数据库/对象目录中验证 6 个逻辑输入、7 个分段、3 个内容对象，以及去重、路径穿越、超限、同名、无效 Session 与冻结拒绝；
- 空库/旧基线迁移、目标枝干 HTTP、插件环境 HTTP 全部回归通过；25 个 Rust 单元测试全部通过，fmt/Clippy 无警告；
- 隔离容器与网络已清理；真实库计数仍为 3/8/23/2；
- 第 5 级验收通过，开始第 6 级项目图与工作台原型。

### 2026-08-22 03:04 +08:00

- 项目默认入口切换为新目标枝干工作台，旧 DAG 保留为独立“探索图”页签，两套模型不自动重解释；
- 新增只读 `goal-lanes-v1` 投影：枝干是目标车道，圆点卡片是 Agent Session，`HEAD` 表示当前 Session；投影版本不写回领域数据；
- 点击 Session 可看到契约/未知、文件与产物、环境指纹、工具/浏览器/测试证据、Contribution、暂停上下文和审核决定；
- 增加结构化 HTML 表单，无需手写 JSON 即可推进 Proposal 草拟/修订/批准、Contribution、子目标、判断请求、异常/手动暂停、恢复、拟合并、独立 AI 建议和用户最终决定；
- 新增浏览器文件渐进增强：选择文件后按受限分段上传、完整哈希验证并导入 Session；服务器端仍是完整性权威；
- `scripts/test-workbench-http.sh` 用真实 HTML 表单跑通 Proposal → 判断暂停/恢复 → Contribution → 拟合并 → AI 建议 → 用户接受；
- `scripts/test-workbench-browser.sh` 用固定官方 Playwright 1.62.0 容器中的 Chromium 真实点击同一闭环和文件上传；1440px 与 390px 都无页面级横向溢出；
- 保存桌面/手机截图和 `workbench-projection-v1.md`，并明确它们是待用户凭感觉继续修改的原型，不是最终 UI 定案；
- Rust 单元测试增至 29 个并全部通过，fmt/Clippy 无警告；所有浏览器隔离容器/网络已清理；
- 真实库计数仍为 3/8/23/2，原预览仍只绑定 `127.0.0.1:3001`；
- 第 6 级验收通过，开始第 7 级最终质量门与早晨交付。

### 2026-08-22 03:15 +08:00

- 新增只读权限的 GitHub Actions 模板，直接运行本地同一 `scripts/quality-gate.sh`，不发布、不部署、不持久化 Git 凭据；
- 完整质量门一次通过：rustfmt、Clippy、29 个 Rust 测试、迁移、4 套隔离 HTTP、Chromium 和 production runtime；
- production runtime 以 UID 1000、只读根文件系统、一次性 PostgreSQL 和随机 `127.0.0.1` 端口完成 4 个迁移、健康及隔离写入验收；
- Compose 的未来默认 PostgreSQL 映射收紧为 `127.0.0.1`；当前旧容器的既有 `0.0.0.0:55432` 映射未擅自重建，并记录为醒后安全事项；
- 真实数据库前后保持 `3:8:23:2`，迁移登记未增加；产物内容/路径指纹前后均为 `f0f167bf097e0bc6d8f7c821b6af98eb065acd0ab0281462c07610a65e5aff0e`；
- 原应用和数据库容器 ID/启动时间未变，所有一次性测试容器和网络均已清理；
- 最新备份与恢复归档 SHA-256 通过，仓库无 remote、无常见凭据模式、无超过 5 MiB 的待跟踪文件；
- 生成 `overnight-report.md`，列明实现、证据、未完成事项、风险和建议下一 Goal；
- 第 7 级及本阶段全部最终验收通过。
