# 0014：一次拟合并只对应一个 MergeGate

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

现有实现和文档使用 MergeCandidate、ReviewGate、ReviewDecision、Integration 与 ActionRun 等多个对象描述一次枝干回流。它们分别保存冻结现场、审核决定和物理 Git 操作，但对用户而言共同构成图上的一个菱形关口。把内部阶段并列暴露会增加理解成本，也容易产生“先批准源枝干、合并后是否还要再次批准”的模糊流程。

用户已经确定：拟合并只是工作 Agent 声称目标达成；最终候选必须经过隔离整合、测试和独立 AI 审核，用户可以在决定前继续追问，只有用户接受后父枝干才真正更新。

## 备选方案

- 候选、审核、决定和 Integration 继续作为多个同级领域对象和图节点。
- 源枝干先人工批准，物理整合后自动发布。
- 一次拟合并创建一个 MergeGate，所有尝试和决定都归入 Gate。

## 决定

工作 Agent 每次提出拟合并时创建一个 `MergeGate`。它是用户可见的唯一合并关口和图上唯一菱形对象，持有冻结源 HEAD、当前目标说明版本、环境、Evidence、候选成果摘要及状态。

子枝干的 Gate 先从当时父安全 HEAD 创建隔离 worktree，执行完整 `git merge --no-ff`，处理冲突并构建测试；根枝干没有父级整合步骤。只读的独立审核 AI 检查最终候选后，Gate 才进入等待用户决定。

无法在不作主观选择的情况下形成整合候选时，Gate 可以先请求用户判断；该判断只指导当前整合尝试，不等于最终批准。用户等待最终决定期间可以与该枝干最后一个 Session 的 Agent 及审核 AI 对话以澄清信息。对话本身不修改冻结候选；需要改文件或目标说明时，必须退回枝干继续工作。

用户接受子枝干候选后，系统以 compare-and-swap 发布已经审核的 merge commit；父 HEAD 漂移时，本次整合尝试失效，在同一 Gate 内重新形成、测试和审核候选，并再次等待用户决定。根枝干被接受时直接完成项目，不创建虚假的父级 merge。

MergeAttempt、冲突选择、AIReview、HumanDecision、ActionRun 和发布结果可以作为不可变子记录实现，但不是独立用户概念或主要图节点。退回后原 Gate 进入终态；后续 Session 再次拟合并会创建新的 Gate。

## 影响

- 用户只需理解一个菱形及其当前阶段，可以展开查看完整内部历史。
- 不存在“批准尚未整合的源枝干后自动发布一个不同结果”的授权缺口。
- 部分接受仍只记录反馈并退回，不发布摘要子集或部分 Git 内容。
- 当前 `goal_review_gates`、`goal_review_decisions`、`goal_integrations`、ActionRun 和对应 API 可以继续作为兼容存储，但必须由一个 Gate 身份聚合；迁移不能改写旧审核记录。
- 当前先人工决定、再创建 Integration 的实现顺序需要迁移到先形成最终整合候选、后人工批准。

## 相关资料

- [在菱形 Merge Gate 中由 AI 处理整合](0008-ai-assisted-merge-gate.md)
- [成果摘要属于拟合并候选](0012-merge-candidate-outcome-summary.md)
- [产品设计：MergeGate](../product/product-design.md#46-mergegate)
