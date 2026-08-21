# Fudian 目标枝干核心 v0.1：执行报告

- 执行窗口：2026-08-22 01:20–03:15（Asia/Shanghai）
- Goal：`01a024d6-1224-7fe1-a7f2-ea8dafd226e4`
- 枝干：`feat/goal-branch-core-v0.1`
- 产品语义基线：[`product-design.md`](product-design.md)
- 计划与逐步证据：[`overnight-plan.md`](overnight-plan.md)、[`overnight-progress.md`](overnight-progress.md)

## 结论

有限目标“目标枝干核心 v0.1”已全部实现并通过本地完整质量门。当前纵向版本能够从 BranchProposal 开始，经用户批准创建一目标一枝干及首个 Agent Session，处理子目标、主观判断、异常/手动暂停、显式恢复、Contribution、冻结拟合并、独立 AI 建议、用户接受/部分接受/退回及下一 Session。

插件环境和文件输入也进入同一审计链：Session 固定不可变 EnvironmentManifest，插件同名多版本可并存，Mock Broker 证明无状态调用与结果绑定；文件支持受限分段、完整 SHA-256、内容寻址、安全 inbox/Artifact 引用和候选冻结。默认项目入口已有桌面与手机可操作的 Git 车道 + 工作现场原型，但它仍明确是可替换投影，不是最终 UI 定案。

没有推送 GitHub、没有公开部署、没有使用私人凭据或付费 API，也没有把新增迁移应用到真实数据库。

## 可审查提交

| 提交 | 内容 |
| --- | --- |
| `60b6cad` | 恢复后的 Rust 可构建基线 |
| `b923bd5` | 安全盘点、Git 枝干与执行基线 |
| `26edef2` | 目标枝干及中央工具协议 |
| `2dc263a` | GoalBranch 数据迁移、状态机与约束 |
| `9ffb0f1` | Proposal → Session → Review 的 HTTP/HTML 闭环 |
| `99778db` | 固定插件版本、EnvironmentManifest 与 Mock Broker |
| `8182797` | 安全分段文件输入、内容寻址与 Session 导入 |
| `aa00c4f` | 响应式目标枝干工作台与 Chromium 流程 |
| `6f22d67` | CI 模板、完整隔离质量门与生产镜像验收 |

## 验证结果

`./scripts/quality-gate.sh` 在提交 `6f22d67` 前从头运行并通过：

- rustfmt 通过；
- Clippy `--all-targets --all-features -D warnings` 通过；
- 29 个 Rust 测试通过，0 失败；
- 空库迁移、SQL 重放和旧 DAG fixture 增量迁移通过，旧计数保持 `1:1:1`；
- 目标枝干 HTTP：Proposal → 子枝干 → 退回 → 下一 Session → 接受 → 根目标完成；
- 工具 HTTP：同名多版本、固定 `latest`、冲突环境隔离、Mock Broker 审计；
- 输入 HTTP：6 个逻辑输入、7 个不可变分段、3 个内容对象，冻结候选拒绝写入；
- 结构化 HTML：Proposal → 判断暂停/恢复 → Contribution → AI 建议 → 用户接受；
- Chromium：1440px 与 390px、文件上传和完整审核闭环通过，无页面级横向溢出；
- runtime 镜像：UID 1000、只读根文件系统、随机回环端口、4 个迁移、健康检查和隔离写入通过。

当前视觉证据见 [`workbench-projection-v1.md`](workbench-projection-v1.md)、[`goal-workbench-desktop.png`](screenshots/goal-workbench-desktop.png) 和 [`goal-workbench-mobile.png`](screenshots/goal-workbench-mobile.png)。GitHub Actions 模板直接运行同一质量入口，但由于本阶段没有 remote，它尚未在 GitHub 托管 Runner 上实际触发。

## 真实现场保护证据

质量门前后结果完全一致：

| 项目 | 前 | 后 |
| --- | --- | --- |
| 项目/旧枝干/旧节点/产物计数 | `3:8:23:2` | `3:8:23:2` |
| 真实库迁移登记 | 4 条旧登记，无 `0002_goal_branch_core` 等新迁移 | 相同 |
| 产物路径与文件内容指纹 | `f0f167bf097e0bc6d8f7c821b6af98eb065acd0ab0281462c07610a65e5aff0e` | 相同 |
| 原预览容器 | `bebb50d…`，启动于 `2026-08-21T16:09:35Z` | ID 与启动时间相同 |
| 原 PostgreSQL 容器 | `482ccdee…`，启动于 `2026-08-21T07:44:25Z` | ID 与启动时间相同 |

最终只运行原来的应用与 PostgreSQL 两个容器；所有 `goal/tooling/input/workbench/browser/runtime` 测试容器和网络都已清理。最新备份 `backups/20260821T160940Z` 的数据库、产物、源码三项校验通过，恢复取证归档也校验通过。仓库没有 remote，没有检测到常见凭据模式，也没有超过 5 MiB 的待跟踪文件。

## 仍未完成与风险

1. **不要公开部署。** 身份认证、授权、CSRF、速率限制和可信反向代理边界尚未实现。
2. 当前正在运行的旧 PostgreSQL 容器仍把 `55432` 映射到所有宿主机接口。这是本阶段开始前就存在的状态；仓库已把未来 Compose 默认值改为 `127.0.0.1`，但为避免中断和触碰真实数据，没有擅自重建现有容器。应在完成新备份后的维护窗口处理。
3. Tool Broker 当前只有确定性 Mock 插件；真实 C/C++、Python、Rust、Playwright、PPTMaster Runner、租约续期、沙箱和远程执行尚未实现。
4. 领域模型记录了贡献集成与物理 Git 状态，但尚未真正创建 worktree、执行合并或处理 Git 冲突。
5. 工作台是可用原型，信息密度、车道布局、暂停/合并的视觉表达仍应由用户实际使用后的感觉推动迭代。
6. 当前只有“项目”一级；“想法”一级空间是明确非目标。文件与数据库写入也仍缺少孤儿对象后台回收。
7. `Cargo.toml` 声明最低 Rust 1.94，本轮完整门固定在 Rust 1.97；最低版本尚未作为独立 CI 矩阵验证。

## 建议下一次 BranchProposal

醒来后的第一个动作应是审查桌面/手机截图和真实工作台手感，并只修改感到不对的投影层，不重写已验证领域语义。

随后建议优先形成“单用户私有多设备访问 v0.1”BranchProposal：登录与会话、CSRF、速率限制、反向代理/TLS、数据库不公开、备份后切换与回退。完成这条安全前置枝干后，再开启“真实 Git worktree Agent Runner v0.1”，把现有 GoalBranch/Session/EnvironmentManifest 协议连接到真正的分支工作目录和中央插件执行器。
