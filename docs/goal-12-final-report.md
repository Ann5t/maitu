# Fudian 第一阶段 12 方面 Goal：拟合并审查报告

- Goal：`01a024d6-1224-7fe1-a7f2-ea8dafd226e4`
- 工作枝干：`feat/goal-branch-core-v0.1`
- 产品语义基线：[`product-design.md`](product-design.md)
- 唯一验收矩阵：[`design-12-aspects.md`](design-12-aspects.md)
- 结论：12 个方面均已形成可运行候选并通过各自工程机器门；7 项已有用户语义确认并完成，5 项诚实停在用户主观/真实部署判断点。当前是拟合并，不是已合并，也不是 Goal 自动完成。

## 12 项逐条结论

| # | 方面 | 结论 | 关键实现与证据 |
| --- | --- | --- | --- |
| 1 | 想法 / 项目两级空间 | 待用户判断 | 不可变 Idea/来源/关系、多来源 ProjectProposal、原子立项、时间流/关系图及三尺寸闭环通过；默认投影和手感待定 |
| 2 | 核心领域模型 | 完成 | Project/GoalBranch/Session/Proposal/Contract/Contribution/Evidence/Review/Event、数据库约束、并发幂等和真实 Git 身份通过 |
| 3 | 目标契约与探索型目标 | 完成 | 最低充分表单、诚实未知、delivery/exploration/hybrid、版本差异/来源、判断暂停和不可降标通过 |
| 4 | 执行、暂停、拆分、拟合并与回流 | 完成 | 持久状态机、Attention、冻结候选、独立审核、选择性父整合、冲突/CAS/崩溃恢复和用户唯一终结权通过 |
| 5 | 上下文、记忆和渐进披露 | 完成 | 精确父快照、不可折叠契约/权限信封、完整来源目录、固定窗口读取审计和可重建派生索引通过 |
| 6 | 单写者、worktree、权限和 Runner 隔离 | 完成 | 真实 bare repo/worktree、单写 Lease/fencing、断网无能力 Runner、资源/路径/符号链接/删除边界和 Git CAS 通过 |
| 7 | 中央插件与版本环境 | 待用户判断 | Ed25519 签名 OCI、摘要绑定 Skill/reference、Session 级渐进披露、冲突 Python、Rust/C/C++/Playwright、PPTMaster 请求、ToolLease、保留输出和同源受控 endpoint 通过；发现/运行现场手感待定 |
| 8 | PostgreSQL、Git、文件、Artifact 与恢复 | 待用户判断 | 内容寻址输入、来源与哈希、跨存储日志/CAS、稳定 reconcile、可逆 quarantine、v2 备份、空目标恢复、升级/回退通过；默认复制/引用体验待定 |
| 9 | 持久调度与恢复 | 完成 | PostgreSQL ActionRun、Worker/Action Lease、心跳/fencing、安全重排、人工暂停、通知/outbox、取消竞态和服务/launcher 崩溃恢复通过 |
| 10 | 测试、证据、独立 AI 审核与用户验收 | 完成 | 准确 commit/环境/产物绑定、独立只读 Review Worker、父契约 Integration Worker、反例/冲突和用户唯一决定权通过 |
| 11 | Git 图 + 工作现场多设备前端 | 待用户判断 | `goal-worksite-v2`、真实文件/Git/Runner/工具/插件/审核现场、1440/820/390、字号/对比/触控/键盘及 100/300 大图预算通过；整体层级和密度待用户使用判断 |
| 12 | 私有部署、安全与运维 | 待用户判断 | 单 owner、Argon2id/恢复码/会话、CSRF/Origin、持久限速、可信代理、HTTPS、安全 Compose、备份恢复与应用回退通过；真实访问方案和维护窗口待定 |

## 最终机器证据

BP-10 后首次总门只在入口发现一处 rustfmt 折行差异；格式化后第二次从起点完整通过 `scripts/quality-gate.sh`，覆盖：

