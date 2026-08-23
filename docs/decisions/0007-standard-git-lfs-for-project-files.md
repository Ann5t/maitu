# 0007：项目文件使用 Git 与标准 Git LFS

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

此前设计把“代码与文本”交给 Git，把“大型或二进制资料”默认放进独立 Artifact 仓库。这会让同一个项目的可编辑文件分属两套版本历史，也迫使 Fudian 定义自有对象指针和合并语义。

Git 本身可以保存任意文件；Git LFS 在 Git 中保存标准文本指针，把大文件内容交给 LFS 服务，同时继续使用相同的 branch、commit 和 merge 工作流。

## 备选方案

- 普通 Git 加 Fudian 自定义对象指针协议。
- 文本使用 Git，所有二进制使用独立 Artifact 版本库。
- 项目文件统一归 Git，普通文件直接存储，大文件使用标准 Git LFS。

## 决定

所有属于项目工作树、需要随目标枝干分叉、审查和合并的文件都由 Git 管理。适合直接存放的文件使用普通 Git；不适合直接存放的大文件使用标准 Git LFS。Fudian 不实现自定义大文件指针协议。

分流不只依据文本或二进制：小型二进制可以直接进入 Git，大型文本也可以进入 LFS。仓库通过受版本控制的 `.gitattributes` 固定文件匹配规则；具体默认扩展名和大小阈值后续单独确定。

用户导入文件默认复制到当前枝干 inbox，再按仓库规则进入普通 Git 或 Git LFS。只有明确作为参考、证据、运行输出或临时对象的内容才留在 Artifact 仓库；Artifact 不是另一套项目文件历史。

Git LFS 服务的实际对象可以保存在本地文件系统、S3 或 MinIO。物理后端不改变 Git LFS 指针、传输和 Git 历史语义。

## 影响

- `.docx`、`.pptx`、`.psd`、`.prproj` 等可编辑项目文件可以正常随枝干工作，而不需要 Fudian 专用客户端理解自定义指针。
- Git LFS 只改变大文件存储和传输，不让二进制内容自动可合并；并行编辑和锁定策略仍需单独决定。
- 当前仓库没有 `.gitattributes`，运行环境也未安装 Git LFS；现有 InputArtifact 实现仍把大文件默认导入为 Artifact 引用，后续必须增量迁移。
- 旧 Artifact 和输入记录继续可读，不改写已发布迁移或历史数据。

## 相关资料

- [产品设计：数据、Git 与产物](../product/product-design.md#12-数据git-与产物)
- [Git LFS 官方规范](https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md)
- [Git LFS 官方说明](https://git-lfs.com/)
