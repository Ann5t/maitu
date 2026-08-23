# 0048：第一版使用 OCI Worker 与原生依赖后端

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

统一工具目录、Session 环境隔离、不可变版本与热 Worker 已经确定，但底层仍可以采用一个可变的大容器、Nix 风格总环境，或 OCI Worker 配合各语言自己的依赖系统。

Fudian 既要直接处理普通 GitHub 项目，也要逐步接入 Rust、Python、Node、C/C++、Playwright 和专业软件。第一版如果要求所有项目先进入一种新的总包管理体系，会提高兼容、打包和调试成本；如果只提供一个长期可变的工具容器，又无法可靠隔离冲突版本。

## 备选方案

- 所有 Session 共用一个安装了全部工具的可变 Linux 容器。
- 使用 Nix 风格内容存储统一描述和构建全部系统与语言依赖。
- 使用 OCI 隔离系统运行层，各语言保留原生锁文件和解析器，由 Fudian 在上层统一环境身份、权限和调用。

## 决定

### 统一产品，隔离 Worker

用户与 Agent 仍只看到一个工具系统。Tool Broker 根据 Session、准确的 `EnvironmentManifest` 和权限启动 OCI 兼容的隔离 Worker；部署内部可以存在多个按需 Worker，不能把“统一工具系统”实现成所有 Session 共同修改的一个大容器。

每个 Worker 拥有独立的可写 home、进程、临时目录、构建目录和输出层。不同 Session 只共享不可变内容，不能共享可变 Python 环境、`node_modules`、进程或未经完整输入键控的构建状态。

### 原生依赖后端

系统工具链和浏览器由固定摘要的 OCI 基础层提供。语言依赖继续使用项目真实声明与锁文件：Rust 使用 rustup/Cargo，Python 环境可以由 uv 或 micromamba 适配器解析，Node 使用 Corepack/pnpm，C/C++ 使用项目声明的构建与依赖工具，Playwright 使用固定的浏览器运行层。具体解析器及其版本都进入 `EnvironmentManifest`，不能依赖服务器上的隐式默认版本。

Agent 正常修改 `Cargo.toml`、`Cargo.lock`、`pyproject.toml`、`uv.lock`、`environment.yml`、`package.json`、`pnpm-lock.yaml` 或项目已有的等价声明。Broker 解析这些文件并生成新的不可变环境版本。普通项目依赖变动不要求管理员逐次批准；新增服务器级平台工具、插件、秘密或更大外部权限仍走原有批准边界。

### 去重与缓存边界

OCI 层按内容摘要复用。各依赖后端的下载包、源码归档和经过完整输入哈希校验的安全编译缓存由中央存储复用；环境目录通过只读引用、内容寻址对象或可验证链接组合，不能用一个跨 Session 可任意修改的缓存目录冒充环境。

共享只是一项存储和启动优化。项目锁文件、基础镜像摘要、解析器版本、目标平台、Feature、权限与构建参数共同形成环境指纹；相同内容只保存一份，不同指纹可以并存。

### 调用速度

连续工具调用可以复用仅属于同一 Session 与准确环境指纹的热 Worker。环境变化、租约过期、Session 结束、异常或资源超限后销毁；销毁后必须能完全由 `EnvironmentManifest` 和持久项目文件重建。

### Nix 边界

第一版不把 Nix 作为全部环境的必经层，也不要求普通项目增加 Nix 配置。未来若性能和复现测试证明有价值，可以把 Nix 增加为 Tool Broker 的另一种环境后端；它必须遵守相同的环境身份、权限、输出写回和审计合同，不能形成第二套用户产品。

## 影响

- 普通项目可以沿用已有锁文件和 GitHub CI 习惯，不必先改写为 Fudian 或 Nix 专用工程。
- OCI 提供进程与系统层隔离，原生解析器保持语言生态兼容；Fudian 负责把多套后端统一为一个环境与调用模型。
- 去重粒度通常不如完全由 Nix 构建的内容存储天然统一，因此必须专门验证 OCI 层、包缓存和编译缓存的空间效果。
- 第一版需要为不同生态实现少量环境适配器，但不需要重新实现 Cargo、Python、Node 或 C/C++ 包管理器。
- 实现验收必须覆盖冲突版本并行、相同内容去重、环境重建、热 Worker 延迟和可变状态不串用。

## 相关资料

- [统一工具目录复用不可变资源并隔离 Session 环境](0024-unified-tools-with-isolated-reused-environments.md)
- [管理员安装插件且 Session 固定工具版本](0025-admin-plugin-install-and-pinned-session-tools.md)
- [工具能力由环境、适配器和 Agent 说明组合](0026-composable-tool-capability-model.md)
- [产品设计：版本与依赖隔离](../product/product-design.md#113-版本与依赖隔离)
