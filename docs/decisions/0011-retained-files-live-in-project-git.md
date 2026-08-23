# 0011：持久文件只进入项目 Git 或 Git LFS

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

现有实现把项目说明、上传文件、Contribution 附件和 Evidence 附件登记进独立 `artifacts` 表及内容仓库。这使 Artifact 同时承担项目文件、正式成果、输入引用和证明附件，形成了项目 Git 之外的第二套文件历史。

已经确定每个 Project 拥有自己的 Git 仓库和 Git LFS。继续保留长期 Artifact 文件历史会让一个项目无法只通过自己的仓库表达其持久文件，并使 GitHub 远端缺少一部分项目内容。

## 备选方案

- 保留独立 Artifact 文件与版本系统。
- 项目文件进入 Git/Git LFS，证明附件长期保存在独立对象仓库。
- 所有需要长期保留的文件都进入对应项目的 Git/Git LFS，其他文件只作临时数据。

## 决定

所有需要长期保留的用户输入、代码、文档、数据、报告、图片、浏览器 Trace 和其他运行输出，都必须写入当前目标枝干的 worktree，并由该项目的普通 Git 或标准 Git LFS 管理。

`Evidence` 是 PostgreSQL 中的结构化事实和索引。它可以绑定准确的 Git commit、仓库相对路径、内容哈希、ToolCall、环境或外部来源，但不复制被引用的项目文件，也不拥有独立文件版本。

Fudian 不保留长期 `Artifact` 或 `EvidenceAttachment` 文件仓库。服务器对象存储可以承载标准 Git LFS 对象，也可以暂存未完成上传、Runner 输出层和缓存；后者没有项目历史语义。只有被安全选入 worktree 并形成 Git 检查点的输出才获得长期保留，其他临时对象按保留策略清理。

## 影响

- 项目的持久文件可以随其 Git/Git LFS 仓库一起审查、合并、恢复和推送远端。
- Evidence 在目标图中提供可查询的结论，原始证明材料仍由同一项目仓库保存。
- “仅供本次运行”的输入不能成为拟合并所依赖的持久证据；需要引用时必须先导入项目仓库。
- 当前 `artifacts`、`InputArtifact`、项目启动说明下载和 Artifact 前端列表都是待迁移的旧实现。迁移必须保留旧数据可读，把新写入逐步切换到项目 Git/Git LFS，不能改写已经发布的数据库迁移。
- 项目仓库不得保存未经脱敏的密钥、Cookie、Token 或包含秘密的完整日志。

## 相关资料

- [项目文件使用 Git 与标准 Git LFS](0007-standard-git-lfs-for-project-files.md)
- [每个项目使用独立 Git 仓库，不设总 Git](0010-one-repository-per-project.md)
- [产品设计：数据、Git 与项目文件](../product/product-design.md#12-数据git-与项目文件)
