# 0010：每个项目使用独立 Git 仓库，不设总 Git

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

目标枝干已经确定与 Git branch 一一对应，但还需要明确这些 branch 属于哪个仓库，以及 Fudian 是否还要用一个父级 Git 仓库包住所有项目。

把多个项目仓库嵌套进一个“总 Git”不会让父仓库理解子仓库的 commit、branch 或 Git LFS 对象，反而会混淆项目权限、远端、垃圾回收和恢复边界。Fudian 自身源码仓库也只用于开发 Fudian 软件，不能充当运行时项目的父仓库。

## 备选方案

- 所有项目共用一个大型 Git 仓库。
- 每个项目使用独立仓库，再由一个父级 Git 或 submodule 集合管理。
- 每个项目使用独立仓库，Fudian 通过数据库和服务层统一协调，不设置父级 Git。

## 决定

每个正式 Project 拥有一个逻辑独立的托管 Git 仓库及项目级 Git LFS 命名空间。根 GoalBranch 使用该仓库的默认 branch，其他 GoalBranch 使用同一仓库内的独立 branch。

每个项目可以绑定自己对应的 GitHub 仓库作为远端。项目之间不共享 Git 历史、refs、远端或权限边界，也不通过嵌套仓库、submodule 或父级 Git 组成“总 Git”。底层可以共用磁盘、对象存储或 LFS 服务，但所有访问、引用和恢复必须保持项目隔离。

PostgreSQL 管理项目、目标、Session、审批、权限和运行状态；Git 与 Git LFS 管理单个项目的文件及其版本。Fudian 软件自身的源码仓库是另一套独立开发仓库，不在运行时项目仓库层级中。

## 影响

- 创建正式项目时必须原子地准备项目记录和独立仓库身份；失败不能留下可执行的半成品项目。
- 手机和其他客户端通过 Fudian 服务操作服务器上的仓库，不需要自行 clone。
- 整机备份必须一致地覆盖 PostgreSQL、每个项目仓库和对应 LFS 数据，不能依靠父级 Git commit 表示系统快照。
- GitHub 远端配置、凭据和同步状态都按项目隔离。
- 哪些运行材料需要进入项目仓库仍需另行决定，本记录不引入或保留独立的项目文件历史。

## 相关资料

- [目标枝干与 Git branch 一一对应](0003-goal-branch-git-identity.md)
- [项目文件使用 Git 与标准 Git LFS](0007-standard-git-lfs-for-project-files.md)
- [产品设计：数据、Git 与产物](../product/product-design.md#12-数据git-与产物)