- rustfmt、Clippy `--all-targets --all-features -D warnings`；
- 57 个 Rust 测试，14 条应用迁移，空库、重复迁移和旧 DAG fixture；
- 目标/契约、上下文、想法立项、输入、插件、真实 Git/Runner、调度恢复、独立审核/集成的隔离 HTTP；
- Rust、C/C++、双冲突 Python、Playwright/Chromium 签名 OCI，5 个不可变内容包/10 项 Skill+reference、Session Skill 引导、按需读取/审计，以及持续 ToolLease；
- 1440/820/390 Chromium 与 100 GoalBranch / 300 Session / 20 Proposal 大图；本次大图 TTFB 中位 `86.227ms`、DOMContentLoaded `241.9ms`、筛选 `18.3ms`；
- UID 1000、只读根的发行镜像及 14 条迁移；
- 四类存储稳定扫描、missing 半状态、可逆 quarantine 和哈希一致恢复；
- HTTPS setup/login/session/恢复码/CSRF/Origin/持久限速/ToolLease 代理与三尺寸浏览器；
- BP-08 的 12 迁移备份 → 带标签空目标恢复 → 当前 14 迁移升级 → BP-08 应用回退；
- 真实安全 Compose 的文件 secret、内部网络、唯一 HTTPS、无应用/数据库端口、非 root/只读根、资源/PID/日志上限。

三尺寸安全现场见 [`private-desktop.png`](screenshots/private-desktop.png)、[`private-tablet.png`](screenshots/private-tablet.png) 和 [`private-mobile.png`](screenshots/private-mobile.png)。

## 真实现场保护

最终审计与 Goal 启动基线一致：

| 项目 | 结果 |
| --- | --- |
| 生产应用 | 完整 ID `bebb50d02f5ff163638f6dd38a5fe40140050ddb36f76b66c79bc3e17fe6d624`；启动时间 `2026-08-21T16:09:35.31506644Z`；仍只绑定 `127.0.0.1:3001` |
| 生产 PostgreSQL | 完整 ID `482ccdee9d25a2c2f34c26aee365353e0b77fd827bdb05017793c24280db371f`；启动时间 `2026-08-21T07:44:25.221155924Z` |
| 业务计数 | `3:8:23:2`（项目/旧枝干/旧节点/Artifact） |
| 迁移 | 仍只有原 4 条：`0001_initial`、`0001_rust_baseline`、`0002_project_evolution_graph`、`0003_selective_branch_merges` |
| Artifact 1 | 数据库与文件均为 `f6bc647c9f81180c15c624a6ae1cc8eb49f11faf075ebcaa37304391796ce935` |
| Artifact 2 | 数据库与文件均为 `203895184dfbcd40d7f1117daedfe782722f7155d50df96dfbae130ebdbf74b2` |
| 测试残留 | 无 BP-09/BP-10/总门一次性容器、网络或卷；中央 Cargo/工具镜像缓存按设计保留 |

两个相同相对路径和文件摘要构成的原产物汇总指纹仍对应基线 `f0f167bf097e0bc6d8f7c821b6af98eb065acd0ab0281462c07610a65e5aff0e`。

## 不应隐藏的风险

1. 真实 PostgreSQL 仍是 Goal 开始前的 `0.0.0.0:55432` 公共映射。仓库候选已不公开数据库，但修改正在运行的真实容器需要经过复核的异地 v2 备份和用户授权维护窗口。
2. Docker 不会为只连接 internal 网络的容器建立宿主端口。因此 Caddy 独占一个非 internal ingress，同时通过 internal edge 访问应用；私有模板仍只绑定宿主回环，但 Caddy 网络本身不是“无出站”沙箱。
3. 当前是唯一 owner，不是团队 RBAC。不能在未重新设计授权模型时直接扩成多用户。
4. 本阶段完成 Agent Worker/Lease/审核协议和确定性夹具，但未使用私人凭据或付费 API 接入真实模型提供商。第一个真实 provider 应作为独立 Tool Pack/凭据/费用/数据外发审查。
5. 私人网络或公网域名、系统 CA 信任、DNS、防火墙、异地备份位置和通知适配器均需要真实环境信息，仓库没有擅自执行。

## 你不需要填表的最终判断

只需按真实感受回答或修改以下几件事，系统再把自然语言反馈落回契约：

1. 想法空间默认先看时间流、关系图，还是两者混合？
2. 文件拖入 Session 时，默认复制到 worktree/inbox，还是先作为只读 Artifact 引用？
3. 目标图、Session 现场和插件/浏览器入口的层级、字号、密度是否已经可用；哪里仍让你看不清或找不到？
4. 真实访问优先选私人网络 + 内部 CA，还是公网域名 + ACME？确认异地备份后，何时允许处理旧数据库 `55432`？

在这些判断前，工作 Agent 的结论只能是“候选工程完整，提出拟合并”。不得自行合并 `main`、push GitHub、部署服务器或标记总 Goal complete。
