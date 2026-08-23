# 文档指南

> 状态：当前工程规则。

## 采用的原则

本项目使用“项目规则优先、官方指南补充”的方式维护文档：

- [Cargo 包布局](https://doc.rust-lang.org/cargo/guide/project-layout.html)用于保持 Rust 源码、可执行文件和测试位置符合生态惯例。
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/documentation.html)和 [rustdoc book](https://doc.rust-lang.org/stable/rustdoc/index.html)用于公共 Rust 接口、示例和文档测试。
- [Google developer documentation style guide](https://developers.google.com/style)用于清晰、一致、面向开发者的技术表达。
- [Microsoft 的可扫描内容指南](https://learn.microsoft.com/en-us/style-guide/scannable-content/)用于先说结论、短段落、稳定标题和长文导航。
- [GitHub README 指南](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)用于仓库入口、相对链接和贡献入口。
- [Microsoft ADR 指南](https://learn.microsoft.com/en-us/azure/well-architected/architect-role/architecture-decision-record)用于记录重要且难以逆转的选择。
- [Diátaxis](https://diataxis.fr/)用于区分教程、操作指南、参考和解释；本仓库目前主要使用后三类。
- [Google 工程评审实践](https://google.github.io/eng-practices/review/reviewer/looking-for.html)用于要求行为变化与对应文档、测试在同一改动中完成。

## 信息架构

- `product/`：产品目标、用户体验和仍在讨论的方向。
- `architecture/`：当前实现的解释和领域边界。
- `reference/`：API、协议、字段和路由。
- `operations/`：完成部署、备份和恢复任务的操作指南。
- `development/`：贡献者需要的仓库、文档和测试规则。
- `decisions/`：追加式 ADR。
- `archive/`：过去的计划、执行日志、验收报告和被替代原型。

不要按日期、开发者或临时 Goal 在 `docs/` 根目录继续堆文件。历史材料进入 `archive/`；现行文档按读者用途归位。

## 每份文档的规则

1. 只解决一个主要问题，并在开头给出用途或状态。
2. 使用一个一级标题；标题层级连续，名称具体。
3. 先写结论、前提或最终操作，再补充背景。
4. 段落保持短小；有顺序才使用编号列表，有可比较字段才使用表格。
5. 命令、路径、类型和字面值使用反引号；链接文字描述目标，不直接裸露 URL。
6. 仓库内使用相对链接，移动文件时同时修复链接。
7. 不复制另一页的长篇状态清单；指定权威页面并链接。
8. 示例不得包含真实 secret、Token、Cookie、邮箱或服务器地址。
9. 截图只用于证明视觉或响应式行为；可由测试重建的截图放在 `docs/assets/screenshots/`。
10. 过时内容不悄悄删除。保留历史价值的移入 `archive/`，并清楚标明不再定义当前行为。

文件名使用小写 kebab-case。只有真实 API、协议或领域 schema 版本才保留 `v1`、`v2`；Git 已经负责普通文档修订历史，不在文件名中添加随意的 `final`、`new` 或副本编号。

## ADR 规则

ADR 只记录会影响系统结构、关键质量属性或难以逆转的决定。记录必须包含背景、备选方案、结果、影响、状态和置信度。

已接受 ADR 不直接改写结论。方向变化时创建新 ADR，把旧记录标为 `Superseded` 并互相链接。讨论草稿和长篇方案留在产品或架构文档，ADR 只保留可独立理解的决定。

## 自动检查

```bash
./scripts/check-docs.py
```

检查器验证受版本控制的 Markdown 文件只有一个一级标题，并验证本地相对链接指向现有文件。完整质量门也会运行该检查。
