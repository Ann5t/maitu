# 想法空间与 ProjectProposal v1

## 目的

“想法”和“项目”是两个一级空间，不是同一张记录的两个状态。想法负责保存仍在发展、可能互相支持或矛盾的上游材料；项目负责围绕正式目标枝干执行。把想法转成项目时必须先形成可审查的 `ProjectProposal`，批准前不会出现正式项目。

用户不需要填一张预先完备的大表：记录想法只需要一句话，标题可以自动生成。只有准备立项时，界面才渐进披露“为何现在做、根目标结果、验证、停止、未知和暂不采用内容”。

## 权威对象

```text
Idea ── current_revision ──> IdeaRevision（不可变）
  │                              │
  ├── IdeaLink ──────────────────┤ 固定双方当时的版本
  │
  └── ProjectProposalRevisionIdea（角色 + 精确版本）
          │
          ▼
   ProjectProposal ── current_revision ──> ProjectProposalRevision（不可变）
          │                                  ├── root_goal
          │                                  ├── retained_notes
          │                                  └── omitted_notes
          │ 人工批准（单事务）
          ├────────> ProjectOrigin ────────> Project
          └────────> root BranchProposal 草案
```

- `IdeaRevision` 只追加，不覆盖；改变标题、正文或来源说明都产生新版本和理由。
- `IdeaLink` 记录 `related / supports / contradicts / depends_on / duplicates`，并固定关联建立时双方的具体版本。
- 一个 `ProjectProposalRevision` 可以引用多个想法；每个来源有 `source / supporting / constraint / omitted` 角色。
- `retainedNotes` 与 `omittedNotes` 明确保留“这次没采用什么”，不能因立项而把原想法删掉或改写成项目描述。
- `ProjectOrigin` 与 `ProjectOriginIdea` 是不可变来源账本。之后即使想法继续发展，已创建项目仍指向批准当时的版本。
- 批准 `ProjectProposal` 在一个 PostgreSQL 事务中创建旧项目兼容结构、正式来源账本和根 `BranchProposal` 草案。事务任一步失败时三者都不存在。
- 根 `BranchProposal` 仍是草案；批准立项不等于批准目标开始执行。用户可在项目空间继续修订、提交和批准根目标契约。

## 状态与权限

`Idea` 状态为：

- `captured`：刚记录；
- `developing`：已有后续版本；
- `proposed`：至少进入过一个立项提案；
- `promoted`：作为采用来源形成了项目；
- `archived`：用户明确归档，历史仍可读。

`ProjectProposal` 状态为：

```text
draft ──submit──> awaiting_approval ──approve──> approved
  ▲                    ├──reject───────────────> rejected
  └────revise──────────┘
  └/awaiting_approval──cancel──────────────────> cancelled
```

- 草案和待批准提案都可修订；修订会回到 `draft` 并追加版本。
- `submit` 会按“最低充分明确度”重新验证根目标：至少有完成/停止条件，并至少有验证方法或用户判断时机。未知可以非空，不会被伪造精度替代。
- 只有人工命令路径能够批准、退回或取消；认证接入后该要求还要由用户身份和 CSRF 共同强制。
- 所有写命令带 `clientRequestId`。相同对象、动作和输入重放返回原结果；同一键换对象或换输入返回 `409`。

## HTTP 与 HTML 投影

| 方法 | 地址 | 含义 |
| --- | --- | --- |
| `GET` | `/ideas?view=stream` | 时间流投影 |
| `GET` | `/ideas?view=map` | 关系投影 |
| `GET` | `/ideas/:id` | 想法版本、关系和立项提案工作区 |
| `POST` | `/api/v1/ideas` | 幂等 `idea.create` |
| `GET` | `/api/v1/ideas` | 想法摘要列表 |
| `GET` | `/api/v1/ideas/:id` | 完整来源快照 |
| `POST` | `/api/v1/ideas/:id/commands` | 修订、关联、归档或创建提案 |
| `POST` | `/api/v1/project-proposals/:id/commands` | 修订、提交、批准、退回或取消提案 |

时间流和关系图只是可替换的只读投影，选择哪种界面不会改变领域记录。桌面使用左右关系索引；820px 平板改为上下布局；390px 手机保持单列操作和固定底部导航。

## 已验证证据

- 32 个 Rust 单元测试通过，其中覆盖一句话自动标题、探索型未知和提案状态权限；
- `idea_project_constraints.sql` 验证不可变版本、自关联拒绝和不完整批准状态拒绝；
- `test-goal-migrations.sh` 在空库、迁移重放和带旧 DAG fixture 的库中验证 `0005` 只增不减；
- `test-ideas-http.sh` 在一次性数据库跑通创建、幂等重放/冲突、修订、旧版本冲突、关联、提交、批准、精确来源和根提案；
- Chromium 在 1440px、820px 和 390px 跑通真实表单闭环并断言无页面级横向溢出。

## 仍未关闭的边界

- v1 页面已支持文字和带来源引用的领域类型，但“直接上传文件、图片、语音并形成内容寻址 IdeaSource”的二进制入口尚未实现；这会与第 8 项内容仓库共同完成。
- 多来源提案由 API 和数据库支持；当前 HTML 编辑器先展示一个主要来源，尚缺可视化添加/移除多个来源。
- 时间流和关系图均为可操作候选，但哪种默认投影、关系图密度和移动端手感必须由用户实际体验判断。
- 旧 `/new` 与 `POST /api/projects` 暂保留兼容既有脚本和旧客户端，已从一级导航移除；在迁移策略确定前不冒充“所有项目创建都已强制经过 ProjectProposal”。
- 身份认证、CSRF 和真正的人类权限证明属于第 12 项；当前只能在本机或受控隔离环境使用。
