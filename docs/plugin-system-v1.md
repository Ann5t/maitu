# BP-05：中央插件与真实工具目标契约

## 目标

把已有的 Manifest、EnvironmentManifest 和 Mock Broker 原型推进成真正可执行、可复现且不污染目标 worktree 的中央插件系统。Rust、Python、C/C++ 与 Playwright 使用同一调用协议，经 BP-04 的只读输入、独立输出、单写 Lease 和 Git compare-and-swap 回写；Session 只留下源码、输入、报告和选择保留的产物，不留下 `target`、虚拟环境、包缓存、`node_modules`、浏览器安装或系统工具链。

## 硬约束

- 插件身份是准确的 `(pluginId, SemVer, manifestDigest, runtimeImageDigest)`；`latest` 只用于目录解析，不能进入 EnvironmentManifest 或 ToolCall。
- OCI Runtime 必须有受信任发布者签名和成功自检。签名覆盖规范 Manifest、镜像摘要、入口摘要和自检摘要；升级只追加新版本，不覆盖旧包。
- 中央共享的是只读内容寻址镜像/包缓存，不是跨 Session 共享的可变 Python、Rust、C/C++ 或 Node 环境。不同插件版本可以依赖互不兼容的库而并存。
- 插件目录默认只披露短描述、能力、固定版本、安装/信任状态；完整工具 schema、权限、Skill 和资源只在选中后读取。
- ToolCall 必须绑定 Project、GoalBranch、Session、真实 workspace snapshot、EnvironmentManifest 指纹、插件四元组、结构化输入、资源、RunnerJob 和最终 Git snapshot。
- Manifest 只能声明最大需求。实际调用还必须同时满足 BranchProposal 固定权限；断网插件不能获得网络，未实现安全适配器的外部写、账号、付费和部署能力继续明确拒绝。
- 普通工具调用使用一次性 Worker；持续浏览器/开发服务器只能通过有 token、心跳、硬到期和清理语义的 ToolLease。物理调度、重启接管与取消竞态由紧接的 BP-06 完成，BP-05 不留下暗中运行的进程。
- Worker 不获得 Docker Socket、宿主 home、SSH、真实数据库、其他 worktree 或秘密值；日志只以受限报告/摘要回流。
- 真实插件镜像必须能由仓库脚本重复构建。运行时按镜像 ID/摘要启动，不能把可漂移 tag 当作持久证据。
- PPTMaster 等未安装或需要专有授权的能力只通过同一 Manifest 兼容夹具和中央安装请求表达；没有用户授权、许可证和适配器时不得执行。

## 代表性插件

| 插件 | 真实验证 | 留在 worktree 的内容 |
| --- | --- | --- |
| `fudian.tools.rust` | 固定 Rust 工具链对只读源文件执行语法/类型检查 | JSON 报告 |
| `fudian.tools.python` | 两个插件版本分别携带互不兼容的示例库版本并真实运行脚本 | 程序报告，不含虚拟环境/缓存 |
| `fudian.tools.cxx` | 固定 GCC/G++ 对 C 或 C++ 执行 `-fsyntax-only` | JSON 报告 |
| `fudian.tools.playwright` | 固定 Chromium/Playwright 打开本地 HTML、检查页面并截图 | JSON 报告与截图 |
| `fudian.tools.pptmaster` | 只校验 SDK 兼容 Manifest，并形成待安装请求 | 无；不能冒充可运行 |

## 验收表

- [x] 发布者公钥、签名安装、自检、供应链冲突、撤销和追加升级均有数据库约束与 HTTP/CLI 测试。
- [x] 渐进式目录只返回摘要，按需详情返回准确 Manifest、工具 schema、Skill、权限、Runtime 和安装证明。
- [x] EnvironmentManifest 固定准确插件与镜像；同名多版本及 Python 冲突库可在不同 Session 同时执行，`latest` 漂移不改变旧环境。
- [x] Rust、Python、C/C++、Playwright 四类真实 OCI Worker 在断网、只读输入、无能力、资源受限环境运行并经真实 Git CAS 回写。
- [x] 每次真实 ToolCall 绑定 RunnerJob、base/head/tree/snapshot、插件/镜像/环境和输出摘要；重放不重复执行或提交。
- [x] worktree 中没有工具链、包缓存、虚拟环境、`target`、`node_modules`、浏览器资料或跨 Session 可变环境。
- [x] 越权写、错误输入 schema、错误镜像/入口摘要、未签名/撤销插件、环境外插件、同版本双摘要和失败工具均被安全拒绝或暂停。
- [x] ToolLease 的持久身份、token 摘要、心跳时间、软/硬到期和清理状态沿用既有唯一表/领域状态，BP-06 可直接实现物理调度，不另造第二套工具状态。
- [x] PPTMaster 兼容夹具通过统一 SDK 校验，未安装调用产生明确安装请求而不是下载或使用专有软件。
- [x] Rust/SQL/迁移/真实 HTTP+Git+OCI+Chromium 插件流程、生产镜像及完整质量门通过；真实生产数据与容器保持不变。

## 当前未知与判断边界

- MCP 最终采用 stdio、HTTP 还是进程内传输不在本阶段锁定；稳定 ToolCall/ToolResult 和能力语义不依赖传输。
- 联网插件的域名代理和账号注入仍未知。本阶段四个代表插件全部断网运行，不用默认容器网络冒充授权。
- 插件市场、发现/安装页面与发布审批的最终交互归 BP-08/BP-09；本阶段先形成可靠目录 API、签名链和安装请求。
- PPTMaster 的许可证、可执行入口、参数 schema 与文件兼容性需要用户提供真实产品边界后另立插件版本；本阶段只有不执行的兼容夹具。
