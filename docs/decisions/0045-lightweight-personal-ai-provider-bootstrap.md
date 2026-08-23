# 0045：首批 AI 使用轻量个人连接

- 状态：Accepted
- 日期：2026-08-24
- 置信度：高

## 背景

第一版需要真实接入文字 Agent 和图片生成，但不能为了使用 Codex 在服务器安装完整 Codex，也不能把 ChatGPT Pro 的产品内额度误当成通用 OpenAI API 额度。AI 连接还必须符合现有约束：按账号隔离、由 Session 固定版本、不能静默切换模型，并能在后续增加国内 Provider。

Pi 已经把 Codex 账号登录、令牌刷新和 Codex Responses 协议做成独立于完整编码界面的模型连接层，并由自己的 Agent 循环调用。Fudian 可以复用这条轻量连接边界，同时继续由 Rust 核心拥有项目工作流和 Agent 行为。

## 备选方案

- 安装完整 Codex，并把每个 Fudian Session 映射为 Codex 线程。
- 直接在 Rust 中复制当前 Codex 私有协议和客户端身份。
- 用固定版本的 Pi 模型连接组件适配 Codex，Fudian 自己实现 Agent 核心。

## 决定

Fudian 不安装完整 Codex，也不安装 Pi 的终端界面或完整编码 Agent。Rust Agent 核心负责 Session 状态、上下文、工具循环、暂停、恢复和事件持久化；首批 Codex 连接由一个固定版本的 Pi 模型连接组件负责账号登录、令牌刷新、请求转换和流式响应转换。它是 AI 连接下的 Provider Driver，不拥有 GoalBranch、worktree、权限决定或项目数据。

Provider Driver 作为共享的常驻服务或进程运行，不为每个 Session 重复安装。版本、内容摘要和内部协议版本由管理员管理，并进入 Session 的非秘密配置快照。一次调用必须绑定准确的 Fudian 账号和连接；不同账号可以共享驱动程序代码，但不能共享访问令牌、刷新令牌、配额或调用记录。驱动更新只影响新 Session；驱动失效时按既有规则暂停相关 Session，不临时安装 Codex，也不静默切换到 API 计费模型。

第一版的 AI 连接均先按个人使用设计：

- **Codex**：用户以自己的 ChatGPT 账号授权，优先支持适合服务器与手机的设备码流程；凭据只属于该 Fudian 账号。
- **GPT Image 2**：用户提供自己的 OpenAI Platform API Key，承担独立的 API 计费；ChatGPT Pro 图片额度不视为 API 额度。

这两项在界面中是两个独立连接，但模型仍按能力统一配置：Codex 先承担默认文字 Agent，GPT Image 2 提供图片生成与编辑。秘密只保存在服务器的受限凭据存储中，不进入模型上下文、项目 Git/Git LFS、日志或项目导出包。

接入顺序固定为：首批完成 Codex 与 GPT Image 2；之后接入 DeepSeek 与 MiniMax，并先提供文字能力。国内 Provider 的具体认证方式和其他媒体能力在实现前根据其正式接口另行确定。

## 影响

- Fudian 不依赖完整 Codex 安装，Rust 核心也不会被某个模型供应商接管。
- 复用经过实际维护的 Codex 连接层比冒充 Pi 或复制易变协议更稳妥，但会引入一个受版本管理的轻量运行时依赖。
- 本地共享 Driver 不会重复占用每个 Session 的环境空间；模型网络延迟远大于本地一次进程通信，Driver 必须常驻而不能逐次启动。
- 用户需要分别维护 Codex 账号授权和 GPT Image 2 API Key；二者故障、额度与账单相互独立。
- Provider Driver 的统一内部合同应允许后续增加 DeepSeek、MiniMax 或替换 Codex 适配实现，而不改写 Agent 核心。

## 相关资料

- [AI 连接与按模型能力配置相互分离](0031-capability-based-ai-configuration.md)
- [Session 固定 AI 配置且第一版不自动换模型](0042-session-pinned-ai-without-fallback.md)
- [管理员安装插件且 Session 固定工具版本](0025-admin-plugin-install-and-pinned-session-tools.md)
- [Pi 的 OpenAI Codex 登录实现](https://github.com/earendil-works/pi/blob/main/packages/ai/src/auth/oauth/openai-codex.ts)
- [Pi 的 Codex Responses 适配实现](https://github.com/earendil-works/pi/blob/main/packages/ai/src/api/openai-codex-responses.ts)
- [OpenAI API 认证](https://developers.openai.com/api/reference/overview#authentication)
- [GPT Image 2 图片接口](https://developers.openai.com/api/docs/guides/image-generation)
