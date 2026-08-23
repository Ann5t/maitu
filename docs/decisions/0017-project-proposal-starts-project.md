# 0017：接受 ProjectProposal 直接启动项目

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

现有实现批准 ProjectProposal 后创建 Project 和一个根 BranchProposal 草稿，用户进入项目后还要再次批准根目标才真正开始执行。这把一次“授权正式投入工作”拆成两次近似决定，并产生一个没有活动根目标的项目空壳。

保留 ProjectProposal 仍有必要：它让用户在随口想法变成持续消耗 Agent、存储和工具资源的项目之前，确认 AI 对目标、边界、未知和工作方式的理解。项目是否能够开始不取决于想法是否已经成熟；不确定性本身可以成为探索型项目的工作目标。

## 备选方案

- ProjectProposal 只创建空项目，根 BranchProposal 再次批准后开工。
- 删除 ProjectProposal，任何想法都能直接启动 Agent。
- 保留 ProjectProposal，但接受当前版本后直接创建并启动完整项目。

## 决定

用户在原想法对话中接受准确的当前 ProjectProposal revision，即授权系统正式开始该项目。系统在一次可恢复的业务操作中建立：

- Project 身份及准确 Idea/Proposal 来源；
- 项目独立 Git 仓库、默认 branch 与 Git LFS 配置边界；
- 映射默认 branch 的根 GoalBranch；
- 由获批 Proposal 冻结得到的目标说明 v1；
- 首个持久 Agent Session 及其调度请求。

不创建根 BranchProposal，也不要求第二次根目标批准。用户还不想正式投入工作时，应留在原想法对话继续修改 ProjectProposal，而不是先接受并创建空项目。

项目可以是交付型、探索型或混合型。探索型 Proposal 只需清楚说明准备减少什么不确定性、怎样留下证据、何时停止或请求用户判断，不承诺尚不可能定义的最终产品。

跨 PostgreSQL、Git 仓库和调度器的创建必须具有幂等操作身份、阶段记录与恢复逻辑。只有持久项目、仓库、根枝干、目标说明和首个 Session 均可恢复时才向用户显示成功；Runner 暂时不可用时，首个 Session 保持可见的排队或暂停状态，不重新请求项目授权。

## 影响

- 项目从接受提案开始就有根目标和工作现场，不存在正常流程中的空项目。
- 一次人工决定同时固定项目来源和最初执行边界。
- 想法仍保留并可以继续演化，同一想法以后仍可产生其他 ProjectProposal。
- 当前“批准 ProjectProposal 后生成根 BranchProposal 草稿”的事务、页面、测试和 API 是待迁移实现；旧项目与来源记录继续可读，已发布迁移不改写。

## 相关资料

- [Proposal 是原对话中的版本化提案卡](0016-inline-versioned-proposal-conversation.md)
- [每个项目使用独立 Git 仓库，不设总 Git](0010-one-repository-per-project.md)
- [目标说明是 GoalBranch 的内部版本](0013-goal-definition-is-branch-version.md)
- [产品设计：顶层信息架构](../product/product-design.md#3-顶层信息架构)
