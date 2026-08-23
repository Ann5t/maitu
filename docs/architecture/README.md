# 实现架构

> 状态：已实现快照。这里描述当前代码和测试证明的边界，不替代仍在讨论的产品设计。

- [总体架构](architecture.md)：部署单元、数据、一致性、安全和测试边界
- [目标枝干领域](goal-branch-domain.md)：聚合、状态机和全局不变量
- [目标契约与生命周期](core-domain-v2.md)：探索、Evidence、候选和终态
- [想法与项目](idea-project-domain-v1.md)：Idea、ProjectProposal 和来源
- [上下文与记忆](context-memory-v1.md)：继承、目录、渐进式披露和审计

具体接口字段放在[接口参考](../reference/README.md)。已完成 BranchProposal 的目标契约和验收证据保存在[历史归档](../archive/README.md)，避免把过去的实施计划误当作现行架构。

当前实现及本目录仍使用 `GoalContractVersion` 名称；目标产品已经决定将其视为 GoalBranch 内部的“目标说明版本”，迁移边界见[决策 0013](../decisions/0013-goal-definition-is-branch-version.md)。

当前实现把 ReviewGate、ReviewDecision、Integration 和对应 ActionRun 分开持久化；目标产品把它们视为同一 MergeGate 的内部记录，见[决策 0014](../decisions/0014-single-merge-gate-aggregate.md)。

当前实现使用独立 `goal_attention_items` 保存待处理状态；目标产品改为直接汇总 Session、目标草稿、MergeGate 和工具请求自身的等待状态，见[决策 0015](../decisions/0015-attention-is-derived-view.md)。

当前实现仍有独立的 Proposal 提交、审批路由和视图；目标交互改为在来源对话中持续修订并接受准确版本，见[决策 0016](../decisions/0016-inline-versioned-proposal-conversation.md)。

当前 ProjectProposal 获批后只创建 Project 和根 BranchProposal 草稿；目标产品改为一次授权直接创建项目仓库、根 GoalBranch、目标说明 v1 和首个 Session，见[决策 0017](../decisions/0017-project-proposal-starts-project.md)。

当前恢复能力以整实例迁移和跨存储调和为主；目标产品还要求把单个项目的 Git/Git LFS、完整 Session 对话和全部工作流状态导出成一个可校验文件，并可导入新实例，见[决策 0018](../decisions/0018-complete-project-export.md)。

本目录中的现有 schema 和兼容路径尚未成为正式测试基线。用户决定不保留或转换这些开发样本；目标领域模型审定后将干净重建实现，正式测试开始后才承担前向迁移和数据保留义务，见[决策 0019](../decisions/0019-pretest-clean-domain-rebuild.md)。

目标产品允许新建仓库或接入已有 GitHub 仓库，不把 Fudian 内部状态写进项目目录，并默认自动推送用户已接受的默认分支更新，见[决策 0020](../decisions/0020-clean-project-repository-and-github-sync.md)。

第一版的项目 GitHub 远端采用单写模式：只允许当前 Fudian 实例更新 branch，push 前验证远端未发生意外变化，见[决策 0021](../decisions/0021-fudian-is-sole-github-writer.md)。

目标 MergeGate 首层只显示简短判断，Evidence 由工具链自动采集并按需展开；用户可以例外接受普通质量缺口，但不能绕过候选身份与内容完整性，见[决策 0022](../decisions/0022-concise-merge-evidence-and-user-waiver.md)。

目标 Session 默认继承精简但不可丢失的上下文，能看到项目地图的被动摘要，并在安全动作边界接收用户消息或形成暂停检查点，见[决策 0023](../decisions/0023-session-context-and-safe-message-delivery.md)。

目标工具系统在统一目录下区分插件能力与环境资源，按内容复用不可变工具和包，为每个 Session 隔离环境并允许短期热 Worker，见[决策 0024](../decisions/0024-unified-tools-with-isolated-reused-environments.md)。

目标插件由管理员安装，获批 Session 可直接使用固定版本；升级不影响运行中环境，详细调用和版本占用只按需显示，见[决策 0025](../decisions/0025-admin-plugin-install-and-pinned-session-tools.md)。

目标工具目录中的能力由共享环境、调用适配器和 Agent 说明组合；MCP、MCPB、OCI 只是统一 Tool Broker 合同的兼容方式，见[决策 0026](../decisions/0026-composable-tool-capability-model.md)。

Fudian 原生插件最少提供 `plugin.toml` 和隔离自检，Skill、资料、Assets、MCP 与专用 Runtime 按复杂度选择，见[决策 0027](../decisions/0027-minimal-native-plugin-layout.md)。

目标插件只有在受控开发沙箱通过安装后才能供 Session 使用；第一版无公共市场，网络、秘密和 worktree 输入输出都由 Broker 最小授权，见[决策 0028](../decisions/0028-controlled-plugin-development-and-sandbox.md)。

目标调度没有单独过夜模式：获批 Session 在服务器持续运行，等待只阻塞来源枝干，其他合格叶子继续；新分支仍必须人工批准，见[决策 0029](../decisions/0029-continuous-server-execution-with-localized-blocking.md)。

目标调度器按服务器和 Provider 资源限制并行运行 Session，其余工作持久排队；队列默认按批准时间并允许用户提升优先级，所有项目共同受实例级 AI 用量提醒与硬上限约束，见[决策 0030](../decisions/0030-resource-aware-session-scheduling.md)。

目标 AI 设置把 API 或账号登录等连接方式与模型用途分开；模型可勾选七类媒体能力，默认文字模型在同一 Session 内按需调用其他已配置能力，见[决策 0031](../decisions/0031-capability-based-ai-configuration.md)。

目标产品由首个注册账号成为实例管理员，后续账号由管理员创建；想法按账号隔离，项目成员分负责人和参与者，管理员介入项目必须明确接管并审计，见[决策 0032](../decisions/0032-admin-created-accounts-and-project-roles.md)。

目标想法空间只保留“刚记下、在讨论、待立项”三阶段看板；ProjectProposal 在待立项阶段确认转化，接受后来源进入历史而项目直接启动，见[决策 0033](../decisions/0033-three-stage-idea-board.md)。

目标产品在所有普通文字入口复用服务器侧腾讯语音识别，转写只形成可编辑草稿且默认删除临时录音；它与 Agent 的音频理解能力相互独立，见[决策 0034](../decisions/0034-system-wide-tencent-speech-input.md)。
