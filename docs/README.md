# 脉图文档

这里按读者要解决的问题组织文档，而不是按开发时间平铺文件。根 [README](../README.md) 负责快速开始；本页负责完整导航。

## 脉图当前入口

- [首版范围](product/maitu-scope.md)
- [实施进度](development/maitu-progress.md)
- [旧项目来源](product/source-projects.md)
- [产品设计建议](product/maitu-design.md)

以下为继承的浮点文档导航，各页保留原始状态和背景。

## 从哪里开始

| 需求 | 入口 | 状态 |
| --- | --- | --- |
| 理解产品想解决什么 | [产品设计](product/README.md) | 讨论中 |
| 理解当前代码怎样运行 | [实现架构](architecture/README.md) | 已实现快照 |
| 查找 API 和协议字段 | [接口参考](reference/README.md) | 已实现参考 |
| 部署、备份或恢复 | [运维指南](operations/README.md) | 当前操作指南 |
| 修改代码或文档 | [开发指南](development/README.md) | 当前工程规则 |
| 理解重要选择的理由 | [架构决策](decisions/README.md) | 追加式记录 |
| 查阅过去的执行证据 | [历史归档](archive/README.md) | 非现行规范 |

## 文档状态

- **讨论中**：方向仍在与用户讨论，不能当作最终契约。
- **已接受**：用户已经接受的产品或架构决定；改变时新增决策记录。
- **已实现快照**：描述当前代码和测试能证明的行为，不承诺未来方向。
- **当前操作指南**：应与可执行脚本和配置保持同步。
- **历史归档**：保留当时计划、过程和证据，不定义当前行为。

运行代码、迁移和自动化测试是“现在实现了什么”的最终证据；产品文档描述“希望建设什么”；ADR 说明“为什么选择”。三者用途不同，不能互相冒充。

## 维护规则

文档分类、标题、链接、状态和 ADR 规则见[文档指南](development/documentation.md)。仓库布局见[仓库布局](development/repository-layout.md)。所有本地 Markdown 链接由 `./scripts/check-docs.py` 检查。
