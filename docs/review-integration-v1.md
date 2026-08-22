# BP-07：冻结候选、独立审核与真实父枝干集成目标契约

## 目标

把拟合并从“数据库里记录一个建议”补成真实闭环：工作 Agent 冻结准确现场；独立 Review Worker 在只读候选上找遗漏、复跑测试并提交绑定报告；用户最终决定；被接受的 Contribution 在隔离集成现场形成候选，经父目标验证后以 Git compare-and-swap 写入父枝干。任何一步未完成，都不能把 `not_attempted` 冒充成 `integrated`。

## 不变量

- `merge.propose` 只允许当前 `running` Session 使用其干净、无写 Lease、数据库与磁盘一致的 worktree。服务端绑定准确 workspace、HEAD、tree、snapshot、契约和环境，再计算完整候选摘要；客户端文字不能替代物理状态。
- 冻结后源 worktree 不再接受 RunnerJob 或 ToolLease 写入。撤回或退回只解冻到同一枝干的下一 Session，不覆盖旧 ReviewGate。
- 提案自动创建唯一、持久的 `review` ActionRun。审核身份来自已注册 Worker 和有效 ActionLease；不能由表单伪造，不能与工作 Agent 身份相同，旧 fencing token 不能迟到写回。
- Review Worker 只读准确候选，报告必须绑定候选摘要、HEAD、tree、环境、逐条契约检查、反例和复验。观察值或摘要不匹配时，审核不进入用户阶段。
- AI 审核只是建议。只有用户可以接受、部分接受、退回或放弃；未经用户接受，不创建集成候选、不移动父 Git ref，也不把选中内容加入父上下文。
- 对有父枝干的接受/部分接受，人工决定只创建 `pending` Integration 和唯一持久 `integration` ActionRun。此时源 Session、源 GoalBranch 仍保持等待审核终结，不能提前写成 `accepted` / `integrated`。
- 代码回流以选中 `code_change` Contribution 绑定的成功 RunnerJob commit 为最小边界，按原提交顺序 cherry-pick；非代码 Contribution 只进入可追溯上下文。候选存在未绑定代码 commit、重复/非祖先 commit 或无法选择性应用时必须暂停，不得静默做整枝干 merge。
- 集成先在服务器托管根内的临时 detached worktree 形成候选；父 worktree 保持不变。独立 Integration Worker 只读该候选并按父契约执行回归验证。
- 最终发布必须同时核对源候选、父基线、ActionLease fencing 和验证报告，并用 `git update-ref <candidate> <expected-parent>` CAS。父 ref 已移动或父 worktree 漂移时不覆盖胜者，Integration 进入冲突暂停。
- CAS 后如果服务在数据库确认前崩溃，重放会识别“ref 已是候选”并完成同一操作；若现场既不等于基线也不等于候选，则停止并请求人工处理。
- 只有 CAS、父 worktree 更新、父 workspace snapshot、选中 Contribution 上下文和审计记录全部确认后，源枝干才成为 `integrated`（部分接受则成为带明确原因的 `stopped`），父 Session 获得新上下文并恢复推进。子枝干通过不等于父目标通过。
- 根枝干没有父集成。它仍需独立审核和用户最终接受；用户接受后才可把 Project/根 GoalBranch 标为完成。
- 不触碰远程仓库、GitHub、生产数据库、生产 volume、真实账号或公开部署；所有验证使用一次性数据库、托管仓库、worktree 和 Worker。

## 最小交互

用户仍只处理一个拟合并判断：看目标摘要、差异、产物、风险和独立审核意见，然后选择接受、部分接受、退回或放弃。ActionRun、Worker 身份、commit 清单、fencing、候选摘要和集成操作默认折叠，发生冲突或用户按需查看时再展开。

用户接受后，系统可以在既定权限内自动准备和验证集成；成功后父 Agent 继续。出现 Git 冲突、父现场漂移、测试失败、未绑定变更或恢复状态不唯一时主动暂停并通知用户，说明最后安全点、已尝试操作、风险和建议。

## 验收表

- [x] `merge.propose` 冻结真实 workspace/HEAD/tree/snapshot/环境，篡改请求、脏现场、active Lease 和候选后漂移均被拒绝。
- [x] 自动产生 Review ActionRun；同身份 Worker、错误候选摘要、错误观察值和旧 fencing token 无法提交，独立 Worker 的真实只读复验可进入用户阶段。
- [x] 用户决定前父 Git、父文件和父上下文不变；人工接受后也不会提前把子枝干标为已集成。
- [x] 完整接受和部分接受都只集成选中 Contribution；代码 commit 有明确边界，未选代码不会借由祖先历史混入，非代码结果有来源边。
- [x] 隔离候选通过父契约回归后以 CAS 更新准确父 ref/worktree/snapshot；父 Session 获得新上下文并恢复，但父目标不会自动完成。
- [x] Git 内容冲突、父 ref 抢先移动、父 worktree 漂移、验证失败和服务中断均保持父安全点，生成去重 Attention/Notification，旧 Worker 不能覆盖处理结果。
- [x] CAS 后、数据库前的故障可幂等 reconcile；无法唯一判定的跨存储状态不会自动猜测。
- [x] 根目标只有独立审核后由用户确认完成，不创建虚假 Integration。
- [x] 空库、旧 fixture、迁移重放、SQL 负约束、Rust 单元、真实 HTTP/Git/Worker/重启流程和完整质量门通过。
- [x] 测试结束无残留容器、公开端口或临时 worktree；生产容器身份、数据计数和 Artifact 摘要只读复核不变。

## 本阶段不锁定

- Review AI 的具体模型供应商和提示词；协议固定独立身份、输入摘要和结构化报告，模型是可版本化 Worker 实现。
- 各技术栈的父目标验证命令如何自动推断；当前由目标契约/EnvironmentManifest 给出，无法安全解释时暂停而不是猜测。
- 冲突解决的可视化三方编辑器和审核摘要密度；BP-08 提供渐进披露界面，本阶段先保证底层事实完整。
